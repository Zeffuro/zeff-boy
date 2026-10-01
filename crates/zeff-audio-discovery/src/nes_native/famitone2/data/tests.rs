use super::*;
use std::sync::atomic::AtomicBool;

fn parse(bytes: &[u8], work: u64) -> Result<Option<Data>, ScanStop> {
    inspect(
        bytes,
        0x8462,
        &mut Budget {
            cancel: &AtomicBool::new(false),
            remaining: work,
        },
    )
}

#[test]
fn both_cues_share_closed_envelopes_and_exact_data_ranges() {
    let bytes = super::super::fixture::rom(false);
    let data = parse(&bytes, 10_000).unwrap().unwrap();
    assert_eq!(data.channels.each_ref().map(Vec::len), [5, 5]);
    assert_eq!(data.spans.len(), 1);
    assert_eq!(data.spans[0], prg_span(&bytes, 0x8462, 107).unwrap());
    assert_eq!(data.channels[0][0].sequence.byte_len, 8);
    assert_eq!(data.channels[1][4].sequence.byte_len, 4);
}

#[test]
fn malformed_and_unsupported_graphs_are_refused() {
    let bytes = super::super::fixture::rom(false);
    let cases: &[(u16, &[u8])] = &[
        (0x8462, &[3]),
        (0x8463, &[0xff, 0xff]),
        (0x8497, &[0x82]),
        (0x8497, &[0xff]),
        (0x8496, &[0]),
        (0x8496, &[0x80]),
        (0x8494, &[6]),
        (0x848e, &[0, 0]),
        (0x848b, &[0xc1]),
        (0x848e, &[0xff]),
        (0x848e, &[0x80]),
        (0x8495, &[0xfd, 0x95, 0x84]),
        (0x849b, &[0x9b, 0x84]),
        (0x849b, &[0xff, 0xff]),
        (0x84a9, &[4]),
        (0x84a9, &[5]),
        (0x8473, &[1, 1]),
        (0x8467, &[0, 0x85]),
    ];
    for &(address, replacement) in cases {
        let mut changed = bytes.clone();
        let offset = usize::from(address - 0x8000) + 16;
        changed[offset..offset + replacement.len()].copy_from_slice(replacement);
        assert!(
            parse(&changed, 10_000).unwrap().is_none(),
            "address {address:x}, bytes {replacement:x?}"
        );
    }
}

#[test]
fn limits_and_cancellation_survive_nested_traversal() {
    let bytes = super::super::fixture::rom(false);
    for remaining in [0, 1, 10, 40] {
        assert!(matches!(parse(&bytes, remaining), Err(ScanStop::WorkLimit)));
    }
    assert!(matches!(
        inspect(
            &bytes,
            0x8462,
            &mut Budget {
                cancel: &AtomicBool::new(true),
                remaining: 10_000,
            }
        ),
        Err(ScanStop::Cancelled)
    ));
    for header in [0, 0x7fff, 0xfff0] {
        assert!(
            inspect(
                &bytes,
                header,
                &mut Budget {
                    cancel: &AtomicBool::new(false),
                    remaining: 10_000
                }
            )
            .unwrap()
            .is_none()
        );
    }
}

#[test]
fn all_rest_or_zero_volume_cues_are_not_admitted() {
    let bytes = super::super::fixture::rom(false);
    let mut rests = bytes.clone();
    for address in [0x8498, 0x84a0, 0x84b4, 0x84bc] {
        rests[address - 0x8000 + 16] = 0;
    }
    assert!(parse(&rests, 10_000).unwrap().is_none());
    let mut silent = bytes;
    silent[0x848e - 0x8000 + 16..0x8493 - 0x8000 + 16].fill(0xc0);
    assert!(parse(&silent, 10_000).unwrap().is_none());
}

#[test]
fn channel_jumps_cannot_claim_unvisited_source_gaps() {
    let mut bytes = super::super::fixture::rom(false);
    bytes[0x849b - 0x8000 + 16..0x849d - 0x8000 + 16].copy_from_slice(&0x84d0_u16.to_le_bytes());
    bytes[0x84d0 - 0x8000 + 16..0x84d6 - 0x8000 + 16]
        .copy_from_slice(&[0x80, 4, 0x87, 0xfd, 0xd0, 0x84]);
    assert!(parse(&bytes, 10_000).unwrap().is_none());
}

#[test]
fn four_channel_data_has_closed_spans_and_rejects_unsupported_shapes() {
    let bytes = super::super::four_channel_rom(false);
    let data = parse(&bytes, 10_000).unwrap().unwrap();
    assert!(data.four_channels);
    assert_eq!(data.spans, [prg_span(&bytes, 0x8462, 123).unwrap()]);
    for (address, replacement) in [
        (0x84a8, 0),
        (0x84b0, 34),
        (0x84b5, 4),
        (0x8483, 0xf0),
        (0x848b, 0xc1),
    ] {
        let mut changed = bytes.clone();
        changed[address - 0x8000 + 16] = replacement;
        assert!(
            parse(&changed, 10_000).unwrap().is_none(),
            "address {address:x}"
        );
    }
    let mut mixed = bytes.clone();
    mixed[0x846b - 0x8000 + 16..0x846d - 0x8000 + 16].copy_from_slice(&0x84b5_u16.to_le_bytes());
    assert!(parse(&mixed, 10_000).unwrap().is_none());
    for noise in [0x84b0, 0x84d4] {
        for note in [2, 32, 33] {
            let mut changed = bytes.clone();
            changed[noise - 0x8000 + 16] = note;
            assert!(parse(&changed, 10_000).unwrap().is_some());
        }
    }
}

#[test]
fn triangle_and_noise_can_rest_between_notes() {
    let mut bytes = super::super::four_channel_rom(false);
    for address in [0x84a9, 0x84b1, 0x84cd, 0x84d5] {
        bytes[address - 0x8000 + 16] = 0;
    }
    assert!(parse(&bytes, 10_000).unwrap().unwrap().four_channels);
}
