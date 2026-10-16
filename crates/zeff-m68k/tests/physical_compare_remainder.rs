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

#[path = "physical_compare_remainder/check.rs"]
mod check;
#[path = "physical_tst_remainder_first/controls.rs"]
mod controls;
#[path = "physical_compare_remainder/decode.rs"]
mod decode;
#[path = "physical_status_register_first/default_bus.rs"]
mod default_bus;
#[path = "physical_status_register_first/device.rs"]
mod device;
#[path = "physical_status_register_first/model.rs"]
mod model;
#[path = "physical_compare_remainder/reset.rs"]
mod reset;
#[path = "physical_compare_remainder/value.rs"]
mod value;

pub fn section(section: u32) -> (u32, u32, u32) {
    assert!(section < 3);
    let (mut count, mut bad, mut unsupported) = (0, 0, 0);
    for case in decode::cases() {
        if case.raw.get("section").number() != u64::from(section) {
            continue;
        }
        let (good, absent) = check::run(case, bad < 12);
        count += 1;
        bad += u32::from(!good);
        unsupported += u32::from(absent);
    }
    (count, bad, unsupported)
}

#[test]
fn all968_encoded_words_banks_and_byte_lanes() {
    assert_eq!(section(0), (2640, 0, 0));
}

#[test]
fn all23_literal_forms_and_46_consumed_callbacks() {
    assert_eq!(section(1), (414, 0, 0));
}

#[test]
fn all_reserve_tail_idle_capture_preflight_terminal_and_reset_rows() {
    assert_eq!(section(2), (664, 0, 0));
}
