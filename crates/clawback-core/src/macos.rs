//! Darwin bulk directory metadata. The wire parser is portable so malformed
//! records can be tested on every platform; only macOS links the native API.
use std::io;

#[cfg(target_os = "macos")]
mod reader;
#[cfg(target_os = "macos")]
pub(crate) use reader::read_dir;

// Darwin sys/attr.h. No FSOPT_PACK_INVAL_ATTRS: absent fields are not packed.
const NAME: u32 = 0x0000_0001;
const DEVICE: u32 = 0x0000_0002;
const OBJECT_TYPE: u32 = 0x0000_0008;
const MOD_TIME: u32 = 0x0000_0400;
const FILE_ID: u32 = 0x0200_0000;
const ERROR: u32 = 0x2000_0000;
const RETURNED: u32 = 0x8000_0000;
const COMMON: u32 = NAME | DEVICE | OBJECT_TYPE | MOD_TIME | FILE_ID | ERROR | RETURNED;
const LINK_COUNT: u32 = 0x1;
const ALLOC_SIZE: u32 = 0x4;
const DATA_LENGTH: u32 = 0x200;
const FILE: u32 = LINK_COUNT | ALLOC_SIZE | DATA_LENGTH;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Metadata {
    device: u64,
    inode: u64,
    modified: i64,
    allocated: u64,
    length: u64,
    links: u64,
}

#[derive(Debug)]
struct Record {
    name: Vec<u8>,
    // Only complete regular-file metadata takes the fast path. Directories
    // must be stat'ed to resolve mount points and System/Data firmlinks.
    metadata: Option<Metadata>,
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid macOS bulk directory metadata")
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn take<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let end = self.pos.checked_add(N).ok_or_else(invalid)?;
        let value = self.data.get(self.pos..end).ok_or_else(invalid)?.try_into().map_err(|_| invalid())?;
        self.pos = end;
        Ok(value)
    }

    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_ne_bytes(self.take()?))
    }

    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_ne_bytes(self.take()?))
    }

    fn optional_u32(&mut self, mask: u32, bit: u32) -> io::Result<Option<u32>> {
        if mask & bit != 0 { self.u32().map(Some) } else { Ok(None) }
    }

    fn optional_u64(&mut self, mask: u32, bit: u32) -> io::Result<Option<u64>> {
        if mask & bit != 0 { self.u64().map(Some) } else { Ok(None) }
    }
}

fn parse_record(data: &[u8]) -> io::Result<Record> {
    let mut c = Cursor { data, pos: 4 };
    let common = c.u32()?;
    let volume = c.u32()?;
    let directory = c.u32()?;
    let file = c.u32()?;
    let fork = c.u32()?;
    if common & (NAME | RETURNED) != NAME | RETURNED
        || common & !COMMON != 0
        || file & !FILE != 0
        || volume | directory | fork != 0
    {
        return Err(invalid());
    }
    // ERROR is a documented exception to attribute bitmap order.
    let error = c.optional_u32(common, ERROR)?.unwrap_or(0);
    let reference = c.pos;
    let offset = c.u32()? as i32;
    let length = c.u32()? as usize;
    let name_start = reference.checked_add_signed(offset as isize).ok_or_else(invalid)?;
    let name_end = name_start.checked_add(length).ok_or_else(invalid)?;
    let name = data.get(name_start..name_end).ok_or_else(invalid)?;
    let name = name.strip_suffix(&[0]).ok_or_else(invalid)?;
    if name.is_empty() || name == b"." || name == b".." || name.contains(&0) || name.contains(&b'/') {
        return Err(invalid());
    }
    let device = c.optional_u32(common, DEVICE)?;
    let object_type = c.optional_u32(common, OBJECT_TYPE)?;
    let modified = if common & MOD_TIME != 0 {
        let seconds = c.u64()? as i64;
        let nanos = c.u64()?;
        if nanos >= 1_000_000_000 {
            return Err(invalid());
        }
        Some(seconds)
    } else {
        None
    };
    let inode = c.optional_u64(common, FILE_ID)?;
    let links = c.optional_u32(file, LINK_COUNT)?;
    let allocated = c.optional_u64(file, ALLOC_SIZE)?;
    let length = c.optional_u64(file, DATA_LENGTH)?;
    if name_start < c.pos
        || allocated.is_some_and(|n| n > i64::MAX as u64)
        || length.is_some_and(|n| n > i64::MAX as u64)
    {
        return Err(invalid());
    }
    let metadata = match (error, object_type, device, inode, modified, links, allocated, length) {
        (0, Some(1), Some(dev), Some(ino), Some(time), Some(links), Some(size), Some(len)) => Some(Metadata {
            device: i64::from(dev as i32) as u64,
            inode: ino,
            modified: time,
            allocated: size,
            length: len,
            links: u64::from(links),
        }),
        _ => None,
    };
    Ok(Record { name: name.to_vec(), metadata })
}

/// Validate a whole syscall batch before publishing any of it. All reads are
/// bounded byte copies: Darwin aligns 64-bit fields to four bytes, not eight.
fn parse_batch(mut data: &[u8], count: usize) -> io::Result<Vec<Record>> {
    if count > data.len() / 24 {
        return Err(invalid());
    }
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        let length = Cursor { data, pos: 0 }.u32()? as usize;
        if length < 24 || !length.is_multiple_of(8) {
            return Err(invalid());
        }
        let record = data.get(..length).ok_or_else(invalid)?;
        records.push(parse_record(record)?);
        data = &data[length..];
    }
    Ok(records)
}

#[cfg(test)]
mod tests;
