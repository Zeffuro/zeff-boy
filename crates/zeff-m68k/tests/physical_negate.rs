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
#[path = "physical_negate/check.rs"]
mod check;
#[path = "physical_negate/device.rs"]
mod device;
#[path = "physical_negate/goldens.rs"]
mod goldens;
#[path = "physical_negate/memory.rs"]
mod memory;
#[path = "physical_negate/model.rs"]
mod model;
fn run_first() -> (u32, u32, u32) {
    goldens::run()
}
#[test]
fn literal_first536_physical_neg_negx_publication_and_writes() {
    let (count, bad, unsupported) = run_first();
    eprintln!("physical NEG/NEGX first count={count} bad={bad} unsupported={unsupported}");
    assert_eq!((count, bad, unsupported), (536, 0, 0));
}

#[path = "physical_negate/bulk_alu.rs"]
mod bulk_alu;
#[path = "physical_negate/bulk_capacity.rs"]
mod bulk_capacity;
#[path = "physical_negate/bulk_device.rs"]
mod bulk_device;
#[path = "physical_negate/bulk_directed.rs"]
mod bulk_directed;
#[path = "physical_negate/bulk_entry.rs"]
mod bulk_entry;
#[path = "physical_negate/bulk_entry_oracle.rs"]
mod bulk_entry_oracle;
#[path = "physical_negate/bulk_entry_reset_capacity.rs"]
mod bulk_entry_reset_capacity;
#[path = "physical_negate/bulk_guards.rs"]
mod bulk_guards;
#[path = "physical_negate/bulk_matrix.rs"]
mod bulk_matrix;
#[path = "physical_negate/bulk_model.rs"]
mod bulk_model;
#[path = "physical_negate/bulk_oracle.rs"]
mod bulk_oracle;
#[path = "physical_negate/bulk_plan.rs"]
mod bulk_plan;
#[path = "physical_negate/bulk_private.rs"]
mod bulk_private;
#[path = "physical_negate/bulk_reset.rs"]
mod bulk_reset;
#[path = "physical_negate/bulk_sections.rs"]
mod bulk_sections;

#[test]
fn processing_and_execution_masks() {
    assert_eq!(bulk_sections::section(1), (bulk_sections::COUNTS[1], 0));
}

#[test]
fn family_rejected_forms() {
    assert_eq!(bulk_sections::section(2), (bulk_sections::COUNTS[2], 0));
}

#[test]
fn preflight_guards() {
    assert_eq!(bulk_sections::section(3), (bulk_sections::COUNTS[3], 0));
}

#[test]
fn main_completion() {
    assert_eq!(bulk_sections::section(4), (bulk_sections::COUNTS[4], 0));
}

#[test]
fn main_consumed_faults() {
    assert_eq!(bulk_sections::section(5), (bulk_sections::COUNTS[5], 0));
}

#[test]
fn alternate_failed_write_effect() {
    assert_eq!(bulk_sections::section(6), (bulk_sections::COUNTS[6], 0));
}

#[test]
fn main_first_odd() {
    assert_eq!(bulk_sections::section(7), (bulk_sections::COUNTS[7], 0));
}

#[test]
fn raw_absent_a7_first_odd() {
    assert_eq!(bulk_sections::section(8), (bulk_sections::COUNTS[8], 0));
}

#[test]
fn index_selectors_and_aliases() {
    assert_eq!(bulk_sections::section(9), (bulk_sections::COUNTS[9], 0));
}

#[test]
fn pc_ea_bus_wraps() {
    assert_eq!(bulk_sections::section(10), (bulk_sections::COUNTS[10], 0));
}

#[test]
fn all_upper_sr_masks() {
    assert_eq!(bulk_sections::section(11), (bulk_sections::COUNTS[11], 0));
}

#[test]
fn actual_callback_ram_mutations() {
    assert_eq!(bulk_sections::section(12), (bulk_sections::COUNTS[12], 0));
}

#[test]
fn code_source_stack_overlaps() {
    assert_eq!(bulk_sections::section(13), (bulk_sections::COUNTS[13], 0));
}

#[test]
fn a7_source_stack_overlays() {
    assert_eq!(bulk_sections::section(14), (bulk_sections::COUNTS[14], 0));
}

#[test]
fn retained_n_processing_guards() {
    assert_eq!(bulk_sections::section(15), (bulk_sections::COUNTS[15], 0));
}

#[test]
fn every_reserve_clock_edge() {
    assert_eq!(bulk_sections::section(16), (bulk_sections::COUNTS[16], 0));
}

#[test]
fn pre_index_i2_capacity() {
    assert_eq!(bulk_sections::section(17), (bulk_sections::COUNTS[17], 0));
}

#[test]
fn short_complete_all_positions() {
    assert_eq!(bulk_sections::section(18), (bulk_sections::COUNTS[18], 0));
}

#[test]
fn maximum_elapsed_all_positions() {
    assert_eq!(bulk_sections::section(19), (bulk_sections::COUNTS[19], 0));
}

#[test]
fn exact_final_u64max() {
    assert_eq!(bulk_sections::section(20), (bulk_sections::COUNTS[20], 0));
}

#[test]
fn actual_default_instructionbus_detector() {
    assert_eq!(bulk_sections::section(21), (bulk_sections::COUNTS[21], 0));
}

#[test]
fn terminal_entry_all11() {
    assert_eq!(bulk_sections::section(22), (bulk_sections::COUNTS[22], 0));
}

#[test]
fn terminal_entry_success_controls() {
    assert_eq!(bulk_sections::section(23), (bulk_sections::COUNTS[23], 0));
}

#[test]
fn secondary_odd_actual_entrybus() {
    assert_eq!(bulk_sections::section(24), (bulk_sections::COUNTS[24], 0));
}

#[test]
fn terminal_warm_reset_all6() {
    assert_eq!(bulk_sections::section(25), (bulk_sections::COUNTS[25], 0));
}

#[test]
fn warm_reset_geometry_controls() {
    assert_eq!(bulk_sections::section(26), (bulk_sections::COUNTS[26], 0));
}

#[test]
fn reset_input_rejection() {
    assert_eq!(bulk_sections::section(27), (bulk_sections::COUNTS[27], 0));
}

#[test]
fn entry_reset_reserve_edges() {
    assert_eq!(bulk_sections::section(28), (bulk_sections::COUNTS[28], 0));
}

#[test]
fn entry_reset_internal_idle_edges() {
    assert_eq!(bulk_sections::section(29), (bulk_sections::COUNTS[29], 0));
}
