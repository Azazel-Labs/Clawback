//! Bounded NTFS 3.x record parser. No packed-structure casts or unchecked disk
//! offsets. The Windows reader below uses read-only volume handles; unsupported
//! or inconsistent metadata makes the caller fall back to directory traversal.
use std::io;

const INDEX_MASK: u64 = (1 << 48) - 1;

// Attribute type codes.
const STANDARD_INFORMATION: u32 = 0x10;
#[cfg_attr(not(windows), allow(dead_code))]
const ATTRIBUTE_LIST: u32 = 0x20;
const FILE_NAME: u32 = 0x30;
const DATA: u32 = 0x80;
const REPARSE_POINT: u32 = 0xc0;
const END: u32 = u32::MAX;

// FILE record header flags.
const IN_USE: u16 = 1;
const IS_DIRECTORY: u16 = 2;
/// Compression-unit mask plus the sparse bit of a nonresident attribute.
const COMPRESSED_OR_SPARSE: u16 = 0x80ff;
/// Reparse tags with this bit (junctions, symbolic links) name another file.
const NAME_SURROGATE: u32 = 0x2000_0000;
/// A short 8.3 alias, not another hard link.
const DOS_NAMESPACE: u8 = 2;
/// Update sequences protect the last two bytes of every stride this long.
const FIXUP_STRIDE: usize = 512;

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Unsupported or inconsistent NTFS metadata")
}

fn bytes(data: &[u8], start: usize, len: usize) -> io::Result<&[u8]> {
    data.get(start..start.checked_add(len).ok_or_else(invalid)?).ok_or_else(invalid)
}

fn array<const N: usize>(data: &[u8], start: usize) -> io::Result<[u8; N]> {
    data.get(start..).and_then(<[u8]>::first_chunk).copied().ok_or_else(invalid)
}

fn u16_at(data: &[u8], start: usize) -> io::Result<u16> {
    array(data, start).map(u16::from_le_bytes)
}

fn u32_at(data: &[u8], start: usize) -> io::Result<u32> {
    array(data, start).map(u32::from_le_bytes)
}

fn u64_at(data: &[u8], start: usize) -> io::Result<u64> {
    array(data, start).map(u64::from_le_bytes)
}

fn is_file_record(record: &[u8]) -> io::Result<bool> {
    Ok(bytes(record, 0, 4)? == b"FILE")
}

/// Raw disk records require update-sequence fixups for every 512-byte stride,
/// even on volumes with larger physical sectors. FSCTL records are already fixed.
fn fixup(record: &mut [u8]) -> io::Result<()> {
    let offset = usize::from(u16_at(record, 4)?);
    let count = usize::from(u16_at(record, 6)?);
    if !record.len().is_multiple_of(FIXUP_STRIDE)
        || count != record.len() / FIXUP_STRIDE + 1
        || offset < 8
        || offset.checked_add(count * 2).is_none_or(|end| end > FIXUP_STRIDE - 2)
    {
        return Err(invalid());
    }
    // The update sequence array lies inside the first stride, before any
    // bytes it restores, so it can be read in place.
    let check = array::<2>(record, offset)?;
    for i in 1..count {
        let end = i * FIXUP_STRIDE;
        if record[end - 2..end] != check {
            return Err(invalid()); // torn or changing record, never trust its sizes
        }
        record.copy_within(offset + i * 2..offset + i * 2 + 2, end - 2);
    }
    Ok(())
}

struct Attribute<'a> {
    kind: u32,
    data: &'a [u8],
}

impl Attribute<'_> {
    fn is_resident(&self) -> bool {
        self.data[8] == 0
    }

    fn resident(&self) -> io::Result<&[u8]> {
        if !self.is_resident() {
            return Err(invalid());
        }
        let offset = usize::from(u16_at(self.data, 20)?);
        if offset < 24 {
            return Err(invalid());
        }
        bytes(self.data, offset, u32_at(self.data, 16)? as usize)
    }

    fn unnamed(&self) -> bool {
        self.data[9] == 0
    }

    /// The unnamed `$DATA` stream: the file's contents.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn is_contents(&self) -> bool {
        self.kind == DATA && self.unnamed()
    }

    /// Nonresident attributes can span records; only the first extent holds
    /// the stream sizes.
    fn is_first_extent(&self) -> io::Result<bool> {
        Ok(u64_at(self.data, 16)? == 0)
    }
}

/// The record's attributes, validated as they are read.
fn attributes(record: &[u8]) -> io::Result<impl Iterator<Item = io::Result<Attribute<'_>>>> {
    if !is_file_record(record)? {
        return Err(invalid());
    }
    let used = u32_at(record, 24)? as usize;
    let record = bytes(record, 0, used)?;
    let mut offset = usize::from(u16_at(record, 20)?);
    if offset < 42 || !offset.is_multiple_of(8) {
        return Err(invalid());
    }
    let mut done = false;
    Ok(std::iter::from_fn(move || {
        if done {
            return None;
        }
        let next = attribute_at(record, offset);
        match &next {
            Ok(Some(attr)) => offset += attr.data.len(),
            _ => done = true,
        }
        next.transpose()
    }))
}

fn attribute_at(record: &[u8], offset: usize) -> io::Result<Option<Attribute<'_>>> {
    let kind = u32_at(record, offset)?;
    if kind == END {
        return Ok(None);
    }
    let len = u32_at(record, offset + 4)? as usize;
    if len < 24 || !len.is_multiple_of(8) {
        return Err(invalid());
    }
    let data = bytes(record, offset, len)?;
    if data[8] > 1 || (data[8] == 1 && len < 64) {
        return Err(invalid());
    }
    if data[9] != 0 {
        bytes(data, usize::from(u16_at(data, 10)?), usize::from(data[9]) * 2)?;
    }
    Ok(Some(Attribute { kind, data }))
}

#[derive(Debug, PartialEq, Eq)]
struct Run {
    vcn: u64,
    clusters: u64,
    lcn: Option<u64>,
}

fn runs(attribute: &[u8]) -> io::Result<Vec<Run>> {
    if *attribute.get(8).ok_or_else(invalid)? != 1 {
        return Err(invalid());
    }
    let mut vcn = u64_at(attribute, 16)?;
    let high = u64_at(attribute, 24)?;
    let mut offset = usize::from(u16_at(attribute, 32)?);
    if offset < 64 {
        return Err(invalid());
    }
    let mut lcn = 0i64;
    let mut result = Vec::new();
    loop {
        let header = *attribute.get(offset).ok_or_else(invalid)?;
        offset += 1;
        if header == 0 {
            break;
        }
        let length_bytes = usize::from(header & 15);
        let delta_bytes = usize::from(header >> 4);
        if length_bytes == 0 || length_bytes > 8 || delta_bytes > 8 {
            return Err(invalid());
        }
        let mut value = [0u8; 8];
        value[..length_bytes].copy_from_slice(bytes(attribute, offset, length_bytes)?);
        let clusters = u64::from_le_bytes(value);
        offset += length_bytes;
        if clusters == 0 {
            return Err(invalid());
        }
        let physical = if delta_bytes == 0 {
            None
        } else {
            let delta = bytes(attribute, offset, delta_bytes)?;
            let mut value = [if delta[delta_bytes - 1] & 128 != 0 { 255 } else { 0 }; 8];
            value[..delta_bytes].copy_from_slice(delta);
            lcn = lcn.checked_add(i64::from_le_bytes(value)).ok_or_else(invalid)?;
            Some(u64::try_from(lcn).map_err(|_| invalid())?)
        };
        offset += delta_bytes;
        result.push(Run { vcn, clusters, lcn: physical });
        vcn = vcn.checked_add(clusters).ok_or_else(invalid)?;
    }
    if vcn != high.checked_add(1).ok_or_else(invalid)? {
        return Err(invalid());
    }
    Ok(result)
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Name {
    parent: u64,
    text: Vec<u16>,
}

#[derive(Debug)]
struct Record {
    id: u64,
    base: u64,
    directory: bool,
    surrogate: bool,
    names: Vec<Name>,
    size: Option<(u64, u64)>, // allocated, logical; unnamed data stream only
    mtime: i64,
}

#[cfg(test)]
fn parse(record: &[u8], index: u64) -> io::Result<Option<Record>> {
    parse_with_reparse(record, index, |_| Err(invalid()))
}

fn parse_with_reparse(
    record: &[u8],
    index: u64,
    mut read_reparse_tag: impl FnMut(&[u8]) -> io::Result<u32>,
) -> io::Result<Option<Record>> {
    if index > INDEX_MASK {
        return Err(invalid());
    }
    let attributes = attributes(record)?;
    let flags = u16_at(record, 22)?;
    if flags & IN_USE == 0 {
        return Ok(None);
    }
    let mut result = Record {
        id: index | (u64::from(u16_at(record, 16)?) << 48),
        base: u64_at(record, 32)?,
        directory: flags & IS_DIRECTORY != 0,
        surrogate: false,
        names: Vec::new(),
        size: None,
        mtime: i64::MIN,
    };
    for attr in attributes {
        let attr = attr?;
        match attr.kind {
            STANDARD_INFORMATION => {
                let data = attr.resident()?;
                result.mtime = crate::format::unix_from_filetime(u64_at(data, 8)?).ok_or_else(invalid)?;
            }
            FILE_NAME => {
                let data = attr.resident()?;
                let count = usize::from(*data.get(64).ok_or_else(invalid)?);
                let namespace = *data.get(65).ok_or_else(invalid)?;
                if namespace > 3 {
                    return Err(invalid());
                }
                if namespace == DOS_NAMESPACE {
                    continue;
                }
                let text: Vec<_> =
                    bytes(data, 66, count * 2)?.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
                let forbidden = [0, u16::from(b'/'), u16::from(b'\\')];
                if text.is_empty() || text.iter().any(|c| forbidden.contains(c)) || text == [u16::from(b'.'); 2] {
                    return Err(invalid());
                }
                result.names.push(Name { parent: u64_at(data, 0)?, text });
            }
            DATA if attr.unnamed() => {
                let size = if attr.is_resident() {
                    Some((0, attr.resident()?.len() as u64))
                } else if attr.is_first_extent()? {
                    let (allocated_offset, header_len) =
                        if u16_at(attr.data, 12)? & COMPRESSED_OR_SPARSE != 0 { (64, 72) } else { (40, 64) };
                    if usize::from(u16_at(attr.data, 32)?) < header_len {
                        return Err(invalid());
                    }
                    let size = (u64_at(attr.data, allocated_offset)?, u64_at(attr.data, 48)?);
                    if size.0 > i64::MAX as u64 || size.1 > i64::MAX as u64 {
                        return Err(invalid());
                    }
                    Some(size)
                } else {
                    None
                };
                if let Some(size) = size
                    && result.size.replace(size).is_some()
                {
                    return Err(invalid());
                }
            }
            REPARSE_POINT => {
                // Name-surrogate tags include junctions and symbolic links.
                // Other reparse points (e.g. cloud files) retain their data size.
                let tag = if attr.is_resident() {
                    u32_at(attr.resident()?, 0)?
                } else if attr.is_first_extent()? {
                    read_reparse_tag(attr.data)?
                } else {
                    // The tag is in the first extent, possibly in another record.
                    continue;
                };
                result.surrogate |= tag & NAME_SURROGATE != 0;
            }
            _ => {}
        }
    }
    Ok(Some(result))
}

#[cfg(windows)]
mod reader;
#[cfg(windows)]
pub(crate) use reader::scan;

#[cfg(test)]
mod tests;
