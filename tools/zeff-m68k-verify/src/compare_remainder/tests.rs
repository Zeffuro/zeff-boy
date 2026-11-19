use super::*;
use crate::reference::{Phase, State, Transfer};

fn state() -> State {
    State {
        d0: 0,
        d1: 0,
        d2: 0,
        d3: 0,
        d4: 0,
        d5: 0,
        d6: 0,
        d7: 0,
        a0: 0,
        a1: 0,
        a2: 0,
        a3: 0,
        a4: 0,
        a5: 0,
        a6: 0,
        usp: 0x8000_3000,
        ssp: 0x8765_5000,
        sr: 0x14,
        pc: 0x1004,
        prefetch: [0xbecf, 0x7201],
        ram: vec![(0x1004, 0x74), (0x1005, 0x80)],
    }
}

fn case() -> Case {
    let initial = state();
    let mut expected = state();
    expected.sr = 0x18;
    expected.pc = 0x1006;
    expected.prefetch = [0x7201, 0x7480];
    Case {
        cohort: "CMPA.w".into(),
        name: "active_a7_alias".into(),
        initial,
        expected,
        reference_total_cycles: 6,
        transactions: vec![
            Phase::Read(Transfer {
                duration_cycles: 4,
                function_code: 2,
                address_bus: 0x1004,
                data_bus: 0x7480,
                uds: 1,
                lds: 1,
            }),
            Phase::Idle { duration_cycles: 2 },
        ],
        normalization_exclusion: None,
    }
}

#[test]
fn exact_remaining_word_partition() {
    let mut case = case();
    let mut counts = [0; 5];
    for word in 0..=u16::MAX {
        case.initial.prefetch[0] = word;
        if selected(&case) {
            counts[match word >> 6 & 7 {
                0 => 0,
                1 => 1,
                2 => 2,
                3 => 3,
                7 => 4,
                _ => panic!(),
            }] += 1;
        }
    }
    assert_eq!(counts, [424, 136, 136, 136, 136]);
}

#[test]
fn word_a7_alias_sign_extension_preserves_banks_and_checks_originals() {
    let original = case();
    let plan = Plan::new(&original).unwrap();
    assert_eq!(plan.expected.usp, 0x8000_3000);
    assert_eq!(plan.expected.ssp, 0x8765_5000);
    for axis in 0..6 {
        let mut damaged = case();
        match axis {
            0 => damaged.expected.sr ^= 8,
            1 => damaged.expected.usp ^= 2,
            2 => damaged.expected.prefetch[0] ^= 1,
            3 => damaged.reference_total_cycles += 2,
            4 => {
                if let Phase::Read(transfer) = &mut damaged.transactions[0] {
                    transfer.function_code = 1;
                }
            }
            5 => {
                if let Phase::Read(transfer) = &mut damaged.transactions[0] {
                    transfer.data_bus ^= 1;
                }
            }
            _ => unreachable!(),
        }
        assert!(Plan::new(&damaged).is_err());
    }
}
