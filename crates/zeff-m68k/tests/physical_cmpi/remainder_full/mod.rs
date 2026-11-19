use super::*;
use zeff_m68k::timing::physical::EntryKind;
mod capacity;
mod check;
mod device;
mod directed;
mod entry;
mod fixed;
mod matrix;
mod model;
mod oracle;
mod reset;
use device::{Device, Scripts};
use fixed::{Events, Ram};

pub const COUNTS: [u32; 31] = [
    248832, 33216, 2076, 1384, 128, 48, 3200, 1152, 22836, 3460, 131072, 648, 1008, 1344, 1080,
    864, 2768, 2076, 7612, 3460, 8304, 48, 38880, 25920, 5400, 22836, 3460, 216, 192, 864, 8640,
];
pub const ALLOCATION_COUNTS: [u32; 16] = [
    216, 33216, 2076, 1384, 22836, 3460, 1080, 864, 2768, 7612, 8304, 38880, 25920, 5400, 22836,
    3460,
];
#[derive(Default)]
struct Tally(u32, u32);
impl Tally {
    fn add(&mut self, good: bool) {
        self.0 += 1;
        self.1 += u32::from(!good);
    }
    fn finish(self) -> (u32, u32) {
        (self.0, self.1)
    }
}
pub fn section_with(index: u32, mark: fn(bool)) -> (u32, u32) {
    let result = match index {
        0..=2 => matrix::run(index, mark),
        3..=5 | 16..=19 => capacity::run(index, mark),
        6 | 7 | 10..=13 | 20 | 21 | 27..=29 => directed::run(index, mark),
        8 | 9 | 25 | 26 => entry::run(index, mark),
        14 | 15 | 22..=24 | 30 => reset::run(index, mark),
        _ => panic!("CMPI full section"),
    };
    assert_eq!(result.0, COUNTS[index as usize]);
    result
}
pub fn allocation_section(index: u32, mark: fn(bool)) -> (u32, u32) {
    let result = if index == 0 {
        let mut t = Tally::default();
        model::each(|c| t.add(check::run(c, |_| {}, mark)));
        t.finish()
    } else {
        section_with(
            [0, 1, 2, 3, 8, 9, 14, 15, 16, 18, 20, 22, 23, 24, 25, 26][index as usize],
            mark,
        )
    };
    assert_eq!(result.0, ALLOCATION_COUNTS[index as usize]);
    result
}
