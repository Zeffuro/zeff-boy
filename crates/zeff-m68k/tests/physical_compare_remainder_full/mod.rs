use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::physical::{
    CriticalFault, EntryBus, EntryFault, EntryKind, EntryOutcome, EntryReport, Group0Capture,
    PhysicalEntryState, PhysicalInspection, ProcessingClass, SswProfile, TimingQualification,
};
use zeff_m68k::timing::physical_instruction::{
    InstructionBus, InstructionFault, InstructionOutcome, InstructionPhase, InstructionReport,
    PhysicalInstructionStep,
};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu, TimedStep, TimingFault};

mod capacity;
mod check;
#[path = "../physical_tst_remainder_full/classifier.rs"]
#[allow(clippy::duplicate_mod)]
mod classifier;
mod device;
mod directed;
mod entry;
mod entry_products;
mod fixed;
mod literal;
mod matrix;
mod mini;
mod model;
mod oracle;
mod preflight;
mod reset;
mod seed;

pub const COUNTS: [u32; 37] = [
    879616, 4752384, 309248, 14496, 9664, 1632, 2448, 21120, 12144, 159456, 24160, 1179648, 15840,
    16896, 11264, 13200, 10560, 19328, 14496, 53152, 24160, 57984, 475200, 316800, 66000, 159456,
    24160, 960, 3456, 21120, 105600, 20480, 8192, 3264, 3264, 65536, 2688,
];
pub const ALLOCATION_COUNTS: [u32; 18] = [
    2640, 14496, 14496, 9664, 1632, 2448, 159456, 24160, 13200, 10560, 19328, 53152, 57984, 475200,
    316800, 66000, 159456, 24160,
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

pub fn mini_section() -> (u32, u32) {
    mini::run()
}

pub fn section_with(index: u32, mark: fn(bool)) -> (u32, u32) {
    let result = match index {
        0..=3 => matrix::run(index, mark),
        4..=6 | 17 | 18 => capacity::run(index, mark),
        7 | 8 | 11..=14 | 27 | 28 | 31..=36 => directed::run(index, mark),
        9 | 10 | 19..=21 | 25 | 26 => entry_products::run(index, mark),
        15 | 16 | 22..=24 | 30 => reset::run(index, mark),
        29 => preflight::run(mark),
        _ => panic!("compare full section"),
    };
    assert_eq!(result.0, COUNTS[index as usize]);
    result
}
pub fn section(index: u32) -> (u32, u32) {
    section_with(index, |_| {})
}
pub fn allocation_section(index: u32, mark: fn(bool)) -> (u32, u32) {
    let result = if index < 2 {
        let mut tally = Tally::default();
        seed::each(|c| {
            if index == 0 {
                tally.add(check::run(c, |_| {}, mark));
            } else {
                for at in 0..c.p.count() {
                    for kind in [
                        model::Failure::Host,
                        model::Failure::Boundary,
                        model::Failure::Unavailable,
                    ] {
                        tally.add(check::run(c, |b| b.fault(at, kind, 4), mark));
                    }
                }
            }
        });
        tally.finish()
    } else {
        section_with(
            [
                0, 1, 3, 4, 5, 6, 9, 10, 15, 16, 17, 19, 21, 22, 23, 24, 25, 26,
            ][index as usize],
            mark,
        )
    };
    assert_eq!(result.0, ALLOCATION_COUNTS[index as usize]);
    result
}
