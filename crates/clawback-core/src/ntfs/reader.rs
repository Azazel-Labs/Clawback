//! Whole-volume MFT ingestion. Bootstrap $MFT's extents with documented NTFS
//! control codes, then read records in 1 mebibyte batches, not one ioctl per file.
use super::{
    ATTRIBUTE_LIST, DATA, IN_USE, INDEX_MASK, Record, Run, attributes, bytes, fixup, invalid, is_file_record,
    parse_with_reparse, runs, u16_at, u32_at, u64_at,
};
use crate::profiling::Phase;
use crate::scan::{MftPhase, Shared, lock};
use crate::tree::{Kind, NewEntry, NodeId, ROOT, Tree, flags};
use crate::windows;
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::os::windows::ffi::OsStringExt;
use std::sync::atomic::Ordering;

const GET_VOLUME: u32 = 0x0009_0064; // FSCTL_GET_NTFS_VOLUME_DATA
const GET_RECORD: u32 = 0x0009_0068; // FSCTL_GET_NTFS_FILE_RECORD
const BATCH: usize = 1024 * 1024;
/// Assembled entries are added to the tree in groups of this many.
const ASSEMBLE_BATCH: usize = 256;
/// The root directory's record number. Lower numbers are reserved metadata.
const ROOT_RECORD: u64 = 5;
const FIRST_USER_RECORD: u64 = 16;
/// Attribute lists are metadata; never let an on-disk length allocate an
/// unbounded buffer. Larger lists use fallback.
const MAX_ATTRIBUTE_LIST: u64 = 16 * 1024 * 1024;

/// DASD handles obey noncached I/O restrictions even without an explicit
/// flag, so physical reads go through a buffer aligned for any sector size.
/// <https://learn.microsoft.com/windows/win32/api/fileapi/nf-fileapi-createfilew>
#[repr(C, align(65536))]
struct ReadBuffer([u8; BATCH]);

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
    read_buffer: Box<ReadBuffer>,
}

impl Volume {
    fn new(file: File, cluster: u64, sector: usize, clusters: u64, record_size: usize, valid_len: u64) -> Self {
        // SAFETY: ReadBuffer contains only bytes, for which all-zero is valid.
        let read_buffer = unsafe { Box::<ReadBuffer>::new_zeroed().assume_init() };
        Self { file, cluster, sector, clusters, record_size, valid_len, read_buffer }
    }

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
        Ok(Self::new(file, cluster, sector as usize, u64_at(&data, 16)?, record_size, valid_len))
    }

    fn record(&self, reference: u64) -> io::Result<Vec<u8>> {
        let mut data = vec![0u8; self.record_size + 16];
        let index = reference & INDEX_MASK;
        let len = windows::control(&self.file, GET_RECORD, &index.to_le_bytes(), &mut data)?;
        if len < 12 || u64_at(&data, 0)? & INDEX_MASK != index || u32_at(&data, 8)? as usize != self.record_size {
            return Err(invalid());
        }
        let record = bytes(&data[..len], 12, self.record_size)?.to_vec();
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

    /// Read `output.len()` bytes of a stream through the aligned buffer; the
    /// caller's destination need not itself be aligned.
    fn read_stream(&mut self, extents: &[Run], offset: u64, output: &mut [u8]) -> io::Result<()> {
        if !offset.is_multiple_of(self.sector as u64) || !output.len().is_multiple_of(self.sector) {
            return Err(invalid());
        }
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
            let chunk = &mut self.read_buffer.0[..count];
            self.file.read_exact(chunk)?;
            remaining[..count].copy_from_slice(chunk);
            remaining = &mut remaining[count..];
            offset = offset.checked_add(count as u64).ok_or_else(invalid)?;
        }
        Ok(())
    }

    fn mft_runs(&mut self, shared: &Shared) -> io::Result<Vec<Run>> {
        self.mft_runs_with(shared, Self::record)
    }

    fn mft_runs_with(
        &mut self,
        shared: &Shared,
        mut read_record: impl FnMut(&Self, u64) -> io::Result<Vec<u8>>,
    ) -> io::Result<Vec<Run>> {
        let first = read_record(self, 0)?;
        let mut extents = Vec::new();
        let mut references = HashSet::new();
        for attr in attributes(&first)? {
            let attr = attr?;
            if attr.is_contents() {
                extents.extend(runs(attr.data)?);
            }
            if attr.kind != ATTRIBUTE_LIST {
                continue;
            }
            let list = if attr.is_resident() {
                attr.resident()?.to_vec()
            } else {
                let len = u64_at(attr.data, 48)?;
                if len > MAX_ATTRIBUTE_LIST {
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
                shared.progress.checkpoint()?;
                let entry = bytes(&list, offset, 26)?;
                let len = usize::from(u16_at(entry, 4)?);
                if len < 26 {
                    return Err(invalid());
                }
                bytes(&list, offset, len)?;
                // An unnamed $DATA extent held in another record.
                if u32_at(entry, 0)? == DATA && entry[6] == 0 {
                    let reference = u64_at(entry, 16)?;
                    if reference & INDEX_MASK != 0 {
                        references.insert(reference);
                    }
                }
                offset += len;
            }
        }
        for reference in references {
            shared.progress.checkpoint()?;
            let record = read_record(self, reference)?;
            if u64_at(&record, 32)? != u64::from(u16_at(&first, 16)?) << 48 {
                return Err(invalid());
            }
            for attr in attributes(&record)? {
                let attr = attr?;
                if attr.is_contents() {
                    extents.extend(runs(attr.data)?);
                }
            }
        }
        self.validate_runs(&mut extents, self.valid_len)?;
        Ok(extents)
    }
}

pub(crate) fn scan(shared: &Shared) -> io::Result<bool> {
    let (mut volume, serial) = {
        let _phase = shared.profile.timer(Phase::Volume);
        let Some((file, serial)) = windows::volume(&shared.root)? else { return Ok(false) };
        (Volume::open(file).map_err(|e| context(&e, "Reading NTFS volume information"))?, serial)
    };
    shared.progress.checkpoint()?;
    shared.progress.workers.store(1, Ordering::Relaxed);
    let mapping = {
        let _phase = shared.profile.timer(Phase::Bootstrap);
        volume.mft_runs(shared).map_err(|e| context(&e, "Locating the MFT data extents"))?
    };
    ingest(shared, serial, &mut volume, &mapping)?;
    Ok(true)
}

fn ingest(shared: &Shared, serial: u64, volume: &mut Volume, mapping: &[Run]) -> io::Result<()> {
    let mut records = read_records(shared, volume, mapping)?;
    lock(&shared.mft_progress).phase = MftPhase::Resolving;
    records.merge_extensions(shared)?;
    let tree = build_tree(shared, serial, records).map_err(|e| context(&e, "Assembling the MFT directory tree"))?;
    publish(shared, tree)
}

/// What assembly needs from one base record. A whole-volume ingest holds one per MFT
/// slot, so this stays small; names live in [`Records::text`].
#[derive(Clone, Copy, Default)]
struct Slot {
    allocated: u64,
    len: u64,
    mtime: i64,
    sequence: u16,
    flags: u8,
    /// Distinct names, up to `u8::MAX`.
    links: u8,
}

impl Slot {
    const IN_USE: u8 = 1;
    const DIRECTORY: u8 = 2;
    const SURROGATE: u8 = 4;
    const SIZED: u8 = 8;

    fn has(self, flag: u8) -> bool {
        self.flags & flag != 0
    }
}

/// One hard link: `child`'s name inside the directory `parent`.
#[derive(Clone, Copy)]
struct Link {
    /// The parent directory's full file reference, sequence number included.
    parent: u64,
    /// The named record's index.
    child: u32,
    /// Offset of the name in [`Records::text`], stored there after its length.
    start: u32,
}

/// The fields an extension record contributes to its base.
struct Extension {
    base: u64,
    surrogate: bool,
    size: Option<(u64, u64)>,
}

/// Base records by MFT index, with every link name in one buffer.
#[derive(Default)]
struct Records {
    slots: Vec<Slot>,
    text: Vec<u16>,
    links: Vec<Link>,
    extensions: Vec<Extension>,
    count: u64,
}

impl Records {
    fn with_capacity(slots: usize) -> Self {
        Self { slots: Vec::with_capacity(slots), ..Self::default() }
    }

    #[cfg(test)]
    fn from_parsed(records: impl IntoIterator<Item = Record>) -> io::Result<Self> {
        let mut result = Self::default();
        for record in records {
            result.insert(record)?;
        }
        Ok(result)
    }

    fn insert(&mut self, record: Record) -> io::Result<()> {
        let owner = if record.base == 0 { record.id } else { record.base };
        let index = u32::try_from(owner & INDEX_MASK).map_err(|_| invalid())?;
        for name in record.names {
            let start = u32::try_from(self.text.len()).map_err(|_| invalid())?;
            self.text.push(u16::try_from(name.text.len()).map_err(|_| invalid())?);
            self.text.extend_from_slice(&name.text);
            self.links.push(Link { parent: name.parent, child: index, start });
        }
        if record.base != 0 {
            self.extensions.push(Extension { base: record.base, surrogate: record.surrogate, size: record.size });
            return Ok(());
        }
        let index = index as usize;
        if self.slots.len() <= index {
            self.slots.resize(index + 1, Slot::default());
        }
        let slot = &mut self.slots[index];
        if slot.has(Slot::IN_USE) {
            return Err(invalid());
        }
        let (allocated, len) = record.size.unwrap_or_default();
        *slot = Slot {
            allocated,
            len,
            mtime: record.mtime,
            sequence: (record.id >> 48) as u16,
            flags: Slot::IN_USE
                | if record.directory { Slot::DIRECTORY } else { 0 }
                | if record.surrogate { Slot::SURROGATE } else { 0 }
                | if record.size.is_some() { Slot::SIZED } else { 0 },
            links: 0,
        };
        self.count += 1;
        Ok(())
    }

    /// The in-use base record `reference` names, if its sequence number still matches.
    fn get(&self, reference: u64) -> Option<Slot> {
        let slot = *self.slots.get(usize::try_from(reference & INDEX_MASK).ok()?)?;
        (slot.has(Slot::IN_USE) && u64::from(slot.sequence) == reference >> 48).then_some(slot)
    }

    fn reference(&self, index: u32) -> u64 {
        u64::from(index) | u64::from(self.slots[index as usize].sequence) << 48
    }

    fn name(&self, link: Link) -> &[u16] {
        let start = link.start as usize + 1;
        &self.text[start..start + usize::from(self.text[start - 1])]
    }

    /// Fold extension records' reparse state and sizes into their bases. Their names were
    /// already recorded under the base.
    fn merge_extensions(&mut self, shared: &Shared) -> io::Result<()> {
        let _phase = shared.profile.timer(Phase::Merge);
        for extension in std::mem::take(&mut self.extensions) {
            shared.progress.checkpoint()?;
            self.get(extension.base).ok_or_else(invalid)?;
            let base = &mut self.slots[(extension.base & INDEX_MASK) as usize];
            if extension.surrogate {
                base.flags |= Slot::SURROGATE;
            }
            if let Some((allocated, len)) = extension.size {
                if base.has(Slot::SIZED) {
                    return Err(invalid());
                }
                (base.allocated, base.len) = (allocated, len);
                base.flags |= Slot::SIZED;
            }
        }
        Ok(())
    }

    /// Group links by parent, dropping links repeated by base and extension records.
    fn index_links(&mut self, shared: &Shared, root: u32) -> io::Result<()> {
        let text = &self.text;
        let name = |link: &Link| {
            let start = link.start as usize + 1;
            &text[start..start + usize::from(text[start - 1])]
        };
        self.links.retain(|link| link.child != root);
        self.links
            .sort_unstable_by(|a, b| (a.parent, a.child).cmp(&(b.parent, b.child)).then_with(|| name(a).cmp(name(b))));
        self.links.dedup_by(|a, b| a.parent == b.parent && a.child == b.child && name(a) == name(b));
        shared.progress.checkpoint()?;
        if self.links.iter().any(|link| name(link) == [u16::from(b'.')]) {
            return Err(invalid());
        }
        for link in &self.links {
            let slot = &mut self.slots[link.child as usize];
            slot.links = slot.links.saturating_add(1);
        }
        Ok(())
    }

    /// Indexes into `links` of `parent`'s entries.
    fn children(&self, parent: u64) -> std::ops::Range<usize> {
        self.links.partition_point(|link| link.parent < parent)
            ..self.links.partition_point(|link| link.parent <= parent)
    }
}

/// A set of MFT record indexes.
struct Bits(Vec<u64>);

impl Bits {
    fn new(len: usize) -> Self {
        Self(vec![0; len.div_ceil(64)])
    }

    /// Add `index`, returning whether it was absent.
    fn insert(&mut self, index: u32) -> bool {
        let (word, bit) = ((index / 64) as usize, 1u64 << (index % 64));
        let absent = self.0[word] & bit == 0;
        self.0[word] |= bit;
        absent
    }

    fn contains(&self, index: u32) -> bool {
        self.0[(index / 64) as usize] & (1 << (index % 64)) != 0
    }
}

/// Parse every in-use record: base records by index, extension records for later merging.
fn read_records(shared: &Shared, volume: &mut Volume, mapping: &[Run]) -> io::Result<Records> {
    let mut buffer = vec![0u8; BATCH];
    let slots = usize::try_from(volume.valid_len / volume.record_size as u64).map_err(|_| invalid())?;
    let mut records = Records::with_capacity(slots);
    let mut offset = 0;
    lock(&shared.mft_progress).total = volume.valid_len;
    while offset < volume.valid_len {
        shared.progress.checkpoint()?;
        let count = (volume.valid_len - offset).min(BATCH as u64) as usize;
        {
            let _phase = shared.profile.timer(Phase::Read);
            volume.read_stream(mapping, offset, &mut buffer[..count.next_multiple_of(volume.sector)])?;
        }
        let _phase = shared.profile.timer(Phase::Parse);
        for (i, record) in buffer[..count].chunks_exact_mut(volume.record_size).enumerate() {
            let index = offset / volume.record_size as u64 + i as u64;
            // Unused slots can be zero-filled or contain old FILE records.
            if record.iter().all(|&b| b == 0) {
                continue;
            }
            if !is_file_record(record)? {
                return Err(invalid());
            }
            if u16_at(record, 22)? & IN_USE == 0 {
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
                records.insert(record)?;
            }
        }
        offset += count as u64;
        let mut progress = lock(&shared.mft_progress);
        progress.read = offset;
        progress.records = records.count;
    }
    Ok(records)
}

/// Publish a validated tree and its totals.
fn publish(shared: &Shared, tree: Tree) -> io::Result<()> {
    let _phase = shared.profile.timer(Phase::Publish);
    shared.progress.checkpoint()?;
    shared.progress.files.store(tree.root().files, Ordering::Relaxed);
    shared.progress.bytes.store(tree.root().size, Ordering::Relaxed);
    shared.progress.dirs.store(tree.dir_count(ROOT), Ordering::Relaxed);
    *lock(&shared.tree) = tree;
    Ok(())
}

fn build_tree(shared: &Shared, serial: u64, mut records: Records) -> io::Result<Tree> {
    let root = ROOT_RECORD as u32;
    let slot = records.slots.get(root as usize).copied().unwrap_or_default();
    if !slot.has(Slot::IN_USE) || !slot.has(Slot::DIRECTORY) {
        return Err(invalid());
    }
    let excluded = {
        let _phase = shared.profile.timer(Phase::Index);
        records.index_links(shared, root)?;
        exclude_reserved(shared, &records, root)?
    };
    let mut tree = Tree::new(&shared.root);
    tree.node_mut(ROOT).mtime = records.slots[root as usize].mtime;
    // Also identifies MFT accounting to incremental reconciliation. Directory
    // traversal deliberately uses listing-only estimates on Windows.
    tree.node_mut(ROOT).file_id = Some((serial, records.reference(root)));
    {
        let _phase = shared.profile.timer(Phase::Assemble);
        assemble(shared, serial, &records, root, &excluded, &mut tree)?;
    }
    // Sorting allocates too; release the records first.
    drop(records);
    let _phase = shared.profile.timer(Phase::Sort);
    lock(&shared.mft_progress).phase = MftPhase::Sorting;
    tree.sort_all();
    Ok(tree)
}

/// Keep the same user-file scope as directory traversal. Reserved NTFS
/// records ($MFT, $Bitmap, $Extend, etc.) and their descendants account for
/// filesystem overhead, not ordinary directory contents. Including them
/// would also make live directory reconciliation remove invisible entries.
fn exclude_reserved(shared: &Shared, records: &Records, root: u32) -> io::Result<Bits> {
    let mut excluded = Bits::new(records.slots.len());
    let mut pending: Vec<u32> = (0..FIRST_USER_RECORD as u32)
        .filter(|&index| index != root && records.slots.get(index as usize).is_some_and(|slot| slot.has(Slot::IN_USE)))
        .collect();
    while let Some(index) = pending.pop() {
        shared.progress.checkpoint()?;
        if excluded.insert(index) {
            pending.extend(records.links[records.children(records.reference(index))].iter().map(|link| link.child));
        }
    }
    Ok(excluded)
}

/// Add every entry reachable from `root` to `tree`.
fn assemble(
    shared: &Shared,
    serial: u64,
    records: &Records,
    root: u32,
    excluded: &Bits,
    tree: &mut Tree,
) -> io::Result<()> {
    let mut pending = vec![(records.reference(root), ROOT)];
    let mut visited_dirs = Bits::new(records.slots.len());
    visited_dirs.insert(root);
    let mut counted = Bits::new(records.slots.len());
    // Parents whose entries were placed, by index of their first link.
    let mut placed = Bits::new(records.links.len());
    let mut total_bytes = 0u64;
    lock(&shared.mft_progress).phase = MftPhase::Assembling;
    while let Some((reference, node)) = pending.pop() {
        shared.progress.checkpoint()?;
        let entries = records.children(reference);
        if entries.is_empty() {
            continue;
        }
        placed.insert(entries.start as u32);
        let mut batch = Vec::with_capacity(ASSEMBLE_BATCH);
        // Subdirectories to visit, by index into `batch`.
        let mut dirs = Vec::new();
        let mut add = |tree: &mut Tree, batch: Vec<NewEntry>, dirs: &mut Vec<(u64, usize)>| {
            let range = tree.add_children(node, batch);
            pending.extend(dirs.drain(..).map(|(id, i)| (id, range.start + i as NodeId)));
            report_assembled(shared, tree);
        };
        for &link in &records.links[entries] {
            shared.progress.checkpoint()?;
            if excluded.contains(link.child) {
                continue;
            }
            let record = records.slots[link.child as usize];
            let reference = records.reference(link.child);
            let kind = if record.has(Slot::SURROGATE) {
                Kind::Symlink
            } else if record.has(Slot::DIRECTORY) {
                Kind::Dir
            } else {
                Kind::File
            };
            if kind == Kind::Dir {
                if !visited_dirs.insert(link.child) {
                    return Err(invalid());
                }
                dirs.push((reference, batch.len()));
            }
            let duplicate = kind != Kind::Dir && shared.options.dedupe_hardlinks && !counted.insert(link.child);
            let size = if duplicate || kind == Kind::Dir {
                0
            } else if shared.options.apparent_size {
                record.len
            } else {
                record.allocated
            };
            total_bytes = total_bytes.checked_add(size).ok_or_else(invalid)?;
            batch.push(NewEntry {
                name: OsString::from_wide(records.name(link)),
                kind,
                size,
                len: record.len,
                mtime: record.mtime,
                flags: if duplicate { flags::HARDLINK_DUP } else { 0 }
                    | if kind != Kind::Dir && record.links > 1 { flags::LINKED } else { 0 },
                file_id: (kind != Kind::Dir).then_some((serial, reference)),
            });
            if batch.len() == ASSEMBLE_BATCH {
                add(tree, std::mem::take(&mut batch), &mut dirs);
            }
        }
        add(tree, batch, &mut dirs);
    }
    // A missing parent or a cycle means the live MFT was not coherent. Children
    // of name-surrogate directories, and reserved metadata, are intentionally not
    // traversed.
    let mut start = 0;
    while start < records.links.len() {
        let parent = records.links[start].parent;
        let group = records.children(parent);
        let skipped = records.get(parent).is_some_and(|slot| slot.has(Slot::SURROGATE))
            || excluded.contains((parent & INDEX_MASK) as u32);
        if !placed.contains(start as u32) && !skipped {
            return Err(invalid());
        }
        start = group.end;
    }
    Ok(())
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
        let volume = Volume::new(File::open(&path).unwrap(), 512, 512, 100, 1024, image.len() as u64);
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
        assert_eq!(telemetry.phase, MftPhase::Sorting);
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
        shared.progress.cancel();
        assert_eq!(ingest(&shared, 123, &mut volume, &mapping).unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert_eq!(lock(&shared.tree).root().files, 0);
        let shared = state();
        volume.valid_len += 1024 * 100;
        assert!(ingest(&shared, 123, &mut volume, &mapping).is_err());
        assert_eq!(lock(&shared.tree).root().files, 0);
    }

    #[test]
    fn stale_parent_references_and_cycles_are_rejected() {
        let shared = state();
        let root = parse(&record(&[], true, 0), 5).unwrap().unwrap();
        let orphan = parse(&record(&[filename(reference(5) + (1 << 48), "stale", 1)], false, 0), 20).unwrap().unwrap();
        assert!(build_tree(&shared, 1, Records::from_parsed([root, orphan]).unwrap()).is_err());
        let root = parse(&record(&[], true, 0), 5).unwrap().unwrap();
        let cycle = parse(&record(&[filename(reference(20), "cycle", 1)], true, 0), 20).unwrap().unwrap();
        assert!(build_tree(&shared, 1, Records::from_parsed([root, cycle]).unwrap()).is_err());
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
            .mft_runs_with(&shared, |_, id| match id {
                0 => Ok(first.clone()),
                id if id == reference(22) => Ok(extension.clone()),
                _ => Err(invalid()),
            })
            .unwrap();
        assert_eq!(mapping, expected);
        let mut wrong = extension;
        wrong[32..40].copy_from_slice(&reference(99).to_le_bytes());
        assert!(
            volume.mft_runs_with(&shared, |_, id| Ok(if id == 0 { first.clone() } else { wrong.clone() })).is_err()
        );
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
        let records = Records::from_parsed([root, extend, metadata, ordinary]).unwrap();
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
        let runs: usize = std::env::var("CLAWBACK_PROFILE_RUNS").map_or(3, |s| s.parse().unwrap());
        for _ in 0..runs {
            let shared = state();
            let mut volume = Volume::new(File::open(&image.0).unwrap(), 512, 512, len / 512, 1024, len);
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
        println!("peak_commit_mib,{}", peak_commit() >> 20);
    }

    /// The process's peak committed private memory, which includes every transient ingest table.
    #[cfg(feature = "profiling")]
    fn peak_commit() -> usize {
        #[repr(C)]
        #[derive(Default)]
        struct Counters {
            cb: u32,
            page_faults: u32,
            peak_working_set: usize,
            working_set: usize,
            quota: [usize; 4],
            pagefile: usize,
            peak_pagefile: usize,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentProcess() -> *mut std::ffi::c_void;
            fn K32GetProcessMemoryInfo(process: *mut std::ffi::c_void, counters: *mut Counters, size: u32) -> i32;
        }
        let mut counters = Counters { cb: size_of::<Counters>() as u32, ..Counters::default() };
        // SAFETY: no arguments; the pseudo-handle needs no closing.
        let process = unsafe { GetCurrentProcess() };
        // SAFETY: a writable structure of the declared size.
        let ok = unsafe { K32GetProcessMemoryInfo(process, &raw mut counters, counters.cb) };
        assert_ne!(ok, 0);
        counters.peak_pagefile
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
