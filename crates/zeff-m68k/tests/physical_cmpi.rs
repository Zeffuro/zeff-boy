use zeff_m68k::bus::{AccessKind, BUS_ADDRESS_MASK, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::physical::{
    CriticalFault, EntryBus, EntryFault, EntryOutcome, EntryReport, Group0Capture,
    PhysicalEntryState, PhysicalInspection, ProcessingClass, SswProfile, TimingQualification,
};
use zeff_m68k::timing::physical_instruction::{
    InstructionBus, InstructionFault, InstructionOutcome, InstructionPhase, InstructionReport,
    PhysicalInstructionStep,
};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu, TimingFault};
use zeff_m68k::{Registers, SUPERVISOR, TRACE};
#[path = "physical_cmpi/capacity.rs"]
mod capacity;
#[path = "physical_cmpi/detection.rs"]
mod detection;
#[path = "physical_cmpi/device.rs"]
mod device;
#[path = "physical_cmpi/directed.rs"]
mod directed;
#[path = "physical_cmpi/entry.rs"]
mod entry;
#[path = "physical_cmpi/fixture.rs"]
mod fixture;
#[path = "physical_cmpi/remainder_full/mod.rs"]
pub mod full;
#[path = "physical_cmpi/goldens.rs"]
mod goldens;
#[path = "physical_cmpi/guards.rs"]
mod guards;
#[path = "physical_cmpi/matrix.rs"]
mod matrix;
#[path = "physical_cmpi/memory.rs"]
mod memory;
#[path = "physical_cmpi/oracle.rs"]
mod oracle;
#[path = "physical_cmpi/remainder.rs"]
mod remainder;
#[path = "physical_cmpi/remainder_controls.rs"]
mod remainder_controls;
#[path = "physical_cmpi/remainder_literals.rs"]
mod remainder_literals;
#[path = "physical_cmpi/reset.rs"]
mod reset;
#[path = "physical_cmpi/run.rs"]
mod run;
use device::{DefaultDetection, Device, Event, Failure};
use fixture::*;
use memory::Memory;
type BusDevice = Device<7>;
#[test]
fn literal_first44_immediate_extension_operand_and_final_contracts() {
    assert_eq!(goldens::witnesses(), (44, 0));
}
#[test]
fn completion() {
    assert_eq!(matrix::completion(), (16128, 0));
}
#[test]
fn encoded_fault() {
    assert_eq!(matrix::faults(), (143616, 0));
}
#[test]
fn first_odd() {
    assert_eq!(matrix::odd(), (56448, 0));
}
#[test]
fn all32ccr() {
    assert_eq!(matrix::all_ccr(), (32256, 0));
}
#[test]
fn index() {
    assert_eq!(directed::indices(), (49152, 0));
}
#[test]
fn mutation() {
    assert_eq!(directed::mutations(), (396, 0));
}
#[test]
fn code_overlap() {
    assert_eq!(directed::overlaps(), (252, 0));
}
#[test]
fn program_wrap() {
    assert_eq!(directed::program_wrap(), (168, 0));
}
#[test]
fn operand_wrap() {
    assert_eq!(directed::operand_wrap(), (156, 0));
}
#[test]
fn secondary_entry() {
    assert_eq!(directed::secondary(), (924, 0));
}
#[test]
fn odd_secondary_ssp() {
    assert_eq!(directed::odd_ssp(), (140, 0));
}
#[test]
fn warm_reset() {
    assert_eq!(reset::witnesses(), (232, 0));
}
#[test]
fn callback_reserve() {
    assert_eq!(capacity::reservations(), (132, 0));
}
#[test]
fn source_idle_capacity() {
    assert_eq!(capacity::idles(), (16, 0));
}
#[test]
fn short_complete() {
    assert_eq!(capacity::shorts(false), (528, 0));
}
#[test]
fn short_boundary() {
    assert_eq!(capacity::shorts(true), (396, 0));
}
#[test]
fn maximum_callback() {
    assert_eq!(capacity::maximum(), (528, 0));
}
#[test]
fn final_exact_max() {
    assert_eq!(capacity::finals(), (28, 0));
}
#[test]
fn trace_irq_guards() {
    assert_eq!(guards::preflight(3), (84, 0));
}
#[test]
fn pc_mismatch() {
    assert_eq!(guards::preflight(1), (28, 0));
}
#[test]
fn unprimed() {
    assert_eq!(guards::preflight(2), (28, 0));
}
#[test]
fn former_processing_only_inputs_now_execute() {
    assert_eq!(guards::processing_only(), (132, 0));
}
#[test]
fn remaining_cmpi_first350() {
    assert_eq!(remainder::first(), (350, 0));
}
#[test]
fn remaining_cmpi_capacity72() {
    assert_eq!(remainder::capacity(), (72, 0));
}
#[test]
fn remaining_cmpi_controls212() {
    assert_eq!(remainder_controls::controls(), (212, 0));
}
#[test]
fn remaining_cmpi_fixed_device_smoke() {
    assert_eq!(full::allocation_section(0, |_| {}), (216, 0));
    assert_eq!(full::section_with(5, |_| {}), (48, 0));
}
#[test]
fn full_remaining_cmpi_sections() {
    for (index, count) in full::COUNTS.into_iter().enumerate() {
        let result = full::section_with(index as u32, |_| {});
        eprintln!(
            "physical CMPI full section={index} count={} bad={}",
            result.0, result.1
        );
        assert_eq!(result, (count, 0));
    }
}
pub fn remainder_section(index: u32) -> (u32, u32) {
    match index {
        0 => remainder::first(),
        1 => remainder::capacity(),
        2 => remainder_controls::controls(),
        _ => panic!("CMPI remainder section"),
    }
}
#[test]
fn illegal_cmpi() {
    assert_eq!(guards::illegal(), (212, 0));
}
#[test]
fn retained_class() {
    assert_eq!(guards::retained(), (168, 0));
}
#[test]
fn processing_classifier() {
    assert_eq!(guards::classifier(), (512, 0));
}
