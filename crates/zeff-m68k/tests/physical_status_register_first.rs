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
#[path = "physical_status_register_first/check.rs"]
mod check;
#[path = "physical_status_register_first/decode.rs"]
mod decode;
#[path = "physical_status_register_first/default_bus.rs"]
mod default_bus;
#[path = "physical_status_register_first/device.rs"]
mod device;
#[path = "physical_status_register_first/goldens.rs"]
mod goldens;
#[path = "physical_status_register_first/model.rs"]
mod model;
#[path = "physical_status_register_first/value.rs"]
mod value;

#[test]
fn literal_first250_physical_status_register() {
    let (count, bad, unsupported) = goldens::run();
    eprintln!("physical status register first count={count} bad={bad} unsupported={unsupported}");
    assert_eq!((count, bad, unsupported), (250, 0, 9));
}
