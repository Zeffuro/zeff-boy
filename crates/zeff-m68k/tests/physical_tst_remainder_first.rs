use std::collections::BTreeMap;
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
#[path = "physical_tst_remainder_first/check.rs"]
mod check;
#[path = "physical_tst_remainder_first/controls.rs"]
mod controls;
#[path = "physical_tst_remainder_first/decode.rs"]
mod decode;
#[path = "physical_status_register_first/default_bus.rs"]
mod default_bus;
#[path = "physical_status_register_first/device.rs"]
mod device;
#[path = "physical_status_register_first/model.rs"]
mod model;
#[path = "physical_tst_remainder_first/value.rs"]
mod value;

pub fn run_first() -> (u32, u32, u32) {
    let (mut count, mut bad, mut unsupported) = (0, 0, 0);
    for case in decode::cases() {
        let (good, absent) = check::run(case, bad < 12);
        count += 1;
        bad += u32::from(!good);
        unsupported += u32::from(absent);
    }
    (count, bad, unsupported)
}

#[test]
fn literal_first248_physical_tst_remainder() {
    let (count, bad, unsupported) = run_first();
    eprintln!("physical TST remainder first count={count} bad={bad} unsupported={unsupported}");
    assert_eq!((count, bad, unsupported), (248, 0, 0));
}
