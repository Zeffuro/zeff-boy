pub(super) fn run() -> (u32, u32, u32) {
    let cases = super::literals::cases();
    let count = cases.len() as u32;
    let mut bad = 0;
    let mut unsupported = 0;
    for case in cases {
        let (good, absent) = super::check::run(case, bad < 12);
        bad += u32::from(!good);
        unsupported += u32::from(absent);
    }
    (count, bad, unsupported)
}
