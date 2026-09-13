use std::sync::atomic::AtomicBool;

use super::*;
use crate::ScanStop;
use crate::engine_software::{Cell, ExportMeter};

fn pattern(effects: &[(u8, u8)]) -> Pattern {
    Pattern {
        cells: effects
            .iter()
            .map(|&(effect, parameter)| Cell {
                effect,
                parameter,
                ..Cell::default()
            })
            .collect(),
    }
}

fn validate(patterns: &[Pattern], orders: &[u8], restart: usize) -> ReadResult<()> {
    validate_zero_patterns(
        1,
        orders,
        restart,
        patterns,
        &mut ExportMeter {
            cancel: &AtomicBool::new(false),
            remaining: 10_000,
        },
    )
}

#[test]
fn explicit_jumps_can_skip_zero_patterns() {
    let patterns = [pattern(&[(0x0b, 2)]), pattern(&[]), pattern(&[(0x0b, 0)])];
    assert!(validate(&patterns, &[0, 1, 2], 2).is_ok());
    assert!(validate(&patterns, &[0, 1, 2], 1).is_err());
    assert!(validate(&patterns, &[1, 0, 2], 2).is_err());
}

#[test]
fn unknown_control_flow_and_reachable_zero_patterns_are_rejected() {
    for effect in [(0, 0), (0x0b, 1), (0x0b, 3), (0x0d, 0), (0x0e, 0x61)] {
        let patterns = [pattern(&[effect]), pattern(&[]), pattern(&[(0x0b, 0)])];
        assert!(validate(&patterns, &[0, 1, 2], 2).is_err(), "{effect:?}");
    }
    let patterns = [
        pattern(&[(0x0b, 2), (0, 0)]),
        pattern(&[]),
        pattern(&[(0x0b, 0)]),
    ];
    assert!(validate(&patterns, &[0, 1, 2], 2).is_err());
    let patterns = [
        pattern(&[(0x0b, 2); 257]),
        pattern(&[]),
        pattern(&[(0x0b, 0)]),
    ];
    assert!(validate(&patterns, &[0, 1, 2], 2).is_err());
}

#[test]
fn zero_pattern_proof_obeys_work_and_cancel_limits() {
    let patterns = [pattern(&[(0x0b, 0)]), pattern(&[])];
    for (cancel, remaining, stop) in [
        (false, 0, ScanStop::WorkLimit),
        (true, 100, ScanStop::Cancelled),
    ] {
        assert!(matches!(
            validate_zero_patterns(1, &[0, 1], 0, &patterns, &mut ExportMeter {
                cancel: &AtomicBool::new(cancel), remaining,
            }),
            Err(ReadError::Stop(actual)) if actual == stop
        ));
    }
}
