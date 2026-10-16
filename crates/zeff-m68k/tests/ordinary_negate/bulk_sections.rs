use super::*;

pub(super) const COUNTS: [u32; 21] = [
    432, 172032, 382976, 129024, 382976, 1048576, 24192, 32768, 13920, 1344, 1320, 35904, 3072,
    11968, 2688, 86016, 2688, 43008, 98304, 774144, 11112,
];

pub(super) fn section(index: u32) -> (u32, u32) {
    let result = match index {
        0 => {
            let (count, bad, _) = goldens::run();
            (count, bad)
        }
        1..=4 => bulk_matrix::main_paths((index - 1) as u8),
        5 => bulk_addressing::indexes(),
        6 => bulk_addressing::wraps(),
        7 => bulk_directed::upper_sr(),
        8 => bulk_directed::mutations(),
        9 => bulk_addressing::overlaps(),
        10 => bulk_directed::retained(),
        11 => bulk_capacity::edges(),
        12 => bulk_capacity::idle(),
        13 => bulk_capacity::maximum_waits(),
        14 => bulk_capacity::exact_final(),
        15 => bulk_capacity::odd(),
        16 => bulk_guards::preflight(),
        17 => bulk_negative::bytes(),
        18 => bulk_negative::registers(),
        19 => bulk_negative::other_memory(),
        20 => bulk_negative::other_callbacks(),
        _ => panic!("ordinary NEG/NEGX section"),
    };
    assert_eq!(result.0, COUNTS[index as usize]);
    result
}
