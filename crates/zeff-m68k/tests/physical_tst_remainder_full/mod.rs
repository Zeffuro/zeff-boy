use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BUS_ADDRESS_MASK, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::physical::{
    CriticalFault, EntryBus, EntryFault, EntryKind, EntryOutcome, EntryReport, Group0Capture,
    PhysicalEntryState, PhysicalInspection, ProcessingClass, SswProfile,
};
use zeff_m68k::timing::physical_instruction::{
    InstructionBus, InstructionFault, InstructionOutcome, InstructionPhase, InstructionReport,
    PhysicalInstructionStep,
};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu, TimedStep, TimingFault};
mod fixed;
use fixed::{Events, Ram, Scripts};
mod capacity;
mod check;
mod classifier;
mod device;
mod directed;
mod entry;
mod entry_oracle;
mod entry_reset_capacity;
mod guards;
mod matrix;
mod model;
mod oracle;
mod reset;
mod reset_products;
pub const COUNTS: [u32; 33] = [
    248, 65536, 212, 660, 67584, 195072, 25600, 131072, 8192, 16384, 5120, 131072, 65536, 3168,
    2688, 3048, 3168, 2286, 762, 224, 4064, 1016, 132, 22098, 10160, 23760, 7920, 528, 20262, 5484,
    36576, 4224, 1698,
];
pub const ALLOCATION_COUNTS: [u32; 8] = [162600, 22098, 10160, 23760, 528, 20262, 5484, 36576];
#[derive(Default)]
struct Tally {
    count: u32,
    bad: u32,
}
impl Tally {
    fn add(&mut self, good: bool) {
        self.count += 1;
        self.bad += u32::from(!good);
    }
    fn result(self) -> (u32, u32) {
        (self.count, self.bad)
    }
}
pub fn section_with(index: u32, mark: fn(bool)) -> (u32, u32) {
    let result = match index {
        1..=3 => guards::run(index, mark),
        4..=12 => matrix::main(index, mark),
        13..=17 | 31 => directed::run(index, mark),
        18..=22 => capacity::run(index, mark),
        23 | 24 | 30 => entry::run(index, mark),
        25..=27 => reset_products::run(index, mark),
        28 | 29 => entry_reset_capacity::run(index, mark),
        _ => panic!("shared TST sections are 1..31"),
    };
    assert_eq!(result.0, COUNTS[index as usize]);
    result
}
pub fn allocation_section(index: u32, mark: fn(bool)) -> (u32, u32) {
    let result = if index == 0 {
        let mut tally = Tally::default();
        for _ in 0..100 {
            for s in model::encoded() {
                tally.add(check::run(s, |_| {}, mark));
            }
            for (s, at) in model::triggers() {
                for (k, n) in [
                    (device::Kind::Complete, 3),
                    (device::Kind::Host, 6),
                    (device::Kind::Boundary, 8),
                    (device::Kind::Unavailable, 5),
                ] {
                    tally.add(check::run(s, |b| b.report = Some((at, k, n)), mark));
                }
            }
            for (mut s, at) in model::triggers() {
                s.start = u64::MAX - s.idle() - at as u64 * 4 - (capacity::B - 1);
                tally.add(check::run(s, |_| {}, mark));
            }
            let (count, bad) = capacity::run(19, mark);
            tally.count += count;
            tally.bad += bad;
        }
        tally.result()
    } else {
        section_with([0, 23, 24, 25, 27, 28, 29, 30][index as usize], mark)
    };
    assert_eq!(result.0, ALLOCATION_COUNTS[index as usize]);
    result
}
