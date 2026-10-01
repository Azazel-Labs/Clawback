use super::*;

fn word(data: &mut Vec<u8>, value: u32) {
    data.extend(value.to_ne_bytes());
}
fn wide(data: &mut Vec<u8>, value: u64) {
    data.extend(value.to_ne_bytes());
}

fn record(name: &[u8], common: u32, file: u32, kind: u32, error: u32) -> Vec<u8> {
    let mut data = Vec::new();
    for value in [0, common, 0, 0, file, 0] {
        word(&mut data, value);
    }
    if common & ERROR != 0 {
        word(&mut data, error);
    }
    let reference = data.len();
    word(&mut data, 0);
    word(&mut data, (name.len() + 1) as u32);
    if common & DEVICE != 0 {
        word(&mut data, 42);
    }
    if common & OBJECT_TYPE != 0 {
        word(&mut data, kind);
    }
    if common & MOD_TIME != 0 {
        wide(&mut data, 1234);
        wide(&mut data, 56);
    }
    if common & FILE_ID != 0 {
        wide(&mut data, 0x1_0000_0001);
    }
    if file & LINK_COUNT != 0 {
        word(&mut data, 2);
    }
    if file & ALLOC_SIZE != 0 {
        wide(&mut data, 4096);
    }
    if file & DATA_LENGTH != 0 {
        wide(&mut data, 100_000);
    }
    let offset = (data.len() - reference) as u32;
    data[reference..reference + 4].copy_from_slice(&offset.to_ne_bytes());
    data.extend(name);
    data.push(0);
    data.resize(data.len().next_multiple_of(8), 0);
    let length = data.len() as u32;
    data[..4].copy_from_slice(&length.to_ne_bytes());
    data
}

#[test]
fn decodes_packed_metadata_and_raw_names() {
    let mut data = record(b"file\xff", COMMON, FILE, 1, 0);
    data.extend(record(b"folder", COMMON, 0, 2, 0));
    let records = parse_batch(&data, 2).unwrap();
    assert_eq!(records[0].name, b"file\xff");
    assert_eq!(
        records[0].metadata,
        Some(Metadata { device: 42, inode: 0x1_0000_0001, modified: 1234, allocated: 4096, length: 100_000, links: 2 })
    );
    assert_eq!(records[1].name, b"folder");
    assert!(records[1].metadata.is_none());
}

#[test]
fn missing_attributes_and_entry_errors_request_stat() {
    for (common, file, error) in [
        (COMMON & !DEVICE, FILE, 0),
        (COMMON & !FILE_ID, FILE, 0),
        (COMMON & !MOD_TIME, FILE, 0),
        (COMMON, FILE & !ALLOC_SIZE, 0),
        (COMMON, FILE & !DATA_LENGTH, 0),
        (COMMON, FILE & !LINK_COUNT, 0),
        (NAME | RETURNED | ERROR, 0, 13),
        (COMMON, FILE, 13),
    ] {
        let entries = parse_batch(&record(b"file", common, file, 1, error), 1).unwrap();
        assert!(entries[0].metadata.is_none());
    }
    // ERROR need not be returned on successful entries.
    assert!(parse_batch(&record(b"file", COMMON & !ERROR, FILE, 1, 0), 1).unwrap()[0].metadata.is_some());
}

#[test]
fn symlinks_and_special_files_request_stat() {
    for kind in [0, 2, 3, 4, 5, 6, 7] {
        assert!(parse_batch(&record(b"entry", COMMON, FILE, kind, 0), 1).unwrap()[0].metadata.is_none());
    }
}

#[test]
fn rejects_truncation_and_invalid_counts() {
    let data = record(b"file", COMMON, FILE, 1, 0);
    for end in 0..data.len() {
        assert!(parse_batch(&data[..end], 1).is_err());
    }
    assert!(parse_batch(&data, usize::MAX).is_err());
    assert!(parse_batch(&data, 2).is_err());
    for size in [0_u32, 8, 23, 25, u32::MAX] {
        let mut malformed = data.clone();
        malformed[..4].copy_from_slice(&size.to_ne_bytes());
        assert!(parse_batch(&malformed, 1).is_err());
    }
}

#[test]
fn rejects_escaping_and_invalid_names() {
    for name in [b"".as_slice(), b".", b"..", b"a/b", b"a\0b"] {
        assert!(parse_batch(&record(name, COMMON, FILE, 1, 0), 1).is_err());
    }
    for offset in [0_u32, u32::MAX, i32::MAX as u32] {
        let mut data = record(b"file", COMMON, FILE, 1, 0);
        data[28..32].copy_from_slice(&offset.to_ne_bytes());
        assert!(parse_batch(&data, 1).is_err());
    }
    let mut data = record(b"file", COMMON, FILE, 1, 0);
    data[32..36].copy_from_slice(&u32::MAX.to_ne_bytes());
    assert!(parse_batch(&data, 1).is_err());
}

#[test]
fn rejects_unknown_attribute_layouts() {
    let mut data = record(b"file", COMMON, FILE, 1, 0);
    data[4..8].copy_from_slice(&(COMMON | 0x10).to_ne_bytes());
    assert!(parse_batch(&data, 1).is_err());
    let mut data = record(b"file", COMMON, FILE, 1, 0);
    data[4..8].copy_from_slice(&(COMMON & !RETURNED).to_ne_bytes());
    assert!(parse_batch(&data, 1).is_err());
}

#[test]
fn rejects_negative_sizes_invalid_times_and_unterminated_names() {
    for (offset, value) in [(72, u64::MAX), (80, u64::MAX), (52, 1_000_000_000)] {
        let mut data = record(b"file", COMMON, FILE, 1, 0);
        data[offset..offset + 8].copy_from_slice(&value.to_ne_bytes());
        assert!(parse_batch(&data, 1).is_err());
    }
    let mut data = record(b"file", COMMON, FILE, 1, 0);
    data[92] = b'x';
    assert!(parse_batch(&data, 1).is_err());
    // One corrupt record invalidates the entire batch, not just its tail.
    let mut batch = record(b"valid", COMMON, FILE, 1, 0);
    batch.extend(data);
    assert!(parse_batch(&batch, 2).is_err());
}
