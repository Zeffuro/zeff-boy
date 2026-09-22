use super::*;

fn table_image(
    start: usize,
    base: u16,
    dispatcher: usize,
    table: usize,
    pointers: &[u16],
) -> Vec<u8> {
    let mut bytes = image(start, base, false);
    bytes[0x40..0x44].copy_from_slice(&[0xcd, dispatcher as u8, (dispatcher >> 8) as u8, 0xc9]);
    bytes[dispatcher..dispatcher + 18].copy_from_slice(&[
        0xfe,
        pointers.len() as u8,
        0xd0,
        0x6f,
        0x26,
        0,
        0x29,
        0x11,
        table as u8,
        (table >> 8) as u8,
        0x19,
        0x5e,
        0x23,
        0x56,
        0xeb,
        0xc3,
        (start + 0x32) as u8,
        ((start + 0x32) >> 8) as u8,
    ]);
    for (selector, pointer) in pointers.iter().copied().enumerate() {
        word(&mut bytes, table + selector * 2, pointer);
    }
    bytes[0x2100..0x2103].copy_from_slice(&[0xb2, 0x39, 0]);
    bytes
}

#[test]
fn guarded_table_dispatcher_binds_unplayed_entries_and_records_its_abi() {
    let bytes = table_image(0x500, 0xc100, 0x100, 0x180, &[0x2000, 0x2100]);
    let report = discover(&bytes, limits(), &AtomicBool::new(false)).unwrap();
    assert!(report.held.is_empty());
    assert_eq!(report.bound.len(), 2);
    for (selector, song) in report.bound.iter().enumerate() {
        assert_eq!(song.offset, [0x2000, 0x2100][selector]);
        assert!(song.call_sites.is_empty());
        assert!(song.call_roots.is_empty());
        assert_eq!(song.table_entries.len(), 1);
        let entry = &song.table_entries[0];
        assert_eq!(entry.selector, selector as u8);
        assert_eq!(entry.count, 2);
        assert_eq!(entry.dispatcher.offset, 0x100);
        assert_eq!(entry.table.offset, 0x180);
        assert_eq!(entry.entry.offset, 0x180 + selector as u32 * 2);
        assert_eq!(entry.call_sites[0].offset, 0x40);
        assert_eq!(entry.call_roots, [1]);
    }
}

#[test]
fn table_dispatcher_relocates_without_changing_literal_call_serialization() {
    let bytes = table_image(0xa33, 0xcdab, 0x260, 0x400, &[0x2000, 0x2100]);
    let report = discover(&bytes, limits(), &AtomicBool::new(false)).unwrap();
    assert_eq!(report.bound.len(), 2);
    assert_eq!(report.bound[0].evidence.psg_play.offset, 0xa65);
    assert_eq!(report.bound[0].table_entries[0].dispatcher.offset, 0x260);
    let direct = discover(
        &image(0x500, 0xc100, false),
        limits(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(
        serde_json::to_value(&direct.bound[0])
            .unwrap()
            .get("table_entries")
            .is_none()
    );
}

#[test]
fn malformed_or_unreachable_tables_never_bind() {
    let cases = [
        (0x100, 0),
        (0x101, 0),
        (0x102, 0),
        (0x106, 0x23),
        (0x110, 0),
        (0x111, 0),
    ];
    for (at, value) in cases {
        let mut bytes = table_image(0x500, 0xc100, 0x100, 0x180, &[0x2000, 0x2100]);
        bytes[at] = value;
        let report = discover(&bytes, limits(), &AtomicBool::new(false)).unwrap();
        assert!(report.bound.is_empty(), "mutation at {at:#x}");
    }
    let mut invalid_table = table_image(0x500, 0xc100, 0x100, 0x180, &[0x2000, 0x2100]);
    word(&mut invalid_table, 0x108, 0x7fff);
    let report = discover(&invalid_table, limits(), &AtomicBool::new(false)).unwrap();
    assert!(report.bound.is_empty());
    assert!(
        report
            .held
            .iter()
            .any(|held| held.kind == HeldKind::InvalidTable)
    );

    let mut no_frame = table_image(0x500, 0xc100, 0x100, 0x180, &[0x2000, 0x2100]);
    no_frame[0x38] = 0xc9;
    let report = discover(&no_frame, limits(), &AtomicBool::new(false)).unwrap();
    assert!(report.bound.is_empty());
    assert_eq!(report.held[0].kind, HeldKind::FingerprintOnly);

    let mut unreachable = table_image(0x500, 0xc100, 0x100, 0x180, &[0x2000, 0x2100]);
    unreachable[0x40] = 0xc9;
    let report = discover(&unreachable, limits(), &AtomicBool::new(false)).unwrap();
    assert!(report.bound.is_empty());
    assert_eq!(report.held[0].kind, HeldKind::FingerprintOnly);
}

#[test]
fn table_aliases_deduplicate_streams_but_retain_every_selector() {
    let bytes = table_image(0x500, 0xc100, 0x100, 0x180, &[0x2000, 0x2000]);
    let report = discover(&bytes, limits(), &AtomicBool::new(false)).unwrap();
    assert_eq!(report.bound.len(), 1);
    assert_eq!(report.bound[0].table_entries.len(), 2);
    assert_eq!(
        report.bound[0]
            .table_entries
            .iter()
            .map(|entry| entry.selector)
            .collect::<Vec<_>>(),
        [0, 1]
    );
}

#[test]
fn direct_and_table_calls_merge_one_stream_without_losing_table_evidence() {
    let mut bytes = table_image(0x500, 0xc100, 0x100, 0x180, &[0x2000, 0x2100]);
    bytes[0x43..0x4a].copy_from_slice(&[0, 0x21, 0, 0x20, 0xcd, 0x32, 0x05]);
    let report = discover(&bytes, limits(), &AtomicBool::new(false)).unwrap();
    let song = report
        .bound
        .iter()
        .find(|song| song.offset == 0x2000)
        .unwrap();
    assert_eq!(song.call_sites[0].offset, 0x44);
    assert_eq!(song.call_roots, [1]);
    assert_eq!(song.table_entries.len(), 1);
    assert_eq!(song.table_entries[0].selector, 0);
}

#[test]
fn table_decoding_obeys_limits_cancellation_and_per_entry_errors() {
    let bytes = table_image(0x500, 0xc100, 0x100, 0x180, &[0x2000, 0x2100]);
    assert_eq!(
        discover(
            &bytes,
            ScanLimits {
                max_candidates: 1,
                ..limits()
            },
            &AtomicBool::new(false),
        ),
        Err(ScanStop::CandidateLimit)
    );
    assert_eq!(
        discover(&bytes, limits(), &AtomicBool::new(true)),
        Err(ScanStop::Cancelled)
    );
    assert_eq!(
        discover(
            &bytes,
            ScanLimits {
                max_work: 2_000_000,
                ..limits()
            },
            &AtomicBool::new(false),
        ),
        Err(ScanStop::WorkLimit)
    );
    let mut invalid = bytes;
    word(&mut invalid, 0x182, 0x2200);
    let report = discover(&invalid, limits(), &AtomicBool::new(false)).unwrap();
    assert_eq!(report.bound.len(), 1);
    assert!(
        report
            .held
            .iter()
            .any(|held| held.kind == HeldKind::InvalidStream)
    );
    word(&mut invalid, 0x182, 0xffff);
    let report = discover(&invalid, limits(), &AtomicBool::new(false)).unwrap();
    assert_eq!(report.bound.len(), 1);
    assert!(
        report
            .held
            .iter()
            .any(|held| held.kind == HeldKind::InvalidPointer)
    );
}
