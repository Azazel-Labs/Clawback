#![allow(clippy::unwrap_used)]
use super::*;

pub(super) fn reference(index: u64) -> u64 {
    (7 << 48) | index
}

pub(super) fn resident(kind: u32, value: &[u8]) -> Vec<u8> {
    let len = (24 + value.len()).next_multiple_of(8);
    let mut attr = vec![0; len];
    attr[..4].copy_from_slice(&kind.to_le_bytes());
    attr[4..8].copy_from_slice(&(len as u32).to_le_bytes());
    attr[16..20].copy_from_slice(&(value.len() as u32).to_le_bytes());
    attr[20..22].copy_from_slice(&24u16.to_le_bytes());
    attr[24..24 + value.len()].copy_from_slice(value);
    attr
}

pub(super) fn filename(parent: u64, name: &str, namespace: u8) -> Vec<u8> {
    let text: Vec<_> = name.encode_utf16().collect();
    let mut value = vec![0; 66];
    value[..8].copy_from_slice(&parent.to_le_bytes());
    value[64] = text.len() as u8;
    value[65] = namespace;
    for c in text {
        value.extend(c.to_le_bytes());
    }
    resident(0x30, &value)
}

pub(super) fn nonresident(flags: u16, allocated: u64, len: u64, physical: u64) -> Vec<u8> {
    let mut data = vec![0; 80];
    data[..4].copy_from_slice(&0x80u32.to_le_bytes());
    data[4..8].copy_from_slice(&80u32.to_le_bytes());
    data[8] = 1;
    data[12..14].copy_from_slice(&flags.to_le_bytes());
    data[32..34].copy_from_slice(&72u16.to_le_bytes());
    data[40..48].copy_from_slice(&allocated.to_le_bytes());
    data[48..56].copy_from_slice(&len.to_le_bytes());
    data[56..64].copy_from_slice(&len.to_le_bytes());
    data[64..72].copy_from_slice(&physical.to_le_bytes());
    data[72..76].copy_from_slice(&[0x11, 1, 100, 0]);
    data
}

pub(super) fn record(attrs: &[Vec<u8>], directory: bool, base: u64) -> Vec<u8> {
    let mut record = vec![0; 1024];
    record[..4].copy_from_slice(b"FILE");
    record[4..6].copy_from_slice(&48u16.to_le_bytes());
    record[6..8].copy_from_slice(&3u16.to_le_bytes());
    record[16..18].copy_from_slice(&7u16.to_le_bytes());
    record[20..22].copy_from_slice(&56u16.to_le_bytes());
    record[22..24].copy_from_slice(&(if directory { 3u16 } else { 1 }).to_le_bytes());
    record[28..32].copy_from_slice(&1024u32.to_le_bytes());
    record[32..40].copy_from_slice(&base.to_le_bytes());
    let mut offset = 56;
    for attr in attrs {
        record[offset..offset + attr.len()].copy_from_slice(attr);
        offset += attr.len();
    }
    record[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    record[24..28].copy_from_slice(&(offset as u32 + 4).to_le_bytes());
    record
}

pub(super) fn encode_fixups(record: &mut [u8]) {
    record[48..50].copy_from_slice(&[0xab, 0xcd]);
    for i in 1..3 {
        let saved = [record[i * 512 - 2], record[i * 512 - 1]];
        record[48 + i * 2..50 + i * 2].copy_from_slice(&saved);
        record[i * 512 - 2..i * 512].copy_from_slice(&[0xab, 0xcd]);
    }
}

#[test]
fn logical_and_physical_sizes_come_from_data_not_stale_filename_sizes() {
    for (flags, expected) in [(0, 65536), (1, 4096), (0x8000, 4096)] {
        let data = record(&[filename(reference(5), "file", 1), nonresident(flags, 65536, 62000, 4096)], false, 0);
        let parsed = parse(&data, 42).unwrap().unwrap();
        assert!(!parsed.directory);
        assert_eq!(parsed.id, reference(42));
        assert_eq!(parsed.size, Some((expected, 62000)));
    }
    let data = record(&[resident(0x80, b"resident data")], false, 0);
    assert_eq!(parse(&data, 42).unwrap().unwrap().size, Some((0, 13)));
}

#[test]
fn hardlink_names_exclude_dos_aliases_and_extensions_keep_base_identity() {
    let data = record(
        &[
            filename(reference(5), "Long name.txt", 1),
            filename(reference(5), "LONGNA~1.TXT", 2),
            filename(reference(20), "alias.txt", 3),
        ],
        false,
        reference(42),
    );
    let parsed = parse(&data, 43).unwrap().unwrap();
    assert_eq!(parsed.base, reference(42));
    assert_eq!(parsed.names.len(), 2);
    assert_eq!(parsed.names[1].parent, reference(20));
}

#[test]
fn reparse_tags_distinguish_junctions_from_cloud_files() {
    for (tag, surrogate) in [(0xa000_0003u32, true), (0xa000_000c, true), (0x9000_001a, false)] {
        let data = record(&[resident(0xc0, &tag.to_le_bytes())], false, 0);
        assert_eq!(parse(&data, 42).unwrap().unwrap().surrogate, surrogate);
    }
}

#[test]
fn fragmented_mapping_pairs_support_negative_deltas_and_sparse_runs() {
    let mut attr = nonresident(0, 0, 0, 0);
    attr.resize(88, 0);
    attr[4..8].copy_from_slice(&88u32.to_le_bytes());
    attr[24..32].copy_from_slice(&8u64.to_le_bytes());
    attr[72..81].copy_from_slice(&[0x11, 2, 100, 0x11, 3, 0xf6, 0x01, 4, 0]);
    assert_eq!(
        runs(&attr).unwrap(),
        vec![
            Run { vcn: 0, clusters: 2, lcn: Some(100) },
            Run { vcn: 2, clusters: 3, lcn: Some(90) },
            Run { vcn: 5, clusters: 4, lcn: None },
        ]
    );
    attr[73] = 0;
    assert!(runs(&attr).is_err());
}

#[test]
fn fixups_restore_sector_tails_and_reject_torn_records() {
    let mut raw = record(&[resident(0x80, &[23; 600])], false, 0);
    let expected = parse(&raw, 42).unwrap().unwrap().size;
    encode_fixups(&mut raw);
    let mut torn = raw.clone();
    torn[1022] ^= 1;
    assert!(fixup(&mut torn).is_err());
    fixup(&mut raw).unwrap();
    assert_eq!(parse(&raw, 42).unwrap().unwrap().size, expected);
}

#[test]
fn malformed_records_never_panic_or_accept_truncated_attributes() {
    let valid = record(&[filename(reference(5), "file", 1), nonresident(0, 4096, 1234, 4096)], false, 0);
    let used = u32_at(&valid, 24).unwrap() as usize;
    for len in 0..used {
        assert!(parse(&valid[..len], 42).is_err());
    }
    for offset in 0..valid.len() {
        for mask in [1, 0x80, 0xff] {
            let mut changed = valid.clone();
            changed[offset] ^= mask;
            let _ = parse(&changed, 42);
            let _ = fixup(&mut changed);
        }
    }
    for name in ["..", "a/b", "a\\b", "nul\0name"] {
        assert!(parse(&record(&[filename(reference(5), name, 1)], false, 0), 42).is_err());
    }
}
