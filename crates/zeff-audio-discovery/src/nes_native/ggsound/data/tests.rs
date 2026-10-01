use super::*;
use std::sync::atomic::AtomicBool;

fn parses(bytes: &[u8]) -> bool {
    inspect(
        bytes,
        0x8000,
        0x8004,
        &mut Budget {
            cancel: &AtomicBool::new(false),
            remaining: 10_000,
        },
    )
    .unwrap()
    .is_some()
}

#[test]
fn mapped_data_keeps_only_the_declared_instrument_and_streams() {
    let bytes = super::super::rom(false);
    let data = inspect(
        &bytes,
        0x8000,
        0x8004,
        &mut Budget {
            cancel: &AtomicBool::new(false),
            remaining: 10_000,
        },
    )
    .unwrap()
    .unwrap();
    let ranges: Vec<_> = data
        .spans
        .iter()
        .map(|span| (span.canonical_cpu_address - 0x8000, span.byte_len))
        .collect();
    assert_eq!(ranges, [(0, 6), (8, 9), (26, 12), (40, 34), (76, 22)]);
    assert_eq!(data.channels.each_ref().map(Vec::len), [2, 2]);
    assert!(
        data.channels
            .iter()
            .flatten()
            .all(|channel| channel.sequence.byte_len == 6)
    );
    let mut low_note = bytes;
    low_note[16 + 55] = 0;
    assert!(parses(&low_note));
}

#[test]
fn uninitialized_notes_and_unclosed_return_paths_are_refused() {
    let original = super::super::rom(false);
    for (offset, value) in [
        (52, 0x24),
        (53, 1),
        (54, 0x75),
        (54, 0x24),
        (55, 0x74),
        (55, 0x73),
        (55, 0x76),
        (56, 0x70),
        (40, 0x75),
        (43, 0x74),
        (44, 41),
        (41, 41),
        (55, 0xff),
    ] {
        let mut bytes = original.clone();
        bytes[16 + offset] = value;
        assert!(!parses(&bytes), "offset {offset}, value {value:02x}");
    }
    let mut no_return = original;
    no_return[16 + 56..16 + 256].fill(0x24);
    assert!(!parses(&no_return));
}

#[test]
fn unsupported_envelopes_and_pointer_aliases_are_refused() {
    let original = super::super::rom(false);
    for (offset, value) in [
        (8, 0),
        (9, 3),
        (10, 3),
        (11, 0),
        (11, 16),
        (12, 0x7f),
        (13, 1),
        (14, 0x7f),
        (15, 0xc0),
        (16, 0x2a),
        (27, 0),
        (29, 0),
        (34, 1),
        (36, 1),
        (31, 0xc0),
        (0, 4),
        (4, 40),
        (42, 0x81),
    ] {
        let mut bytes = original.clone();
        bytes[16 + offset] = value;
        assert!(!parses(&bytes), "offset {offset}, value {value:02x}");
    }
}

#[test]
fn cancellation_limits_and_truncated_sources_stop_the_walk() {
    let bytes = super::super::rom(false);
    for (cancelled, remaining) in [(true, 10_000), (false, 1)] {
        assert!(
            inspect(
                &bytes,
                0x8000,
                0x8004,
                &mut Budget {
                    cancel: &AtomicBool::new(cancelled),
                    remaining,
                }
            )
            .is_err()
        );
    }
    assert!(!parses(&bytes[..32]));
}
