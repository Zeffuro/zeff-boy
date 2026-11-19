use super::*;

pub(super) fn run() -> (u32, u32, u32) {
    let mut count = 0;
    let mut bad = 0;
    let mut unsupported = 0;
    for case in decode::cases() {
        let (good, absent) = check::run(case, bad < 12);
        count += 1;
        bad += u32::from(!good);
        unsupported += u32::from(absent);
    }
    (count, bad, unsupported)
}
