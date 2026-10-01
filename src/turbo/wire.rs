//! Versioned, bounded, buffered snapshot stream. No serde, temporary files,
//! per-file IPC messages, or per-file metadata queries.
//!
//! GUI → helper: `MAGIC`, root, options, then `Command` bytes.
//! Helper → GUI: `Tag`-prefixed progress packets, then one tree or error.
use clawback_core::{
    ROOT, ScanOptions, Tree,
    scan::{MftPhase, MftProgress, ScanBackend, ScanResult},
    tree::{Kind, NewEntry, flags},
};
use std::{
    ffi::{OsStr, OsString},
    io::{self, Read, Write},
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::Path,
    time::Duration,
};

pub const MAGIC: &[u8; 8] = b"CLAWMFT2";
/// Longest string on the wire, in UTF-16 units: the Windows path limit.
const MAX_UNITS: u64 = 32767;
const OPTION_APPARENT_SIZE: u8 = 1;
const OPTION_DEDUPE_HARDLINKS: u8 = 2;
/// Node flags a fresh scan can produce; `REMOVED` is UI-only.
const SCAN_FLAGS: u8 = flags::DENIED | flags::PARTIAL | flags::OTHER_FS | flags::VIRTUAL | flags::HARDLINK_DUP;
/// Error kinds that survive the trip, by wire code; anything else is `Other` (code 0).
const ERROR_KINDS: [io::ErrorKind; 3] =
    [io::ErrorKind::Other, io::ErrorKind::Unsupported, io::ErrorKind::PermissionDenied];

pub fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid Turbo protocol")
}

/// Helper → GUI message tags.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
    Tree = 1,
    Failed = 2,
    Progress = 3,
}
impl TryFrom<u8> for Tag {
    type Error = io::Error;
    fn try_from(value: u8) -> io::Result<Self> {
        [Self::Tree, Self::Failed, Self::Progress].into_iter().find(|&t| t as u8 == value).ok_or_else(invalid)
    }
}
pub fn write_tag(output: &mut impl Write, tag: Tag) -> io::Result<()> {
    output.write_all(&[tag as u8])
}
pub fn read_tag(input: &mut impl Read) -> io::Result<Tag> {
    Tag::try_from(byte(input)?)
}

/// GUI → helper commands while the MFT is read.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Pause = b'P',
    Resume = b'R',
}
impl TryFrom<u8> for Command {
    type Error = io::Error;
    fn try_from(value: u8) -> io::Result<Self> {
        [Self::Pause, Self::Resume].into_iter().find(|&c| c as u8 == value).ok_or_else(invalid)
    }
}
pub fn write_command(output: &mut impl Write, command: Command) -> io::Result<()> {
    output.write_all(&[command as u8])
}
pub fn read_command(input: &mut impl Read) -> io::Result<Command> {
    Command::try_from(byte(input)?)
}

fn byte(input: &mut impl Read) -> io::Result<u8> {
    let mut byte = [0];
    input.read_exact(&mut byte)?;
    Ok(byte[0])
}
pub fn number(input: &mut impl Read) -> io::Result<u64> {
    let mut bytes = [0; 8];
    input.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}
pub fn put(output: &mut impl Write, value: u64) -> io::Result<()> {
    output.write_all(&value.to_le_bytes())
}

pub fn string(output: &mut impl Write, value: &OsStr) -> io::Result<()> {
    let bytes = value.encode_wide().flat_map(u16::to_le_bytes).collect::<Vec<_>>();
    let units = bytes.len() as u64 / 2;
    if units > MAX_UNITS {
        return Err(invalid());
    }
    put(output, units)?;
    output.write_all(&bytes)
}
/// Bounded UTF-16 without NULs, possibly unpaired surrogates.
fn read_units(input: &mut impl Read) -> io::Result<Vec<u16>> {
    let length = number(input)?;
    if length > MAX_UNITS {
        return Err(invalid());
    }
    let mut bytes = vec![0; length as usize * 2];
    input.read_exact(&mut bytes)?;
    let units = bytes.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)).collect::<Vec<_>>();
    if units.contains(&0) {
        return Err(invalid());
    }
    Ok(units)
}
pub fn read_string(input: &mut impl Read) -> io::Result<OsString> {
    read_units(input).map(|units| OsString::from_wide(&units))
}
/// A single path component: never empty, `.`, `..`, or containing a separator.
fn read_name(input: &mut impl Read) -> io::Result<OsString> {
    let units = read_units(input)?;
    let reserved = b"/\\:".map(u16::from);
    let dot = u16::from(b'.');
    if units.is_empty() || units.iter().any(|c| reserved.contains(c)) || units == [dot] || units == [dot, dot] {
        return Err(invalid());
    }
    Ok(OsString::from_wide(&units))
}

pub fn write_options(output: &mut impl Write, options: &ScanOptions) -> io::Result<()> {
    let apparent = if options.apparent_size { OPTION_APPARENT_SIZE } else { 0 };
    let dedupe = if options.dedupe_hardlinks { OPTION_DEDUPE_HARDLINKS } else { 0 };
    output.write_all(&[apparent | dedupe])
}
pub fn read_options(input: &mut impl Read) -> io::Result<ScanOptions> {
    let bits = byte(input)?;
    if bits & !(OPTION_APPARENT_SIZE | OPTION_DEDUPE_HARDLINKS) != 0 {
        return Err(invalid());
    }
    Ok(ScanOptions {
        apparent_size: bits & OPTION_APPARENT_SIZE != 0,
        dedupe_hardlinks: bits & OPTION_DEDUPE_HARDLINKS != 0,
        ..ScanOptions::default()
    })
}

pub fn write_error(output: &mut impl Write, error: &io::Error) -> io::Result<()> {
    let code = ERROR_KINDS.iter().position(|&kind| kind == error.kind()).unwrap_or(0);
    output.write_all(&[code as u8])?;
    string(output, OsStr::new(&error.to_string()))
}
pub fn read_error(input: &mut impl Read) -> io::Result<io::Error> {
    let kind = ERROR_KINDS.get(usize::from(byte(input)?)).copied().unwrap_or(io::ErrorKind::Other);
    Ok(io::Error::new(kind, read_string(input)?.to_string_lossy().into_owned()))
}

fn kind_tag(kind: Kind) -> u8 {
    match kind {
        Kind::Dir => 0,
        Kind::File => 1,
        Kind::Symlink => 2,
        Kind::Other => 3,
    }
}
fn read_kind(tag: u8) -> io::Result<Kind> {
    Ok(match tag {
        0 => Kind::Dir,
        1 => Kind::File,
        2 => Kind::Symlink,
        3 => Kind::Other,
        _ => return Err(invalid()),
    })
}

pub fn write_tree(output: &mut impl Write, result: &ScanResult) -> io::Result<()> {
    let tree = &result.tree;
    put(output, tree.len() as u64)?;
    put(output, tree.root().mtime as u64)?;
    let (volume, reference) = tree.root().file_id.ok_or_else(invalid)?;
    put(output, volume)?;
    put(output, reference)?;
    put(output, tree.root().size)?;
    put(output, result.elapsed.as_millis().min(u128::from(u64::MAX)) as u64)?;
    for id in 1..tree.len() as u32 {
        let n = tree.node(id);
        put(output, u64::from(n.parent))?;
        string(output, &n.name)?;
        output.write_all(&[kind_tag(n.kind), n.flags, u8::from(n.file_id.is_some())])?;
        let (volume, reference) = n.file_id.unwrap_or_default();
        for value in [if n.is_dir() { 0 } else { n.size }, n.len, n.mtime as u64, volume, reference] {
            put(output, value)?;
        }
    }
    Ok(())
}
pub fn read_tree(input: &mut impl Read, root: &Path) -> io::Result<ScanResult> {
    let count = number(input)?;
    if count == 0 || count >= u64::from(u32::MAX) {
        return Err(invalid());
    }
    let mut tree = Tree::new(root);
    tree.node_mut(ROOT).mtime = number(input)? as i64;
    tree.node_mut(ROOT).file_id = Some((number(input)?, number(input)?));
    let expected_bytes = number(input)?;
    let elapsed = Duration::from_millis(number(input)?);
    let mut parent = ROOT;
    let mut batch = Vec::with_capacity(256);
    let mut bytes = 0u64;
    let mut dirs = 1;
    for id in 1..count {
        let next_parent = number(input)?;
        if next_parent >= id {
            return Err(invalid());
        }
        if next_parent != u64::from(parent) || batch.len() == 256 {
            tree.add_children(parent, std::mem::take(&mut batch));
            parent = next_parent as u32;
        }
        if !tree.get(parent).is_some_and(clawback_core::tree::Node::is_dir) {
            return Err(invalid());
        }
        let name = read_name(input)?;
        let mut tags = [0; 3];
        input.read_exact(&mut tags)?;
        let [kind, flags, has_id] = tags;
        let kind = read_kind(kind)?;
        if flags & !SCAN_FLAGS != 0 || has_id > 1 {
            return Err(invalid());
        }
        let size = number(input)?;
        if kind == Kind::Dir && size != 0 {
            return Err(invalid());
        }
        bytes = bytes.checked_add(size).ok_or_else(invalid)?;
        dirs += u64::from(kind == Kind::Dir);
        let len = number(input)?;
        let mtime = number(input)? as i64;
        let identity = (number(input)?, number(input)?);
        batch.push(NewEntry { name, kind, size, len, mtime, flags, file_id: (has_id == 1).then_some(identity) });
    }
    tree.add_children(parent, batch);
    if bytes != expected_bytes {
        return Err(invalid());
    }
    tree.sort_all();
    Ok(ScanResult {
        backend: ScanBackend::NtfsMft,
        root: root.to_owned(),
        tree,
        skipped: Vec::new(),
        cancelled: false,
        elapsed,
        files: count - dirs,
        dirs,
        bytes,
        denied: 0,
    })
}

/// Fixed-size telemetry packets: never send per-file messages or partial trees.
pub fn write_progress(output: &mut impl Write, p: MftProgress) -> io::Result<()> {
    for value in [p.phase as u64, p.read, p.total, p.records, p.files, p.dirs, p.bytes] {
        put(output, value)?;
    }
    Ok(())
}

pub fn read_progress(input: &mut impl Read) -> io::Result<MftProgress> {
    let p = MftProgress {
        phase: MftPhase::try_from(number(input)?).map_err(|_| invalid())?,
        read: number(input)?,
        total: number(input)?,
        records: number(input)?,
        files: number(input)?,
        dirs: number(input)?,
        bytes: number(input)?,
    };
    if p.read > p.total || p.files > u64::from(u32::MAX) || p.dirs > u64::from(u32::MAX) {
        return Err(invalid());
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_round_trip_preserves_sizes_hardlinks_and_utf16() {
        let mut tree = Tree::new(Path::new("C:\\"));
        tree.node_mut(ROOT).file_id = Some((7, 5));
        let folder = tree
            .add_children(
                ROOT,
                vec![NewEntry {
                    name: "folder".into(),
                    kind: Kind::Dir,
                    size: 0,
                    len: 0,
                    mtime: 12,
                    flags: 0,
                    file_id: None,
                }],
            )
            .start;
        let name = OsString::from_wide(&[0xd800, 65]);
        tree.add_children(
            folder,
            vec![NewEntry {
                name: name.clone(),
                kind: Kind::File,
                size: 4096,
                len: 3000,
                mtime: 13,
                flags: 0,
                file_id: Some((7, 123)),
            }],
        );
        let result = ScanResult {
            backend: ScanBackend::NtfsMft,
            root: "C:\\".into(),
            tree,
            skipped: vec![],
            cancelled: false,
            elapsed: Duration::from_secs(1),
            files: 1,
            dirs: 2,
            bytes: 4096,
            denied: 0,
        };
        let mut data = Vec::new();
        write_tree(&mut data, &result).expect("encode");
        let decoded = read_tree(&mut data.as_slice(), &result.root).expect("decode");
        assert_eq!(decoded.tree.root().size, 4096);
        assert_eq!(decoded.tree.node(2).name.as_ref(), name);
        assert_eq!(decoded.tree.node(2).file_id, Some((7, 123)));
        assert_eq!((decoded.files, decoded.dirs), (1, 2));
        for end in [0, 8, data.len() - 1] {
            assert!(read_tree(&mut &data[..end], &result.root).is_err());
        }
        data[48..56].copy_from_slice(&9u64.to_le_bytes());
        assert!(read_tree(&mut data.as_slice(), &result.root).is_err());
    }

    #[test]
    fn names_must_be_single_components() {
        for bad in ["", ".", "..", "a/b", "a\\b", "c:"] {
            let mut data = Vec::new();
            string(&mut data, OsStr::new(bad)).expect("encode");
            assert!(read_name(&mut data.as_slice()).is_err(), "{bad:?}");
        }
        let mut data = Vec::new();
        string(&mut data, OsStr::new("...")).expect("encode");
        assert_eq!(read_name(&mut data.as_slice()).expect("decode"), "...");
    }

    #[test]
    fn small_messages_keep_their_wire_bytes() {
        let mut data = Vec::new();
        for tag in [Tag::Tree, Tag::Failed, Tag::Progress] {
            write_tag(&mut data, tag).expect("tag");
        }
        write_command(&mut data, Command::Pause).expect("pause");
        write_command(&mut data, Command::Resume).expect("resume");
        let options = ScanOptions { apparent_size: true, dedupe_hardlinks: true, ..ScanOptions::default() };
        write_options(&mut data, &options).expect("options");
        assert_eq!(data, [1, 2, 3, b'P', b'R', 3]);
        let decoded = read_options(&mut &[2u8][..]).expect("options");
        assert!(!decoded.apparent_size && decoded.dedupe_hardlinks);
        assert!(read_options(&mut &[4u8][..]).is_err());
        assert!(read_tag(&mut &[0u8][..]).is_err());
        assert!(read_command(&mut &b"X"[..]).is_err());

        let mut data = Vec::new();
        write_error(&mut data, &io::Error::new(io::ErrorKind::PermissionDenied, "no")).expect("error");
        assert_eq!(data[0], 2);
        let error = read_error(&mut data.as_slice()).expect("decode");
        assert_eq!((error.kind(), error.to_string()), (io::ErrorKind::PermissionDenied, "no".to_owned()));
        let mut data = Vec::new();
        write_error(&mut data, &io::Error::new(io::ErrorKind::TimedOut, "slow")).expect("error");
        assert_eq!(read_error(&mut data.as_slice()).expect("decode").kind(), io::ErrorKind::Other);
    }

    #[test]
    fn telemetry_roundtrip_rejects_truncation_and_invalid_values() {
        let p = MftProgress {
            phase: MftPhase::Assembling,
            read: 1024,
            total: 1024,
            records: 40,
            files: 30,
            dirs: 5,
            bytes: 8192,
        };
        let mut packet = Vec::new();
        write_progress(&mut packet, p).expect("write");
        assert_eq!(packet[..8], 2u64.to_le_bytes(), "phases keep their wire numbers");
        assert_eq!(read_progress(&mut packet.as_slice()).expect("read"), p);
        for len in 0..packet.len() {
            assert!(read_progress(&mut &packet[..len]).is_err());
        }
        for (number, phase) in
            [MftPhase::Reading, MftPhase::Resolving, MftPhase::Assembling, MftPhase::Sorting, MftPhase::Transferring]
                .into_iter()
                .enumerate()
        {
            assert_eq!(MftPhase::try_from(number as u64), Ok(phase));
        }
        let mut bad_phase = packet.clone();
        bad_phase[..8].copy_from_slice(&5u64.to_le_bytes());
        assert!(read_progress(&mut bad_phase.as_slice()).is_err());
        let mut bad_read = Vec::new();
        write_progress(&mut bad_read, MftProgress { read: 1025, ..p }).expect("write");
        assert!(read_progress(&mut bad_read.as_slice()).is_err());
    }
}
