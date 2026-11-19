use super::{quantize, sum_carriers, transfer};

#[test]
fn signed_top_nine_bits_are_taken_before_carrier_addition() {
    for (raw, expected) in [
        (-33, -2),
        (-32, -1),
        (-31, -1),
        (-1, -1),
        (0, 0),
        (1, 0),
        (31, 0),
        (32, 1),
    ] {
        assert_eq!(quantize(raw), expected);
    }
    assert_eq!(sum_carriers([31, 31, 0, 0], 7), 0);
    assert_eq!(sum_carriers([-1, -1, 0, 0], 7), -64);
}

#[test]
fn carrier_accumulator_saturates_after_each_physical_slot() {
    assert_eq!(sum_carriers([8191, 8191, -8192, 0], 7), -32);
    assert_eq!(sum_carriers([-8192, -8192, 8191, 0], 7), -32);
    assert_eq!(sum_carriers([8191; 4], 7), 8160);
    assert_eq!(sum_carriers([-8192; 4], 7), -8192);
    for (algorithm, expected) in [256, 256, 256, 256, 384, 448, 448, 480]
        .into_iter()
        .enumerate()
    {
        assert_eq!(sum_carriers([32, 64, 128, 256], algorithm as u8), expected);
    }
}

#[test]
fn integrated_transfer_has_a_zero_crossing_gap_and_disabled_sign_leakage() {
    for (value, enabled, disabled) in [
        (-256, -8416, -256),
        (-2, -288, -256),
        (-1, -256, -256),
        (0, 0, 0),
        (1, 32, 0),
        (255, 8160, 0),
    ] {
        assert_eq!(transfer(value, true), enabled);
        assert_eq!(transfer(value, false), disabled);
    }
}
