use super::*;

pub(super) const COUNTS: [u32; 30] = [
    261, 65536, 28, 1000, 51200, 235008, 64512, 25088, 10752, 131072, 6240, 12288, 5508, 384, 18,
    270, 7344, 1152, 1170, 1224, 200, 84, 12144, 184, 3680, 18000, 2000, 300, 15072, 4104,
];
pub(super) fn section(index: u32) -> (u32, u32) {
    match index {
        0 => {
            let (count, bad, _) = goldens::run();
            (count, bad)
        }
        1 => bulk_guards::masks(),
        2 => bulk_guards::family_rejected(),
        3 => bulk_guards::preflight(),
        4 => bulk_matrix::completion(),
        5 => bulk_matrix::faults(false),
        6 => bulk_matrix::faults(true),
        7 => bulk_matrix::odd(false),
        8 => bulk_matrix::odd(true),
        9 => bulk_directed::indexes(),
        10 => bulk_directed::wraps(),
        11 => bulk_directed::upper(),
        12 => bulk_directed::mutations(),
        13 => bulk_directed::overlaps(),
        14 => bulk_directed::stack_overlays(),
        15 => bulk_directed::retained(),
        16 => bulk_capacity::reserves(),
        17 => bulk_capacity::idle(),
        18 => bulk_matrix::short(),
        19 => bulk_matrix::maximum(),
        20 => bulk_capacity::exact(),
        21 => bulk_matrix::default_primary(),
        22 => bulk_entry::failures(),
        23 => bulk_entry::controls(),
        24 => bulk_entry::secondary(),
        25 => bulk_reset::failures(),
        26 => bulk_reset::geometry(),
        27 => bulk_reset::inputs(),
        28 => bulk_entry_reset_capacity::reserves(),
        29 => bulk_entry_reset_capacity::idles(),
        _ => (0, 1),
    }
}
