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
#[path = "physical_status_register/bulk_capacity.rs"]
mod bulk_capacity;
#[path = "physical_status_register/bulk_device.rs"]
mod bulk_device;
#[path = "physical_status_register/bulk_directed.rs"]
mod bulk_directed;
#[path = "physical_status_register/bulk_entry.rs"]
mod bulk_entry;
#[path = "physical_status_register/bulk_entry_oracle.rs"]
mod bulk_entry_oracle;
#[path = "physical_status_register/bulk_entry_reset_capacity.rs"]
mod bulk_entry_reset_capacity;
#[path = "physical_status_register/bulk_guards.rs"]
mod bulk_guards;
#[path = "physical_status_register/bulk_matrix.rs"]
mod bulk_matrix;
#[path = "physical_status_register/bulk_model.rs"]
mod bulk_model;
#[path = "physical_status_register/bulk_next.rs"]
mod bulk_next;
#[path = "physical_status_register/bulk_oracle.rs"]
mod bulk_oracle;
#[path = "physical_status_register/bulk_plan.rs"]
mod bulk_plan;
#[path = "physical_status_register/bulk_private.rs"]
mod bulk_private;
#[path = "physical_status_register/bulk_reset.rs"]
mod bulk_reset;
#[path = "physical_status_register/bulk_sections.rs"]
mod bulk_sections;
#[path = "physical_status_register/first.rs"]
mod first;

#[test]
fn section_00_first_numeric_literals() {
    let (count, bad) = bulk_sections::section(0);
    eprintln!("physical status register section=0 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[0], 0));
}

#[test]
fn section_01_processing_and_execution_masks() {
    let (count, bad) = bulk_sections::section(1);
    eprintln!("physical status register section=1 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[1], 0));
}

#[test]
fn section_02_illegal_status_encodings() {
    let (count, bad) = bulk_sections::section(2);
    eprintln!("physical status register section=2 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[2], 0));
}

#[test]
fn section_03_all_word_bank_preflight_guards() {
    let (count, bad) = bulk_sections::section(3);
    eprintln!("physical status register section=3 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[3], 0));
}

#[test]
fn section_04_main_completion() {
    let (count, bad) = bulk_sections::section(4);
    eprintln!("physical status register section=4 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[4], 0));
}

#[test]
fn section_05_all_encoded_consumed_faults() {
    let (count, bad) = bulk_sections::section(5);
    eprintln!("physical status register section=5 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[5], 0));
}

#[test]
fn section_06_every_source_word_mask() {
    let (count, bad) = bulk_sections::section(6);
    eprintln!("physical status register section=6 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[6], 0));
}

#[test]
fn section_07_dn_upper_junk_preservation() {
    let (count, bad) = bulk_sections::section(7);
    eprintln!("physical status register section=7 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[7], 0));
}

#[test]
fn section_08_all_old_upper_sr_masks() {
    let (count, bad) = bulk_sections::section(8);
    eprintln!("physical status register section=8 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[8], 0));
}

#[test]
fn section_09_logical_and_external_program_wraps() {
    let (count, bad) = bulk_sections::section(9);
    eprintln!("physical status register section=9 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[9], 0));
}

#[test]
fn section_10_actual_callback_ram_mutations() {
    let (count, bad) = bulk_sections::section(10);
    eprintln!("physical status register section=10 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[10], 0));
}

#[test]
fn section_11_code_source_stack_overlaps() {
    let (count, bad) = bulk_sections::section(11);
    eprintln!("physical status register section=11 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[11], 0));
}

#[test]
fn section_12_actual_r1_ird_and_processing() {
    let (count, bad) = bulk_sections::section(12);
    eprintln!("physical status register section=12 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[12], 0));
}

#[test]
fn section_13_new_s_t_i_stack_bank_switches() {
    let (count, bad) = bulk_sections::section(13);
    eprintln!("physical status register section=13 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[13], 0));
}

#[test]
fn section_14_every_transfer_reserve_edge() {
    let (count, bad) = bulk_sections::section(14);
    eprintln!("physical status register section=14 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[14], 0));
}

#[test]
fn section_15_both_i2_publication_capacity_edges() {
    let (count, bad) = bulk_sections::section(15);
    eprintln!("physical status register section=15 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[15], 0));
}

#[test]
fn section_16_short_complete_and_suppliedboundary() {
    let (count, bad) = bulk_sections::section(16);
    eprintln!("physical status register section=16 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[16], 0));
}

#[test]
fn section_17_maximum_elapsed_all_positions() {
    let (count, bad) = bulk_sections::section(17);
    eprintln!("physical status register section=17 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[17], 0));
}

#[test]
fn section_18_exact_final_u64max() {
    let (count, bad) = bulk_sections::section(18);
    eprintln!("physical status register section=18 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[18], 0));
}

#[test]
fn section_19_old_trace_and_user_sr_exclusions() {
    let (count, bad) = bulk_sections::section(19);
    eprintln!("physical status register section=19 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[19], 0));
}

#[test]
fn section_20_new_status_next_boundary_trace_irq() {
    let (count, bad) = bulk_sections::section(20);
    eprintln!("physical status register section=20 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[20], 0));
}

#[test]
fn section_21_guard_priority_and_retained36_inputs() {
    let (count, bad) = bulk_sections::section(21);
    eprintln!("physical status register section=21 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[21], 0));
}

#[test]
fn section_22_entry_all11_and_failed_write_policies() {
    let (count, bad) = bulk_sections::section(22);
    eprintln!("physical status register section=22 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[22], 0));
}

#[test]
fn section_23_entry_success_controls() {
    let (count, bad) = bulk_sections::section(23);
    eprintln!("physical status register section=23 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[23], 0));
}

#[test]
fn section_24_secondary_odd_actual_entrybus() {
    let (count, bad) = bulk_sections::section(24);
    eprintln!("physical status register section=24 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[24], 0));
}

#[test]
fn section_25_terminal_warm_reset_all6() {
    let (count, bad) = bulk_sections::section(25);
    eprintln!("physical status register section=25 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[25], 0));
}

#[test]
fn section_26_warm_reset_parity_and_actual_default_detector() {
    let (count, bad) = bulk_sections::section(26);
    eprintln!("physical status register section=26 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[26], 0));
}

#[test]
fn section_27_reset_input_rejection() {
    let (count, bad) = bulk_sections::section(27);
    eprintln!("physical status register section=27 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[27], 0));
}

#[test]
fn section_28_entry_reset_transfer_reserve_edges() {
    let (count, bad) = bulk_sections::section(28);
    eprintln!("physical status register section=28 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[28], 0));
}

#[test]
fn section_29_entry_reset_internal_idle_edges() {
    let (count, bad) = bulk_sections::section(29);
    eprintln!("physical status register section=29 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[29], 0));
}

#[test]
fn section_30_stack_handler_logical_and_bus_wraps() {
    let (count, bad) = bulk_sections::section(30);
    eprintln!("physical status register section=30 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[30], 0));
}
