//! Versioned, bounded, buffered snapshot stream. No serde, temporary files,
//! per-file IPC messages, or per-file metadata queries.
use clawback_core::{
    ROOT, Tree,
    scan::{ScanBackend, ScanResult},
    tree::{Kind, NewEntry},
};
use std::{
    ffi::{OsStr, OsString},
    io::{self, Read, Write},
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::Path,
    time::Duration,
};

pub const MAGIC: &[u8; 8] = b"CLAWMFT2";
pub fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid Turbo protocol")
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
    let units = value.encode_wide().collect::<Vec<_>>();
    if units.len() > 32767 {
        return Err(invalid());
    }
    put(output, units.len() as u64)?;
    for unit in units {
        output.write_all(&unit.to_le_bytes())?;
    }
    Ok(())
}
pub fn read_string(input: &mut impl Read) -> io::Result<OsString> {
    let length = number(input)?;
    if length > 32767 {
        return Err(invalid());
    }
    let mut bytes = vec![0; length as usize * 2];
    input.read_exact(&mut bytes)?;
    let units = bytes.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes([b[0], b[1]])).collect::<Vec<_>>();
    if units.contains(&0) {
        return Err(invalid());
    }
    Ok(OsString::from_wide(&units))
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
        output.write_all(&[
            match n.kind {
                Kind::Dir => 0,
                Kind::File => 1,
                Kind::Symlink => 2,
                Kind::Other => 3,
            },
            n.flags,
            u8::from(n.file_id.is_some()),
        ])?;
        for value in [
            if n.is_dir() { 0 } else { n.size },
            n.len,
            n.mtime as u64,
            n.file_id.unwrap_or_default().0,
            n.file_id.unwrap_or_default().1,
        ] {
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
        let name = read_string(input)?;
        let units = name.encode_wide().collect::<Vec<_>>();
        if units.is_empty() || units.iter().any(|c| [47, 92, 58].contains(c)) || name == "." || name == ".." {
            return Err(invalid());
        }
        let mut tags = [0; 3];
        input.read_exact(&mut tags)?;
        let kind = match tags[0] {
            0 => Kind::Dir,
            1 => Kind::File,
            2 => Kind::Symlink,
            3 => Kind::Other,
            _ => return Err(invalid()),
        };
        if tags[1] & !31 != 0 || tags[2] > 1 {
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
        batch.push(NewEntry {
            name,
            kind,
            size,
            len,
            mtime,
            flags: tags[1],
            file_id: (tags[2] == 1).then_some(identity),
        });
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
}

/// Fixed-size telemetry packets: never send per-file messages or partial trees.
pub fn write_progress(output: &mut impl Write, p: clawback_core::scan::MftProgress) -> io::Result<()> {
    for value in [p.phase, p.read, p.total, p.records, p.files, p.dirs, p.bytes] {
        put(output, value)?;
    }
    Ok(())
}

pub fn read_progress(input: &mut impl Read) -> io::Result<clawback_core::scan::MftProgress> {
    let p = clawback_core::scan::MftProgress {
        phase: number(input)?,
        read: number(input)?,
        total: number(input)?,
        records: number(input)?,
        files: number(input)?,
        dirs: number(input)?,
        bytes: number(input)?,
    };
    if p.phase > 4 || p.read > p.total || p.files > u64::from(u32::MAX) || p.dirs > u64::from(u32::MAX) {
        return Err(invalid());
    }
    Ok(p)
}

#[cfg(test)]
mod progress_tests {
    use super::*;
    #[test]
    fn telemetry_roundtrip_rejects_truncation_and_invalid_values() {
        let p = clawback_core::scan::MftProgress {
            phase: 2,
            read: 1024,
            total: 1024,
            records: 40,
            files: 30,
            dirs: 5,
            bytes: 8192,
        };
        let mut packet = Vec::new();
        write_progress(&mut packet, p).expect("write");
        assert_eq!(read_progress(&mut packet.as_slice()).expect("read"), p);
        for len in 0..packet.len() {
            assert!(read_progress(&mut &packet[..len]).is_err());
        }
        for bad in
            [clawback_core::scan::MftProgress { phase: 5, ..p }, clawback_core::scan::MftProgress { read: 1025, ..p }]
        {
            let mut packet = Vec::new();
            write_progress(&mut packet, bad).expect("write");
            assert!(read_progress(&mut packet.as_slice()).is_err());
        }
    }
}
