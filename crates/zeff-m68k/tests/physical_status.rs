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
#[path = "physical_status/bulk_capacity.rs"]
mod bulk_capacity;
#[path = "physical_status/bulk_entry.rs"]
mod bulk_entry;
#[path = "physical_status/bulk_reset.rs"]
mod bulk_reset;
#[path = "physical_status/capacity.rs"]
mod capacity;
#[path = "physical_status/check.rs"]
mod check;
#[path = "physical_status/device.rs"]
mod device;
#[path = "physical_status/directed.rs"]
mod directed;
#[path = "physical_status/entry_oracle.rs"]
mod entry_oracle;
#[path = "physical_status/fixture.rs"]
mod fixture;
#[path = "physical_status/goldens.rs"]
mod goldens;
#[path = "physical_status/guards.rs"]
mod guards;
#[path = "physical_status/matrix.rs"]
mod matrix;
#[path = "physical_status/model.rs"]
mod model;
#[path = "physical_status/oracle.rs"]
mod oracle;
use check::Path;
use device::{Device, Event};
use fixture::{Case, Transfer, access, registers, status};

#[test]
fn literal_first58_status_publication_and_actual_refetch() {
    assert_eq!(goldens::witnesses(), (58, 0));
}
#[test]
fn completion() {
    assert_eq!(matrix::completion(), (8650752, 0));
}
#[test]
fn encoded_fault() {
    assert_eq!(matrix::faults(), (1880064, 0));
}
#[test]
fn first_odd() {
    assert_eq!(matrix::odd(), (236544, 0));
}
#[test]
fn full_source_mask() {
    assert_eq!(matrix::mask(), (589824, 0));
}
#[test]
fn index() {
    assert_eq!(directed::indices(), (82944, 0));
}
#[test]
fn displacement() {
    assert_eq!(directed::displacements(), (324, 0));
}
#[test]
fn absolute_word_sign() {
    assert_eq!(directed::absolute_word(), (36, 0));
}
#[test]
fn absolute_long_halves() {
    assert_eq!(directed::absolute_long(), (144, 0));
}
#[test]
fn callback_sampling() {
    assert_eq!(directed::sampling(), (1377, 0));
}
#[test]
fn code_overlap() {
    assert_eq!(directed::overlap(), (243, 0));
}
#[test]
fn refetch_mutation() {
    assert_eq!(directed::mutations(), (324, 0));
}
#[test]
fn program_wrap() {
    assert_eq!(directed::program_wrap(), (162, 0));
}
#[test]
fn operand_wrap() {
    assert_eq!(directed::operand_wrap(), (153, 0));
}
#[test]
fn secondary_entry() {
    assert_eq!(bulk_entry::secondary(), (3564, 0));
}
#[test]
fn odd_secondary_ssp() {
    assert_eq!(bulk_entry::odd_ssp(), (540, 0));
}
#[test]
fn warm_reset() {
    assert_eq!(bulk_reset::witnesses(), (3132, 0));
}
#[test]
fn callback_reserve() {
    assert_eq!(bulk_capacity::reservations(), (459, 0));
}
#[test]
fn source_idle_capacity() {
    assert_eq!(bulk_capacity::source_idles(), (18, 0));
}
#[test]
fn postoperand_idle_capacity() {
    assert_eq!(bulk_capacity::post_idles(), (660, 0));
}
#[test]
fn short_complete() {
    assert_eq!(bulk_capacity::shorts(false), (1836, 0));
}
#[test]
fn short_boundary() {
    assert_eq!(bulk_capacity::shorts(true), (1377, 0));
}
#[test]
fn maximum_callback() {
    assert_eq!(bulk_capacity::maximum(), (1836, 0));
}
#[test]
fn final_exact_max() {
    assert_eq!(bulk_capacity::finals(), (132, 0));
}
#[test]
fn old_trace_guard() {
    assert_eq!(guards::old_trace(), (33792, 0));
}
#[test]
fn user_sr_gate() {
    assert_eq!(guards::user_sr(), (11264, 0));
}
#[test]
fn preflight_guards() {
    assert_eq!(guards::preflight(), (1188, 0));
}
#[test]
fn priority_guards() {
    assert_eq!(guards::priority(), (1056, 0));
}
#[test]
fn new_status_next_boundary() {
    assert_eq!(guards::next_boundary(), (12672, 0));
}
#[test]
fn processing_only_execution() {
    assert_eq!(guards::processing_only(), (36, 0));
}
#[test]
fn illegal_execution() {
    assert_eq!(guards::illegal(), (44, 0));
}
#[test]
fn other_status_execution_rejection() {
    assert_eq!(guards::other_status(), (256, 0));
}
#[test]
fn actual_r1_class() {
    assert_eq!(guards::actual_r1(), (648, 0));
}
#[test]
fn processing_classifier() {
    assert_eq!(guards::classifier(), (384, 0));
}
