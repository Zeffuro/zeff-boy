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
#[path = "physical_from_sr_first/check.rs"]
mod check;
#[path = "physical_from_sr_first/device.rs"]
mod device;
#[path = "physical_from_sr_first/goldens.rs"]
mod goldens;
#[path = "physical_from_sr_first/literals.rs"]
mod literals;
#[path = "physical_from_sr_first/model.rs"]
mod model;

#[test]
fn literal_first261_physical_move_from_sr() {
    let (count, bad, unsupported) = goldens::run();
    eprintln!("physical MOVE-from-SR first count={count} bad={bad} unsupported={unsupported}");
    assert_eq!((count, bad, unsupported), (261, 0, 0));
}
