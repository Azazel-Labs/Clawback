//! Bounded NTFS 3.x record parser. No packed-structure casts or unchecked disk
//! offsets. The Windows reader below uses read-only volume handles; unsupported
//! or inconsistent metadata makes the caller fall back to directory traversal.
use std::io;

const INDEX_MASK: u64 = (1 << 48) - 1;

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Unsupported or inconsistent NTFS metadata")
}

fn bytes(data: &[u8], start: usize, len: usize) -> io::Result<&[u8]> {
    data.get(start..start.checked_add(len).ok_or_else(invalid)?).ok_or_else(invalid)
}

fn u16_at(data: &[u8], start: usize) -> io::Result<u16> {
    Ok(u16::from_le_bytes(bytes(data, start, 2)?.try_into().map_err(|_| invalid())?))
}

fn u32_at(data: &[u8], start: usize) -> io::Result<u32> {
    Ok(u32::from_le_bytes(bytes(data, start, 4)?.try_into().map_err(|_| invalid())?))
}

fn u64_at(data: &[u8], start: usize) -> io::Result<u64> {
    Ok(u64::from_le_bytes(bytes(data, start, 8)?.try_into().map_err(|_| invalid())?))
}

/// Raw disk records require update-sequence fixups for every 512-byte stride,
/// even on volumes with larger physical sectors. FSCTL records are already fixed.
fn fixup(record: &mut [u8]) -> io::Result<()> {
    let offset = usize::from(u16_at(record, 4)?);
    let count = usize::from(u16_at(record, 6)?);
    if !record.len().is_multiple_of(512)
        || count != record.len() / 512 + 1
        || offset < 8
        || offset.checked_add(count * 2).is_none_or(|end| end > 510)
    {
        return Err(invalid());
    }
    let replacements = bytes(record, offset, count * 2)?.to_vec();
    for i in 1..count {
        let end = i * 512;
        if record[end - 2..end] != replacements[..2] {
            return Err(invalid()); // torn or changing record, never trust its sizes
        }
        record[end - 2..end].copy_from_slice(&replacements[i * 2..i * 2 + 2]);
    }
    Ok(())
}

struct Attribute<'a> {
    kind: u32,
    data: &'a [u8],
}

impl Attribute<'_> {
    fn resident(&self) -> io::Result<&[u8]> {
        if self.data[8] != 0 {
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
}

fn attributes(record: &[u8]) -> io::Result<Vec<Attribute<'_>>> {
    if bytes(record, 0, 4)? != b"FILE" {
        return Err(invalid());
    }
    let used = u32_at(record, 24)? as usize;
    let record = bytes(record, 0, used)?;
    let mut offset = usize::from(u16_at(record, 20)?);
    if offset < 42 || !offset.is_multiple_of(8) {
        return Err(invalid());
    }
    let mut result = Vec::new();
    loop {
        let kind = u32_at(record, offset)?;
        if kind == u32::MAX {
            return Ok(result);
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
        result.push(Attribute { kind, data });
        offset += len;
    }
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

#[derive(Debug, PartialEq, Eq)]
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
    if bytes(record, 0, 4)? != b"FILE" || index > INDEX_MASK {
        return Err(invalid());
    }
    let flags = u16_at(record, 22)?;
    if flags & 1 == 0 {
        return Ok(None);
    }
    let mut result = Record {
        id: index | (u64::from(u16_at(record, 16)?) << 48),
        base: u64_at(record, 32)?,
        directory: flags & 2 != 0,
        surrogate: false,
        names: Vec::new(),
        size: None,
        mtime: i64::MIN,
    };
    for attr in attributes(record)? {
        match attr.kind {
            0x10 => {
                let data = attr.resident()?;
                let time = u64_at(data, 8)? / 10_000_000;
                result.mtime = i64::try_from(time).map_err(|_| invalid())? - 11_644_473_600;
            }
            0x30 => {
                let data = attr.resident()?;
                let count = usize::from(*data.get(64).ok_or_else(invalid)?);
                let namespace = *data.get(65).ok_or_else(invalid)?;
                if namespace > 3 {
                    return Err(invalid());
                }
                if namespace == 2 {
                    continue;
                } // DOS alias, not another hard link
                let text: Vec<_> =
                    bytes(data, 66, count * 2)?.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
                if text.is_empty() || text.iter().any(|&c| c == 0 || c == 47 || c == 92) || text == [46, 46] {
                    return Err(invalid());
                }
                result.names.push(Name { parent: u64_at(data, 0)?, text });
            }
            0x80 if attr.unnamed() => {
                let size = if attr.data[8] == 0 {
                    Some((0, attr.resident()?.len() as u64))
                } else if u64_at(attr.data, 16)? == 0 {
                    let allocated_offset = if u16_at(attr.data, 12)? & 0x80ff != 0 { 64 } else { 40 };
                    let header_len = if allocated_offset == 64 { 72 } else { 64 };
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
            0xc0 => {
                // Name-surrogate tags include junctions and symbolic links.
                // Other reparse points (e.g. cloud files) retain their data size.
                let tag = if attr.data[8] == 0 {
                    u32_at(attr.resident()?, 0)?
                } else if u64_at(attr.data, 16)? == 0 {
                    read_reparse_tag(attr.data)?
                } else {
                    // The tag is in the first extent, possibly in another record.
                    continue;
                };
                result.surrogate |= tag & 0x2000_0000 != 0;
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
