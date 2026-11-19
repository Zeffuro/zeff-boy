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

#[path = "physical_from_sr/bulk_capacity.rs"]
mod bulk_capacity;
#[path = "physical_from_sr/bulk_device.rs"]
mod bulk_device;
#[path = "physical_from_sr/bulk_directed.rs"]
mod bulk_directed;
#[path = "physical_from_sr/bulk_entry.rs"]
mod bulk_entry;
#[path = "physical_from_sr/bulk_entry_oracle.rs"]
mod bulk_entry_oracle;
#[path = "physical_from_sr/bulk_entry_reset_capacity.rs"]
mod bulk_entry_reset_capacity;
#[path = "physical_from_sr/bulk_guards.rs"]
mod bulk_guards;
#[path = "physical_from_sr/bulk_matrix.rs"]
mod bulk_matrix;
#[path = "physical_from_sr/bulk_model.rs"]
mod bulk_model;
#[path = "physical_from_sr/bulk_oracle.rs"]
mod bulk_oracle;
#[path = "physical_from_sr/bulk_plan.rs"]
mod bulk_plan;
#[path = "physical_from_sr/bulk_private.rs"]
mod bulk_private;
#[path = "physical_from_sr/bulk_reset.rs"]
mod bulk_reset;
#[path = "physical_from_sr/bulk_sections.rs"]
mod bulk_sections;
#[test]
fn section_00_first_numeric_literals() {
    let (count, bad) = bulk_sections::section(0);
    eprintln!("physical MOVE-from-SR section=0 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[0], 0));
}
#[test]
fn section_01_processing_and_execution_masks() {
    let (count, bad) = bulk_sections::section(1);
    eprintln!("physical MOVE-from-SR section=1 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[1], 0));
}
#[test]
fn section_02_family_rejected_forms() {
    let (count, bad) = bulk_sections::section(2);
    eprintln!("physical MOVE-from-SR section=2 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[2], 0));
}
#[test]
fn section_03_preflight_guards() {
    let (count, bad) = bulk_sections::section(3);
    eprintln!("physical MOVE-from-SR section=3 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[3], 0));
}
#[test]
fn section_04_main_completion() {
    let (count, bad) = bulk_sections::section(4);
    eprintln!("physical MOVE-from-SR section=4 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[4], 0));
}
#[test]
fn section_05_main_consumed_faults() {
    let (count, bad) = bulk_sections::section(5);
    eprintln!("physical MOVE-from-SR section=5 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[5], 0));
}
#[test]
fn section_06_alternate_failed_write_effect() {
    let (count, bad) = bulk_sections::section(6);
    eprintln!("physical MOVE-from-SR section=6 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[6], 0));
}
#[test]
fn section_07_main_first_odd() {
    let (count, bad) = bulk_sections::section(7);
    eprintln!("physical MOVE-from-SR section=7 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[7], 0));
}
#[test]
fn section_08_raw_absent_a7_first_odd() {
    let (count, bad) = bulk_sections::section(8);
    eprintln!("physical MOVE-from-SR section=8 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[8], 0));
}
#[test]
fn section_09_index_selectors_and_aliases() {
    let (count, bad) = bulk_sections::section(9);
    eprintln!("physical MOVE-from-SR section=9 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[9], 0));
}
#[test]
fn section_10_pc_ea_bus_wraps() {
    let (count, bad) = bulk_sections::section(10);
    eprintln!("physical MOVE-from-SR section=10 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[10], 0));
}
#[test]
fn section_11_all_upper_sr_masks() {
    let (count, bad) = bulk_sections::section(11);
    eprintln!("physical MOVE-from-SR section=11 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[11], 0));
}
#[test]
fn section_12_actual_callback_ram_mutations() {
    let (count, bad) = bulk_sections::section(12);
    eprintln!("physical MOVE-from-SR section=12 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[12], 0));
}
#[test]
fn section_13_code_source_stack_overlaps() {
    let (count, bad) = bulk_sections::section(13);
    eprintln!("physical MOVE-from-SR section=13 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[13], 0));
}
#[test]
fn section_14_a7_source_stack_overlays() {
    let (count, bad) = bulk_sections::section(14);
    eprintln!("physical MOVE-from-SR section=14 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[14], 0));
}
#[test]
fn section_15_retained_n_processing_guards() {
    let (count, bad) = bulk_sections::section(15);
    eprintln!("physical MOVE-from-SR section=15 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[15], 0));
}
#[test]
fn section_16_every_reserve_clock_edge() {
    let (count, bad) = bulk_sections::section(16);
    eprintln!("physical MOVE-from-SR section=16 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[16], 0));
}
#[test]
fn section_17_pre_index_i2_capacity() {
    let (count, bad) = bulk_sections::section(17);
    eprintln!("physical MOVE-from-SR section=17 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[17], 0));
}
#[test]
fn section_18_short_complete_all_positions() {
    let (count, bad) = bulk_sections::section(18);
    eprintln!("physical MOVE-from-SR section=18 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[18], 0));
}
#[test]
fn section_19_maximum_elapsed_all_positions() {
    let (count, bad) = bulk_sections::section(19);
    eprintln!("physical MOVE-from-SR section=19 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[19], 0));
}
#[test]
fn section_20_exact_final_u64max() {
    let (count, bad) = bulk_sections::section(20);
    eprintln!("physical MOVE-from-SR section=20 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[20], 0));
}
#[test]
fn section_21_actual_default_instructionbus_detector() {
    let (count, bad) = bulk_sections::section(21);
    eprintln!("physical MOVE-from-SR section=21 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[21], 0));
}
#[test]
fn section_22_terminal_entry_all11() {
    let (count, bad) = bulk_sections::section(22);
    eprintln!("physical MOVE-from-SR section=22 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[22], 0));
}
#[test]
fn section_23_terminal_entry_success_controls() {
    let (count, bad) = bulk_sections::section(23);
    eprintln!("physical MOVE-from-SR section=23 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[23], 0));
}
#[test]
fn section_24_secondary_odd_actual_entrybus() {
    let (count, bad) = bulk_sections::section(24);
    eprintln!("physical MOVE-from-SR section=24 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[24], 0));
}
#[test]
fn section_25_terminal_warm_reset_all6() {
    let (count, bad) = bulk_sections::section(25);
    eprintln!("physical MOVE-from-SR section=25 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[25], 0));
}
#[test]
fn section_26_warm_reset_geometry_controls() {
    let (count, bad) = bulk_sections::section(26);
    eprintln!("physical MOVE-from-SR section=26 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[26], 0));
}
#[test]
fn section_27_reset_input_rejection() {
    let (count, bad) = bulk_sections::section(27);
    eprintln!("physical MOVE-from-SR section=27 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[27], 0));
}
#[test]
fn section_28_entry_reset_reserve_edges() {
    let (count, bad) = bulk_sections::section(28);
    eprintln!("physical MOVE-from-SR section=28 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[28], 0));
}
#[test]
fn section_29_entry_reset_internal_idle_edges() {
    let (count, bad) = bulk_sections::section(29);
    eprintln!("physical MOVE-from-SR section=29 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[29], 0));
}
