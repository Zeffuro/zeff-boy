use super::*;
use std::sync::atomic::AtomicBool;

fn parse(bytes: &[u8]) -> Result<Option<Data>, ScanStop> {
    inspect(
        bytes,
        0x8600,
        &mut Budget {
            cancel: &AtomicBool::new(false),
            remaining: 10_000,
        },
    )
}

#[test]
fn reachable_data_excludes_the_unused_envelope() {
    let bytes = super::super::fixture::rom(false);
    let data = parse(&bytes).unwrap().unwrap();
    assert_eq!(data.channels.each_ref().map(Vec::len), [5, 5]);
    assert_eq!(
        data.spans,
        [
            prg_span(&bytes, 0x8600, 55).unwrap(),
            prg_span(&bytes, 0x863a, 48).unwrap()
        ]
    );
}

#[test]
fn instrument_changes_require_an_immediate_attack() {
    let bytes = super::super::fixture::rom(false);
    for replacement in [0, 0x8d, 0x80, 0x46, 0x42] {
        let mut changed = bytes.clone();
        changed[0x8641 - 0x8000 + 16] = replacement;
        assert!(parse(&changed).unwrap().is_none(), "opcode {replacement:x}");
    }
    for replacement in [0x85, 0x25, 0x82] {
        let mut changed = bytes.clone();
        changed[0x8640 - 0x8000 + 16] = replacement;
        assert!(parse(&changed).unwrap().is_none());
    }
}

#[test]
fn unsupported_dispatch_and_envelope_modes_are_refused() {
    let bytes = super::super::fixture::rom(false);
    for (offset, replacement) in [
        (0, 3),
        (0x41, 0x40),
        (0x41, 0x44),
        (0x41, 0x48),
        (0x41, 0x7e),
        (0x3f, 0),
        (0x3f, 128),
        (0x29, 0x80),
        (0x2e, 1),
        (0x30, 0),
        (0x33, 0xc1),
        (0x3a, 0xc3),
        (0x41, 0xff),
    ] {
        let mut changed = bytes.clone();
        changed[0x610 + offset] = replacement;
        assert!(parse(&changed).unwrap().is_none(), "offset {offset:x}");
    }
    let mut changed = bytes;
    changed[0x8646 - 0x8000 + 16..0x8648 - 0x8000 + 16].copy_from_slice(&0x86ff_u16.to_le_bytes());
    assert!(parse(&changed).unwrap().is_none());
}

#[test]
fn data_walk_keeps_cancellation_and_work_limits() {
    let bytes = super::super::fixture::rom(false);
    for remaining in [0, 1, 20, 50] {
        assert!(matches!(
            inspect(
                &bytes,
                0x8600,
                &mut Budget {
                    cancel: &AtomicBool::new(false),
                    remaining
                }
            ),
            Err(ScanStop::WorkLimit)
        ));
    }
    assert!(matches!(
        inspect(
            &bytes,
            0x8600,
            &mut Budget {
                cancel: &AtomicBool::new(true),
                remaining: 10_000
            }
        ),
        Err(ScanStop::Cancelled)
    ));
}
