use std::sync::atomic::AtomicBool;

use super::*;

fn fixture_at(offset: usize) -> Vec<u8> {
    let payload = [
        0x80, 0x90, 0xa0, 0xb0, 0xc0, 0xb0, 0xa0, 0x90, 0x80, 0x70, 0x60, 0x50, 0x40, 0x50, 0x60,
        0x70,
    ];
    let mut bytes = vec![0; offset];
    bytes.extend_from_slice(&[
        1, 0, 0, 0, b'*', b'm', b'a', b'x', b'm', b'o', b'd', b'*', 16, 0, 0, 0,
    ]);
    bytes.extend_from_slice(&32u32.to_le_bytes());
    bytes.extend_from_slice(&[1, 0x18, 1, 0xba]);
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&[0, 0xba]);
    bytes.extend_from_slice(&0x0208u16.to_le_bytes());
    bytes.extend_from_slice(&payload);
    bytes.extend_from_slice(&[0x80; 4]);
    bytes
}

fn discover_fixture(bytes: &[u8]) -> Result<Vec<SampleBank>, ScanStop> {
    discover(bytes, ScanLimits::default(), &AtomicBool::new(false))
}

#[test]
fn canonical_standalone_and_embedded_banks_preserve_exact_spans() {
    for offset in [0, 13_312] {
        let bytes = fixture_at(offset);
        let banks = discover_fixture(&bytes).unwrap();
        assert_eq!(banks.len(), 1);
        assert_eq!(
            banks[0],
            SampleBank {
                bank: span(offset, 56),
                table: span(offset + 12, 4),
                samples: vec![Sample {
                    record: span(offset + 16, 40),
                    header: span(offset + 24, 12),
                    payload: span(offset + 36, 16),
                    guard: span(offset + 52, 4),
                    frequency_code: 0x0208,
                }],
            }
        );
    }
}

#[test]
fn every_truncation_and_structural_refusal_is_not_discovered() {
    let bytes = fixture_at(0);
    for end in 0..bytes.len() {
        assert!(discover_fixture(&bytes[..end]).unwrap().is_empty(), "{end}");
    }
    let cases: &[(usize, u8)] = &[
        (0, 0),
        (2, 1),
        (12, 20),
        (16, 31),
        (20, 2),
        (21, 0x19),
        (22, 0),
        (24, 0),
        (28, 0),
        (32, 1),
        (33, 0),
        (52, 0),
    ];
    for &(offset, value) in cases {
        let mut changed = bytes.clone();
        changed[offset] = value;
        assert!(
            discover_fixture(&changed).unwrap().is_empty(),
            "{offset:#x}"
        );
    }
    for &(offset, value) in &[
        (20, 0),
        (20, 3),
        (21, 0),
        (22, 2),
        (23, 0),
        (32, 2),
        (33, 0),
        (52, 0),
    ] {
        let mut changed = bytes.clone();
        changed[offset] = value;
        assert!(
            discover_fixture(&changed).unwrap().is_empty(),
            "{offset:#x}={value:#x}"
        );
    }
    assert!(discover_fixture(&fixture_at(1)).unwrap().is_empty());
}

#[test]
fn suffix_bytes_do_not_extend_bank_spans_or_hide_other_candidates() {
    let mut bytes = fixture_at(12);
    let expected = discover_fixture(&bytes).unwrap();
    bytes.extend_from_slice(&[0x42; 19]);
    assert_eq!(discover_fixture(&bytes).unwrap(), expected);
    bytes.resize(88, 0x42);
    bytes.extend_from_slice(&fixture_at(0));
    bytes.extend_from_slice(&[0x43; 7]);
    let banks = discover_fixture(&bytes).unwrap();
    assert_eq!(banks.len(), 2);
    assert_eq!(banks[0], expected[0]);
    assert_eq!(banks[1].bank, span(88, 56));
    assert_eq!(
        discover(
            &bytes,
            ScanLimits {
                max_candidates: 1,
                ..ScanLimits::default()
            },
            &AtomicBool::new(false),
        ),
        Err(ScanStop::CandidateLimit)
    );
}

#[test]
fn suffix_does_not_mask_corrupt_guard_or_record_length() {
    let mut bytes = fixture_at(8);
    bytes.extend_from_slice(&[0x80; 64]);
    bytes[8 + 52] = 0;
    assert!(discover_fixture(&bytes).unwrap().is_empty());
    bytes[8 + 52] = 0x80;
    bytes[8 + 16..8 + 20].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(discover_fixture(&bytes).unwrap().is_empty());
}

#[test]
fn oversized_payload_never_becomes_an_inventory_entry() {
    let mut bytes = fixture_at(0);
    let payload_len = MAX_PAYLOAD_LEN + 1;
    let body_len = payload_len + HEADER_LEN + GUARD_LEN;
    bytes[16..20].copy_from_slice(&(body_len as u32).to_le_bytes());
    bytes[24..28].copy_from_slice(&(payload_len as u32).to_le_bytes());
    bytes.resize(36 + payload_len, 0);
    bytes.extend_from_slice(&[0x80; 4]);
    assert!(discover_fixture(&bytes).unwrap().is_empty());
}

#[test]
fn cancellation_work_and_candidate_limits_propagate() {
    let bytes = fixture_at(0);
    assert_eq!(
        discover(&bytes, ScanLimits::default(), &AtomicBool::new(true)),
        Err(ScanStop::Cancelled)
    );
    assert_eq!(
        discover(
            &bytes,
            ScanLimits {
                max_work: 0,
                ..ScanLimits::default()
            },
            &AtomicBool::new(false),
        ),
        Err(ScanStop::WorkLimit)
    );
    assert_eq!(
        discover(
            &bytes,
            ScanLimits {
                max_candidates: 0,
                ..ScanLimits::default()
            },
            &AtomicBool::new(false),
        ),
        Err(ScanStop::CandidateLimit)
    );
}

#[test]
fn invalid_limits_and_oversized_media_stop_before_scanning() {
    assert_eq!(
        discover(
            &[],
            ScanLimits {
                max_work: MAX_SCAN_WORK + 1,
                ..ScanLimits::default()
            },
            &AtomicBool::new(false),
        ),
        Err(ScanStop::InvalidLimits)
    );
    assert_eq!(
        discover(
            &vec![0; MAX_ROM_BYTES + 1],
            ScanLimits::default(),
            &AtomicBool::new(false),
        ),
        Err(ScanStop::MediaLimit)
    );
}

fn multiple_samples(offset: usize, lengths: &[usize]) -> Vec<u8> {
    let mut bytes = vec![0; offset];
    bytes.extend_from_slice(&(lengths.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(b"*maxmod*");
    bytes.resize(offset + 12 + lengths.len() * 4, 0);
    for (index, &length) in lengths.iter().enumerate() {
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0xba);
        }
        let relative = (bytes.len() - offset) as u32;
        let pointer = offset + 12 + index * 4;
        bytes[pointer..pointer + 4].copy_from_slice(&relative.to_le_bytes());
        bytes.extend_from_slice(&(length as u32 + 16).to_le_bytes());
        bytes.extend_from_slice(&[1, 0x18, 1, 0xba]);
        bytes.extend_from_slice(&(length as u32).to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&[0, 0xba]);
        bytes.extend_from_slice(&(index as u16).to_le_bytes());
        bytes.resize(bytes.len() + length, 0x40 + (index % 64) as u8);
        bytes.extend_from_slice(&[0x80; 4]);
    }
    bytes
}

#[test]
fn multiple_samples_keep_table_order_padding_and_individual_spans() {
    for offset in [0, 28] {
        let mut source = multiple_samples(offset, &[1, 2, 3]);
        source.extend_from_slice(&[0x42; 17]);
        let banks = discover_fixture(&source).unwrap();
        assert_eq!(banks.len(), 1);
        assert_eq!(banks[0].bank, span(offset, 107));
        assert_eq!(banks[0].table, span(offset + 12, 12));
        for (index, (record, length)) in [(24, 1), (52, 2), (80, 3)].into_iter().enumerate() {
            assert_eq!(
                banks[0].samples[index],
                Sample {
                    record: span(offset + record, 24 + length),
                    header: span(offset + record + 8, 12),
                    payload: span(offset + record + 20, length),
                    guard: span(offset + record + 20 + length, 4),
                    frequency_code: index as u16,
                }
            );
        }
        assert_eq!(banks[0].samples.len(), 3);
    }
}

#[test]
fn multisample_truncations_bad_records_and_padding_never_admit_partial_banks() {
    let source = multiple_samples(0, &[1, 2, 3]);
    for end in 0..source.len() {
        assert!(
            discover_fixture(&source[..end]).unwrap().is_empty(),
            "{end}"
        );
    }
    for (offset, value) in [
        (2, 1),
        (49, 0),
        (50, 0),
        (51, 0),
        (78, 0),
        (79, 0),
        (52, 0),
        (56, 2),
        (57, 0),
        (58, 0),
        (59, 0),
        (60, 0),
        (64, 0),
        (68, 1),
        (69, 0),
        (77, 0),
        (106, 0),
    ] {
        let mut changed = source.clone();
        changed[offset] = value;
        assert!(discover_fixture(&changed).unwrap().is_empty(), "{offset}");
    }
    for (pointer, value) in [
        (12, 20u32),
        (16, 24),
        (16, 49),
        (16, 56),
        (16, u32::MAX),
        (20, 52),
    ] {
        let mut changed = source.clone();
        changed[pointer..pointer + 4].copy_from_slice(&value.to_le_bytes());
        assert!(discover_fixture(&changed).unwrap().is_empty());
    }
}

#[test]
fn sample_count_and_per_record_work_are_bounded() {
    let source = multiple_samples(0, &vec![1; MAX_SAMPLES]);
    assert_eq!(
        discover_fixture(&source).unwrap()[0].samples.len(),
        MAX_SAMPLES
    );
    assert!(
        discover_fixture(&multiple_samples(0, &vec![1; MAX_SAMPLES + 1]))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        discover(
            &source,
            ScanLimits {
                max_work: 3,
                ..ScanLimits::default()
            },
            &AtomicBool::new(false)
        ),
        Err(ScanStop::WorkLimit)
    );
}

#[test]
fn zero_and_one_byte_padding_and_duplicate_payloads_preserve_distinct_records() {
    let source = multiple_samples(0, &[3, 4, 1]);
    let banks = discover_fixture(&source).unwrap();
    assert_eq!(banks[0].bank, span(0, 105));
    assert_eq!(banks[0].samples[1].record, span(52, 28));
    assert_eq!(banks[0].samples[2].record, span(80, 25));
    let mut duplicate = multiple_samples(0, &[4, 4]);
    duplicate.copy_within(40..44, 68);
    let banks = discover_fixture(&duplicate).unwrap();
    assert_eq!(banks[0].samples.len(), 2);
    assert_eq!(banks[0].samples[0].payload, span(40, 4));
    assert_eq!(banks[0].samples[1].payload, span(68, 4));
    assert_ne!(banks[0].samples[0].record, banks[0].samples[1].record);
}
