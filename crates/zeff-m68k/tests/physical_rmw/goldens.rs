use super::*;
#[path = "literals.rs"]
mod literals;

pub(super) fn run() -> (u32, u32) {
    let mut rows = literals::cases();
    rows.sort_by_key(|c| c.index);
    assert_eq!(rows.len(), 70);
    let mut bad = 0;
    for (index, c) in rows.into_iter().enumerate() {
        assert_eq!(c.index, index);
        bad += u32::from(!check::run(c, bad < 3));
    }
    (70, bad)
}
