use super::*;
pub(super) const COUNTS: [u32; 18] = [
    32, 107520, 239360, 239360, 131072, 5984, 168, 168, 156, 1344, 8064, 23936, 2688, 128, 648,
    5760, 212, 18,
];
pub(super) fn section(index: u32) -> (u32, u32) {
    match index {
        0 => goldens::run(),
        1 => matrix::main_paths(0),
        2 => matrix::main_paths(1),
        3 => matrix::main_paths(2),
        4 => addressing::indexes(),
        5 => directed::mutation(),
        6 => addressing::overlap(),
        7 => addressing::program_wrap(),
        8 => addressing::operand_wrap(),
        9 => addressing::odd(),
        10 => directed::whole_sr(),
        11 => matrix::max_wait(),
        12 => matrix::exact_final(),
        13 => matrix::source_idle(),
        14 => negative::outside(),
        15 => negative::families(),
        16 => negative::illegal(),
        17 => special::run(),
        _ => panic!("ordinary CMPI section"),
    }
}
pub(super) fn run() -> (u32, u32) {
    let mut total = oracle::Count::default();
    for (index, count) in COUNTS.into_iter().enumerate() {
        let actual = section(index as u32);
        assert_eq!(actual.0, count);
        total.merge(actual);
    }
    assert_eq!(total.count, 766618);
    total.result()
}
