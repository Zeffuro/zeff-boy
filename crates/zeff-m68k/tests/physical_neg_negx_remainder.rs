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
#[path = "physical_neg_negx_remainder/check.rs"]
mod check;
#[path = "physical_tst_remainder_first/controls.rs"]
mod controls;
#[path = "physical_clear_not_remainder/decode.rs"]
mod decode;
#[path = "physical_status_register_first/default_bus.rs"]
mod default_bus;
#[path = "physical_clear_not_remainder/device.rs"]
mod device;
#[path = "physical_status_register_first/model.rs"]
mod model;
#[path = "physical_neg_negx_remainder/value.rs"]
mod value;

pub fn run_first() -> (u32, u32, u32) {
    let (mut count, mut bad, mut unsupported) = (0, 0, 0);
    for case in decode::cases() {
        assert_eq!(case.label, format!("NN{count:05}"));
        let (good, absent) = check::run(case, false);
        count += 1;
        bad += u32::from(!good);
        unsupported += u32::from(absent);
    }
    (count, bad, unsupported)
}
#[test]
fn negate_register_and_byte_physical_first() {
    assert_eq!(run_first(), (13840, 0, 16));
}
