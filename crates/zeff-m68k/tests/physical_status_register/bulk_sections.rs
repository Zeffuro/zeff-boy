use super::*;

pub(super) const COUNTS: [u32; 31] = [
    250, 65536, 44, 360, 1769472, 466944, 1179648, 4608, 110592, 1296, 2736, 864, 1368, 2304, 1368,
    1512, 456, 228, 54, 158720, 82944, 324, 6156, 57, 9120, 4860, 1620, 81, 4311, 1152, 8208,
];

pub(super) fn section(index: u32) -> (u32, u32) {
    let result = match index {
        0 => first::run(),
        1 => bulk_guards::masks(),
        2 => bulk_guards::illegal(),
        3 => bulk_guards::preflight(),
        4 => bulk_matrix::completion(),
        5 => bulk_matrix::faults(),
        6 => bulk_matrix::all_sources(),
        7 => bulk_matrix::upper_junk(),
        8 => bulk_matrix::upper_status(),
        9 => bulk_directed::pc_wraps(),
        10 => bulk_directed::mutations(),
        11 => bulk_directed::overlaps(),
        12 => bulk_directed::retained(),
        13 => bulk_directed::bank_switches(),
        14 => bulk_capacity::reserves(),
        15 => bulk_capacity::idles(),
        16 => bulk_capacity::shorts(),
        17 => bulk_capacity::maximum(),
        18 => bulk_capacity::exact(),
        19 => bulk_directed::exclusions(),
        20 => bulk_next::boundaries(),
        21 => bulk_guards::priorities(),
        22 => bulk_entry::failures(),
        23 => bulk_entry::controls(),
        24 => bulk_entry::secondary(),
        25 => bulk_reset::failures(),
        26 => bulk_reset::geometry(),
        27 => bulk_reset::inputs(),
        28 => bulk_entry_reset_capacity::reserves(),
        29 => bulk_entry_reset_capacity::idles(),
        30 => bulk_entry::wraps(),
        _ => panic!("status register synthetic section"),
    };
    assert_eq!(result.0, COUNTS[index as usize]);
    result
}
