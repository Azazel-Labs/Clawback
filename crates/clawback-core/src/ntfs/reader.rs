//! Whole-volume MFT ingestion. Bootstrap $MFT's extents with documented NTFS
//! control codes, then read records in 1 mebibyte batches, not one ioctl per file.
use super::{
    INDEX_MASK, Record, Run, attributes, bytes, fixup, invalid, parse_with_reparse, runs, u16_at, u32_at, u64_at,
};
use crate::scan::{Shared, lock};
use crate::tree::{Kind, NewEntry, ROOT, Tree, flags};
use crate::windows;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::os::windows::ffi::OsStringExt;
use std::sync::atomic::Ordering;

const GET_VOLUME: u32 = 0x0009_0064;
const GET_RECORD: u32 = 0x0009_0068;
const BATCH: usize = 1024 * 1024;
const IO_ALIGNMENT: usize = 65536;

fn context(error: &io::Error, phase: impl std::fmt::Display) -> io::Error {
    io::Error::new(error.kind(), format!("{phase}: {error}"))
}

struct Volume {
    file: File,
    cluster: u64,
    sector: usize,
    clusters: u64,
    record_size: usize,
    valid_len: u64,
    read_buffer: Vec<u8>,
}

impl Volume {
    fn reparse_tag(&mut self, attribute: &[u8]) -> io::Result<u32> {
        // Only the tag is needed. Read one aligned sector, never allocate the
        // whole payload or follow the reparse target. Reject compressed/sparse
        // metadata rather than interpreting encoded bytes as a tag.
        if u64_at(attribute, 48)? < 8 || u64_at(attribute, 56)? < 8 || u16_at(attribute, 12)? != 0 {
            return Err(invalid());
        }
        let mut mapping = runs(attribute)?;
        self.validate_runs(&mut mapping, self.sector as u64)?;
        let mut data = vec![0; self.sector];
        self.read_stream(&mapping, 0, &mut data)?;
        u32_at(&data, 0)
    }

    fn open(file: File) -> io::Result<Self> {
        let mut data = [0u8; 128];
        let len = windows::control(&file, GET_VOLUME, &[], &mut data)?;
        if len < 104 || u16_at(&data, 100)? != 3 || u16_at(&data, 102)? > 1 {
            return Err(invalid());
        }
        let sector = u64::from(u32_at(&data, 40)?);
        let cluster = u64::from(u32_at(&data, 44)?);
        let record_size = u32_at(&data, 48)? as usize;
        let valid_len = u64_at(&data, 56)?;
        if !sector.is_power_of_two()
            || !(512..=65536).contains(&sector)
            || !cluster.is_power_of_two()
            || !(sector..=2 * 1024 * 1024).contains(&cluster)
            || !record_size.is_power_of_two()
            || !(512..=65536).contains(&record_size)
            || !valid_len.is_multiple_of(record_size as u64)
            || valid_len == 0
        {
            return Err(invalid());
        }
        Ok(Self {
            file,
            cluster,
            sector: sector as usize,
            clusters: u64_at(&data, 16)?,
            record_size,
            valid_len,
            read_buffer: vec![0; BATCH + IO_ALIGNMENT],
        })
    }

    fn record(file: &File, record_size: usize, reference: u64) -> io::Result<Vec<u8>> {
        let mut data = vec![0u8; record_size + 16];
        let index = reference & INDEX_MASK;
        let len = windows::control(file, GET_RECORD, &index.to_le_bytes(), &mut data)?;
        if len < 12 || u64_at(&data, 0)? & INDEX_MASK != index || u32_at(&data, 8)? as usize != record_size {
            return Err(invalid());
        }
        let record = bytes(&data[..len], 12, record_size)?.to_vec();
        if reference >> 48 != 0 && u64::from(u16_at(&record, 16)?) != reference >> 48 {
            return Err(invalid());
        }
        Ok(record)
    }

    fn validate_runs(&self, extents: &mut [Run], length: u64) -> io::Result<()> {
        extents.sort_unstable_by_key(|r| r.vcn);
        let mut next = 0;
        for run in extents {
            let lcn = run.lcn.ok_or_else(invalid)?;
            if run.vcn != next || lcn.checked_add(run.clusters).ok_or_else(invalid)? > self.clusters {
                return Err(invalid());
            }
            next = next.checked_add(run.clusters).ok_or_else(invalid)?;
        }
        if next.checked_mul(self.cluster).ok_or_else(invalid)? < length {
            return Err(invalid());
        }
        Ok(())
    }

    fn read_stream(&mut self, extents: &[Run], offset: u64, output: &mut [u8]) -> io::Result<()> {
        if !offset.is_multiple_of(self.sector as u64) || !output.len().is_multiple_of(self.sector) {
            return Err(invalid());
        }
        // DASD handles obey noncached I/O restrictions even without an explicit
        // flag. Align every physical read, including reads across fragmented
        // extents; the caller's destination need not itself be aligned.
        // https://learn.microsoft.com/windows/win32/api/fileapi/nf-fileapi-createfilew
        let aligned = self.read_buffer.as_ptr().align_offset(IO_ALIGNMENT);
        let mut offset = offset;
        let mut remaining = output;
        while !remaining.is_empty() {
            let vcn = offset / self.cluster;
            let index = extents.partition_point(|r| r.vcn <= vcn).checked_sub(1).ok_or_else(invalid)?;
            let run = &extents[index];
            let within =
                offset.checked_sub(run.vcn.checked_mul(self.cluster).ok_or_else(invalid)?).ok_or_else(invalid)?;
            let available =
                run.clusters.checked_mul(self.cluster).and_then(|n| n.checked_sub(within)).ok_or_else(invalid)?;
            let count = available.min(remaining.len().min(BATCH) as u64) as usize;
            if count == 0 {
                return Err(invalid());
            }
            let position = run
                .lcn
                .ok_or_else(invalid)?
                .checked_mul(self.cluster)
                .and_then(|n| n.checked_add(within))
                .ok_or_else(invalid)?;
            self.file.seek(SeekFrom::Start(position))?;
            let chunk = &mut self.read_buffer[aligned..aligned + count];
            self.file.read_exact(chunk)?;
            remaining[..count].copy_from_slice(chunk);
            remaining = &mut remaining[count..];
            offset = offset.checked_add(count as u64).ok_or_else(invalid)?;
        }
        Ok(())
    }

    fn mft_runs(&mut self, shared: &Shared) -> io::Result<Vec<Run>> {
        let file = self.file.try_clone()?;
        let record_size = self.record_size;
        self.mft_runs_with(shared, |reference| Self::record(&file, record_size, reference))
    }

    fn mft_runs_with(
        &mut self,
        shared: &Shared,
        mut read_record: impl FnMut(u64) -> io::Result<Vec<u8>>,
    ) -> io::Result<Vec<Run>> {
        let first = read_record(0)?;
        let mut extents = Vec::new();
        let mut references = HashSet::new();
        for attr in attributes(&first)? {
            if attr.kind == 0x80 && attr.unnamed() {
                extents.extend(runs(attr.data)?);
            }
            if attr.kind != 0x20 {
                continue;
            }
            let list = if attr.data[8] == 0 {
                attr.resident()?.to_vec()
            } else {
                let len = u64_at(attr.data, 48)?;
                // Attribute lists are metadata, never allow an on-disk length
                // to allocate an unbounded buffer. Larger lists use fallback.
                if len > 16 * 1024 * 1024 {
                    return Err(invalid());
                }
                let mut mapping = runs(attr.data)?;
                self.validate_runs(&mut mapping, len)?;
                let mut list = vec![0u8; (len.div_ceil(self.cluster) * self.cluster) as usize];
                self.read_stream(&mapping, 0, &mut list)?;
                list.truncate(len as usize);
                list
            };
            let mut offset = 0;
            while offset < list.len() {
                checkpoint(shared)?;
                let entry = bytes(&list, offset, 26)?;
                let len = usize::from(u16_at(entry, 4)?);
                if len < 26 {
                    return Err(invalid());
                }
                bytes(&list, offset, len)?;
                if u32_at(entry, 0)? == 0x80 && entry[6] == 0 {
                    let reference = u64_at(entry, 16)?;
                    if reference & INDEX_MASK != 0 {
                        references.insert(reference);
                    }
                }
                offset += len;
            }
        }
        for reference in references {
            checkpoint(shared)?;
            let record = read_record(reference)?;
            if u64_at(&record, 32)? != u64::from(u16_at(&first, 16)?) << 48 {
                return Err(invalid());
            }
            for attr in attributes(&record)? {
                if attr.kind == 0x80 && attr.unnamed() {
                    extents.extend(runs(attr.data)?);
                }
            }
        }
        self.validate_runs(&mut extents, self.valid_len)?;
        Ok(extents)
    }
}

fn checkpoint(shared: &Shared) -> io::Result<()> {
    shared.progress.wait_if_paused();
    if shared.progress.cancel.load(Ordering::Relaxed) {
        Err(io::Error::new(io::ErrorKind::Interrupted, "Scan cancelled"))
    } else {
        Ok(())
    }
}

pub(crate) fn scan(shared: &Shared) -> io::Result<bool> {
    #[cfg(feature = "profiling")]
    let volume_phase = shared.profile.timer(crate::profiling::Phase::Volume);
    let Some((file, serial)) = windows::volume(&shared.root)? else { return Ok(false) };
    let mut volume = Volume::open(file).map_err(|e| context(&e, "Reading NTFS volume information"))?;
    #[cfg(feature = "profiling")]
    drop(volume_phase);
    checkpoint(shared)?;
    shared.progress.workers.store(1, Ordering::Relaxed);
    let mapping = {
        #[cfg(feature = "profiling")]
        let _phase = shared.profile.timer(crate::profiling::Phase::Bootstrap);
        volume.mft_runs(shared).map_err(|e| context(&e, "Locating the MFT data extents"))?
    };
    ingest(shared, serial, &mut volume, &mapping)?;
    Ok(true)
}

fn ingest(shared: &Shared, serial: u64, volume: &mut Volume, mapping: &[Run]) -> io::Result<()> {
    let mut buffer = vec![0u8; BATCH];
    let mut records = HashMap::new();
    let mut extensions = Vec::new();
    let mut offset = 0;
    lock(&shared.mft_progress).total = volume.valid_len;
    while offset < volume.valid_len {
        checkpoint(shared)?;
        let count = (volume.valid_len - offset).min(BATCH as u64) as usize;
        {
            #[cfg(feature = "profiling")]
            let _phase = shared.profile.timer(crate::profiling::Phase::Read);
            volume.read_stream(mapping, offset, &mut buffer[..count.next_multiple_of(volume.sector)])?;
        }
        #[cfg(feature = "profiling")]
        let _phase = shared.profile.timer(crate::profiling::Phase::Parse);
        for (i, record) in buffer[..count].chunks_exact_mut(volume.record_size).enumerate() {
            let index = offset / volume.record_size as u64 + i as u64;
            // Unused slots can be zero-filled or contain old FILE records.
            if record.iter().all(|&b| b == 0) {
                continue;
            }
            if bytes(record, 0, 4)? != b"FILE" {
                return Err(invalid());
            }
            if u16_at(record, 22)? & 1 == 0 {
                continue;
            }
            fixup(record).map_err(|e| context(&e, format_args!("Validating MFT record {index}")))?;
            if let Some(record) = parse_with_reparse(record, index, |attr| volume.reparse_tag(attr))
                .map_err(|e| context(&e, format_args!("Parsing MFT record {index}")))?
            {
                if record.size.is_some_and(|(allocated, _)| allocated > volume.clusters.saturating_mul(volume.cluster))
                {
                    return Err(invalid());
                }
                if record.base == 0 {
                    records.insert(record.id, record);
                } else {
                    extensions.push(record);
                }
            }
        }
        offset += count as u64;
        let mut progress = lock(&shared.mft_progress);
        progress.read = offset;
        progress.records = records.len() as u64;
    }
    lock(&shared.mft_progress).phase = 1;
    #[cfg(feature = "profiling")]
    let merge_phase = shared.profile.timer(crate::profiling::Phase::Merge);
    for extension in extensions {
        checkpoint(shared)?;
        let base = records.get_mut(&extension.base).ok_or_else(invalid)?;
        base.names.extend(extension.names);
        base.surrogate |= extension.surrogate;
        if let Some(size) = extension.size
            && base.size.replace(size).is_some()
        {
            return Err(invalid());
        }
    }
    #[cfg(feature = "profiling")]
    drop(merge_phase);
    let tree = build_tree(shared, serial, records).map_err(|e| context(&e, "Assembling the MFT directory tree"))?;
    #[cfg(feature = "profiling")]
    let _phase = shared.profile.timer(crate::profiling::Phase::Publish);
    checkpoint(shared)?;
    shared.progress.files.store(tree.root().files, Ordering::Relaxed);
    shared.progress.bytes.store(tree.root().size, Ordering::Relaxed);
    shared.progress.dirs.store(tree.dir_count(ROOT), Ordering::Relaxed);
    *lock(&shared.tree) = tree;
    Ok(())
}

fn build_tree(shared: &Shared, serial: u64, mut records: HashMap<u64, Record>) -> io::Result<Tree> {
    #[cfg(feature = "profiling")]
    let index_phase = shared.profile.timer(crate::profiling::Phase::Index);
    let root = records.keys().copied().find(|id| id & INDEX_MASK == 5).ok_or_else(invalid)?;
    if !records[&root].directory {
        return Err(invalid());
    }
    let mut tree = Tree::new(&shared.root);
    tree.node_mut(ROOT).mtime = records[&root].mtime;
    // Also identifies MFT accounting to incremental reconciliation. Directory
    // traversal deliberately uses listing-only estimates on Windows.
    tree.node_mut(ROOT).file_id = Some((serial, root));
    let mut children: HashMap<u64, Vec<(u64, std::ffi::OsString)>> = HashMap::new();
    for record in records.values_mut() {
        checkpoint(shared)?;
        if record.id == root {
            continue;
        }
        let mut names = HashSet::new();
        for name in record.names.drain(..) {
            if name.text == [46] {
                return Err(invalid());
            }
            let text = std::ffi::OsString::from_wide(&name.text);
            if names.insert((name.parent, text.clone())) {
                children.entry(name.parent).or_default().push((record.id, text));
            }
        }
    }
    // Keep the same user-file scope as directory traversal. Reserved NTFS
    // records ($MFT, $Bitmap, $Extend, etc.) and their descendants account for
    // filesystem overhead, not ordinary directory contents. Including them
    // would also make live directory reconciliation remove invisible entries.
    let mut excluded: Vec<_> = records.keys().copied().filter(|id| id & INDEX_MASK < 16 && *id != root).collect();
    let mut excluded_seen = HashSet::new();
    while let Some(id) = excluded.pop() {
        checkpoint(shared)?;
        if excluded_seen.insert(id)
            && let Some(entries) = children.remove(&id)
        {
            excluded.extend(entries.into_iter().map(|(id, _)| id));
        }
    }
    for entries in children.values_mut() {
        entries.retain(|(id, _)| !excluded_seen.contains(id));
    }
    #[cfg(feature = "profiling")]
    drop(index_phase);
    #[cfg(feature = "profiling")]
    let assemble_phase = shared.profile.timer(crate::profiling::Phase::Assemble);
    let mut pending = vec![(root, ROOT)];
    let mut visited_dirs = HashSet::from([root]);
    let mut counted = HashSet::new();
    let mut total_bytes = 0u64;
    lock(&shared.mft_progress).phase = 2;
    while let Some((reference, node)) = pending.pop() {
        checkpoint(shared)?;
        let Some(entries) = children.remove(&reference) else { continue };
        let mut batch = Vec::with_capacity(256);
        let mut dirs = Vec::new();
        for (reference, name) in entries {
            checkpoint(shared)?;
            let record = &records[&reference];
            let kind = if record.surrogate {
                Kind::Symlink
            } else if record.directory {
                Kind::Dir
            } else {
                Kind::File
            };
            if kind == Kind::Dir {
                if !visited_dirs.insert(reference) {
                    return Err(invalid());
                }
                dirs.push((reference, batch.len()));
            }
            let (allocated, len) = record.size.unwrap_or((0, 0));
            let duplicate = kind != Kind::Dir && shared.options.dedupe_hardlinks && !counted.insert(reference);
            let size = if duplicate || kind == Kind::Dir {
                0
            } else if shared.options.apparent_size {
                len
            } else {
                allocated
            };
            total_bytes = total_bytes.checked_add(size).ok_or_else(invalid)?;
            batch.push(NewEntry {
                name,
                kind,
                size,
                len,
                mtime: record.mtime,
                flags: if duplicate { flags::HARDLINK_DUP } else { 0 },
                file_id: (kind != Kind::Dir).then_some((serial, reference)),
            });
            if batch.len() == 256 {
                let range = tree.add_children(node, std::mem::take(&mut batch));
                pending.extend(dirs.drain(..).map(|(id, i)| (id, range.start + i as u32)));
                report_assembled(shared, &tree);
            }
        }
        let range = tree.add_children(node, batch);
        pending.extend(dirs.into_iter().map(|(id, i)| (id, range.start + i as u32)));
        report_assembled(shared, &tree);
    }
    // A missing parent or a cycle means the live MFT was not coherent. Children
    // of name-surrogate directories are intentionally not traversed.
    if children.keys().any(|id| !records.get(id).is_some_and(|r| r.surrogate)) {
        return Err(invalid());
    }
    #[cfg(feature = "profiling")]
    drop(assemble_phase);
    #[cfg(feature = "profiling")]
    let _phase = shared.profile.timer(crate::profiling::Phase::Sort);
    lock(&shared.mft_progress).phase = 3;
    tree.sort_all();
    Ok(tree)
}

fn report_assembled(shared: &Shared, tree: &Tree) {
    let mut progress = lock(&shared.mft_progress);
    progress.files = tree.root().files;
    progress.dirs = tree.len() as u64 - progress.files;
    progress.bytes = tree.root().size;
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::ScanOptions;
    use crate::ntfs::parse;
    use crate::ntfs::tests::{encode_fixups, filename, nonresident, record, reference, resident};
    use std::path::PathBuf;

    fn state() -> Shared {
        Shared::new(PathBuf::from(r"C:\fixture"), ScanOptions::default())
    }

    fn fixture() -> Vec<u8> {
        let mut image = vec![0; 36 * 1024];
        let entries = [
            (5, record(&[filename(reference(5), ".", 3)], true, 0)),
            (20, record(&[filename(reference(5), "folder", 1)], true, 0)),
            (21, record(&[filename(reference(20), "original", 1), nonresident(0, 8192, 5000, 8192)], false, 0)),
            (22, record(&[filename(reference(5), "alias", 1)], false, reference(21))),
            (23, record(&[filename(reference(5), "tiny", 1), resident(0x80, b"abc")], false, 0)),
        ];
        for (index, mut data) in entries {
            encode_fixups(&mut data);
            image[index * 1024..(index + 1) * 1024].copy_from_slice(&data);
        }
        image
    }

    struct Image(PathBuf);
    impl Drop for Image {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn disk() -> (Image, Volume, Vec<Run>) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "clawback-mft-{}-{}.img",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let image = fixture();
        let mut disk = vec![0; 100 * 512];
        disk[4 * 512..36 * 512].copy_from_slice(&image[..32 * 512]);
        disk[50 * 512..90 * 512].copy_from_slice(&image[32 * 512..]);
        std::fs::write(&path, disk).unwrap();
        let volume = Volume {
            file: File::open(&path).unwrap(),
            cluster: 512,
            sector: 512,
            clusters: 100,
            record_size: 1024,
            valid_len: image.len() as u64,
            read_buffer: vec![0; BATCH + IO_ALIGNMENT],
        };
        (
            Image(path),
            volume,
            vec![Run { vcn: 0, clusters: 32, lcn: Some(4) }, Run { vcn: 32, clusters: 40, lcn: Some(50) }],
        )
    }

    #[test]
    fn nonresident_reparse_tags_in_extension_records_preserve_link_policy() {
        for (tag, surrogate) in [(0x9000_001au32, false), (0xa000_0003, true), (0xa000_000c, true)] {
            let (image, mut volume, mapping) = disk();
            let mut attr = nonresident(0, 512, 8, 512);
            attr[..4].copy_from_slice(&0xc0u32.to_le_bytes());
            attr[74] = 95; // sector holding the nonresident reparse payload
            let mut extension = record(&[attr], true, reference(20));
            encode_fixups(&mut extension);
            let mut data = std::fs::read(&image.0).unwrap();
            let offset = 50 * 512 + (24 * 1024 - 32 * 512);
            data[offset..offset + 1024].copy_from_slice(&extension);
            data[95 * 512..95 * 512 + 4].copy_from_slice(&tag.to_le_bytes());
            std::fs::write(&image.0, data).unwrap();
            let shared = state();
            ingest(&shared, 123, &mut volume, &mapping).unwrap();
            let tree = lock(&shared.tree);
            let folder = tree.find_path(&shared.root.join("folder")).unwrap();
            assert_eq!(tree.node(folder).kind, if surrogate { Kind::Symlink } else { Kind::Dir });
            assert_eq!(tree.find_path(&shared.root.join("folder/original")).is_none(), surrogate);
            assert_eq!(tree.root().size, 8192);
        }
    }

    #[test]
    fn nonresident_reparse_metadata_requires_valid_initialized_storage() {
        let (_image, mut volume, _) = disk();
        for (flags, len, initialized, lcn) in [(1, 8, 8u64, 95), (0, 3, 3, 95), (0, 8, 0, 95), (0, 8, 8, 100)] {
            let mut attr = nonresident(flags, 512, len, 512);
            attr[56..64].copy_from_slice(&initialized.to_le_bytes());
            attr[74] = lcn;
            assert!(volume.reparse_tag(&attr).is_err());
        }
    }

    #[test]
    fn fragmented_mft_reads_merge_extensions_and_dedupe_hardlinks() {
        let (_image, mut volume, mut mapping) = disk();
        volume.validate_runs(&mut mapping, volume.valid_len).unwrap();
        let shared = state();
        ingest(&shared, 123, &mut volume, &mapping).unwrap();
        let tree = lock(&shared.tree);
        assert_eq!(tree.root().size, 8192);
        assert_eq!(tree.root().files, 3);
        let telemetry = *lock(&shared.mft_progress);
        assert_eq!(telemetry.phase, 3);
        assert_eq!(telemetry.read, telemetry.total);
        assert_eq!((telemetry.files, telemetry.dirs, telemetry.bytes), (3, 2, 8192));
        assert_eq!(tree.dir_count(ROOT), 2);
        let original = tree.find_path(&shared.root.join("folder/original")).unwrap();
        let alias = tree.find_path(&shared.root.join("alias")).unwrap();
        assert_eq!(tree.node(original).file_id, Some((123, reference(21))));
        assert_eq!(tree.node(alias).file_id, tree.node(original).file_id);
        assert_eq!(tree.node(original).size + tree.node(alias).size, 8192);
        drop(tree);
        let mut apparent = state();
        apparent.options.apparent_size = true;
        apparent.options.dedupe_hardlinks = false;
        ingest(&apparent, 123, &mut volume, &mapping).unwrap();
        assert_eq!(lock(&apparent.tree).root().size, 10003);
    }

    #[test]
    fn paused_ingestion_resumes_and_publishes_the_complete_tree() {
        let (_image, mut volume, mapping) = disk();
        let shared = state();
        shared.progress.set_paused(true);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                tx.send(ingest(&shared, 123, &mut volume, &mapping)).unwrap();
            });
            let blocked = matches!(
                rx.recv_timeout(std::time::Duration::from_millis(100)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            );
            let before = shared.progress.snapshot();
            shared.progress.set_paused(false);
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap().unwrap();
            assert!(blocked);
            assert_eq!(before.files, 0);
        });
        assert_eq!(lock(&shared.tree).root().files, 3);
        assert_eq!(lock(&shared.tree).root().size, 8192);
    }

    #[test]
    fn cancelled_or_invalid_ingestion_never_publishes_an_unvalidated_tree() {
        let (_image, mut volume, mapping) = disk();
        let shared = state();
        shared.progress.cancel.store(true, Ordering::Relaxed);
        assert_eq!(ingest(&shared, 123, &mut volume, &mapping).unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert_eq!(lock(&shared.tree).root().files, 0);
        shared.progress.cancel.store(false, Ordering::Relaxed);
        volume.valid_len += 1024 * 100;
        assert!(ingest(&shared, 123, &mut volume, &mapping).is_err());
        assert_eq!(lock(&shared.tree).root().files, 0);
    }

    #[test]
    fn stale_parent_references_and_cycles_are_rejected() {
        let shared = state();
        let root = parse(&record(&[], true, 0), 5).unwrap().unwrap();
        let orphan = parse(&record(&[filename(reference(5) + (1 << 48), "stale", 1)], false, 0), 20).unwrap().unwrap();
        assert!(build_tree(&shared, 1, HashMap::from([(root.id, root), (orphan.id, orphan)])).is_err());
        let root = parse(&record(&[], true, 0), 5).unwrap().unwrap();
        let cycle = parse(&record(&[filename(reference(20), "cycle", 1)], true, 0), 20).unwrap().unwrap();
        assert!(build_tree(&shared, 1, HashMap::from([(root.id, root), (cycle.id, cycle)])).is_err());
    }

    #[test]
    fn extent_validation_rejects_holes_overlaps_and_out_of_volume_reads() {
        let (_image, volume, _) = disk();
        for mut mapping in [
            vec![Run { vcn: 1, clusters: 72, lcn: Some(1) }],
            vec![Run { vcn: 0, clusters: 72, lcn: None }],
            vec![Run { vcn: 0, clusters: 72, lcn: Some(99) }],
            vec![Run { vcn: 0, clusters: 40, lcn: Some(1) }, Run { vcn: 32, clusters: 40, lcn: Some(50) }],
        ] {
            assert!(volume.validate_runs(&mut mapping, volume.valid_len).is_err());
        }
    }

    #[test]
    fn bootstrap_resolves_mft_attribute_lists_and_rejects_wrong_base_records() {
        let (_image, mut volume, expected) = disk();
        let shared = state();
        let mut first_data = nonresident(0, 72 * 512, 72 * 512, 72 * 512);
        first_data[24..32].copy_from_slice(&31u64.to_le_bytes());
        first_data[72..76].copy_from_slice(&[0x11, 32, 4, 0]);
        let mut next_data = nonresident(0, 0, 0, 0);
        next_data[16..24].copy_from_slice(&32u64.to_le_bytes());
        next_data[24..32].copy_from_slice(&71u64.to_le_bytes());
        next_data[72..76].copy_from_slice(&[0x11, 40, 50, 0]);
        let mut list = vec![0; 32];
        list[..4].copy_from_slice(&0x80u32.to_le_bytes());
        list[4..6].copy_from_slice(&32u16.to_le_bytes());
        list[8..16].copy_from_slice(&32u64.to_le_bytes());
        list[16..24].copy_from_slice(&reference(22).to_le_bytes());
        let first = record(&[first_data, resident(0x20, &list)], false, 0);
        let extension = record(&[next_data], false, reference(0));
        let mapping = volume
            .mft_runs_with(&shared, |id| match id {
                0 => Ok(first.clone()),
                id if id == reference(22) => Ok(extension.clone()),
                _ => Err(invalid()),
            })
            .unwrap();
        assert_eq!(mapping, expected);
        let mut wrong = extension;
        wrong[32..40].copy_from_slice(&reference(99).to_le_bytes());
        assert!(volume.mft_runs_with(&shared, |id| Ok(if id == 0 { first.clone() } else { wrong.clone() })).is_err());
    }

    #[test]
    fn reserved_metadata_subtrees_are_excluded_from_user_file_accounting() {
        let shared = state();
        let root = parse(&record(&[], true, 0), 5).unwrap().unwrap();
        let extend = parse(&record(&[filename(reference(5), "$Extend", 1)], true, 0), 11).unwrap().unwrap();
        let metadata = parse(
            &record(&[filename(reference(11), "$Internal", 1), nonresident(0, 65536, 65536, 65536)], false, 0),
            24,
        )
        .unwrap()
        .unwrap();
        let ordinary =
            parse(&record(&[filename(reference(5), "$ordinary", 1), nonresident(0, 4096, 2048, 4096)], false, 0), 25)
                .unwrap()
                .unwrap();
        let records = [root, extend, metadata, ordinary].into_iter().map(|record| (record.id, record)).collect();
        let tree = build_tree(&shared, 1, records).unwrap();
        assert_eq!(tree.root().size, 4096);
        assert_eq!(tree.root().files, 1);
        assert!(tree.find_path(&shared.root.join("$ordinary")).is_some());
    }

    #[cfg(feature = "profiling")]
    #[test]
    #[ignore = "Dedicated optimized synthetic MFT profiling; no raw-volume privileges required"]
    fn profile_synthetic_mft() {
        use std::io::Write;
        use std::time::Instant;
        let files: u64 = std::env::var("CLAWBACK_PROFILE_FILES").map_or(200_000, |s| s.parse().unwrap());
        let dirs = 1000;
        let slots = files + dirs + 16;
        let len = slots * 1024;
        let image = Image(std::env::temp_dir().join(format!("clawback-profile-{}.img", std::process::id())));
        {
            let mut output = io::BufWriter::new(File::create(&image.0).unwrap());
            for index in 0..slots {
                let mut data = if index == 5 {
                    record(&[filename(reference(5), ".", 3)], true, 0)
                } else if index < 16 {
                    vec![0; 1024]
                } else if index < 16 + dirs {
                    record(&[filename(reference(5), &format!("dir-{index}"), 1)], true, 0)
                } else {
                    record(
                        &[
                            filename(reference(16 + index % dirs), &format!("file-{index}.bin"), 1),
                            nonresident(0, 4096, 2048, 4096),
                        ],
                        false,
                        0,
                    )
                };
                if index >= 16 || index == 5 {
                    encode_fixups(&mut data);
                }
                output.write_all(&data).unwrap();
            }
            output.flush().unwrap();
        }
        println!("{}", crate::profiling::header());
        for _ in 0..3 {
            let shared = state();
            let mut volume = Volume {
                file: File::open(&image.0).unwrap(),
                cluster: 512,
                sector: 512,
                clusters: len / 512,
                record_size: 1024,
                valid_len: len,
                read_buffer: vec![0; BATCH + IO_ALIGNMENT],
            };
            let mapping = [Run { vcn: 0, lcn: Some(0), clusters: len / 512 }];
            let start = Instant::now();
            ingest(&shared, 1, &mut volume, &mapping).unwrap();
            let elapsed = start.elapsed().as_secs_f64();
            assert_eq!(shared.progress.files.load(Ordering::Relaxed), files);
            assert_eq!(shared.progress.bytes.load(Ordering::Relaxed), files * 4096);
            println!(
                "synthetic-mft,1,false,{elapsed:.6},{files},{},{},0,false,false,false{},Synthetic,Unknown,1",
                dirs + 1,
                files * 4096,
                shared.profile.fields()
            );
        }
    }

    #[test]
    #[ignore = "Read-only whole-volume validation; requires an elevated process and CLAWBACK_MFT_TEST_ROOT"]
    fn elevated_volume_scan() {
        let root =
            std::env::var_os("CLAWBACK_MFT_TEST_ROOT").expect("Set CLAWBACK_MFT_TEST_ROOT to an NTFS volume root");
        let shared = Shared::new(root.into(), ScanOptions::default());
        assert!(scan(&shared).expect("MFT scan failed; this test does not silently fall back"));
        assert!(shared.progress.files.load(Ordering::Relaxed) > 0);
        eprintln!(
            "MFT scan: {} files in {:?}",
            shared.progress.files.load(Ordering::Relaxed),
            shared.started.elapsed()
        );
    }
}
