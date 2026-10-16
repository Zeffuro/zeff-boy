use super::*;
#[path = "literals.rs"]
mod literals;

pub(super) fn run() -> (u32, u32, u32) {
    let mut count = 0;
    let mut bad = 0;
    let mut unsupported = 0;
    for c in literals::cases() {
        let (good, missing) = check::run(c, bad < 8);
        count += 1;
        bad += u32::from(!good);
        unsupported += u32::from(missing);
    }
    (count, bad, unsupported)
}
