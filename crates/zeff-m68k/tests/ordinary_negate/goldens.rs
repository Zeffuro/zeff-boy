use super::*;
#[path = "literals.rs"]
mod literals;

pub(super) fn run() -> (u32, u32, u32) {
    let mut count = 0;
    let mut bad = 0;
    let mut sr_only = 0;
    for case in literals::cases() {
        let (good, only_sr) = check::run(case);
        count += 1;
        bad += u32::from(!good);
        sr_only += u32::from(only_sr);
    }
    (count, bad, sr_only)
}
