use std::collections::BTreeMap;

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

type Memory = BTreeMap<u32, u16>;
#[path = "physical_move_extensions/device.rs"]
mod device;
#[path = "physical_data_arithmetic/entry.rs"]
mod entry;
#[path = "physical_data_arithmetic/fixture.rs"]
mod fixture;
#[path = "physical_data_arithmetic/goldens.rs"]
mod goldens;
#[path = "physical_data_arithmetic/oracle.rs"]
mod oracle;
#[path = "physical_data_arithmetic/run.rs"]
mod run;
use device::{DefaultDetection, Device, Event, Failure};
use fixture::*;
type BusDevice = Device<5>;
#[test]
fn golden_eight_operation_sizes_publish_low_and_flags_before_f() {
    assert_eq!(goldens::witnesses(), (48, 0));
}

#[path = "physical_data_arithmetic/detection.rs"]
mod detection;

#[path = "physical_data_arithmetic/matrix.rs"]
mod matrix;

#[test]
fn completion_all_encodings_banks_pairs_statuses_waits() {
    assert_eq!(matrix::completion(), (540672, 0));
}
#[test]
fn failures_all_encoded_callbacks_consumed_once() {
    assert_eq!(matrix::faults(true), (3219456, 0));
}
#[test]
fn odd_all_forms_banks_pairs_and_seven_detectors() {
    assert_eq!(matrix::odd(true), (48384, 0));
}
#[test]
fn every_ccr_on_all_forms_and_three_final_paths() {
    assert_eq!(matrix::all_ccr(), (165888, 0));
}

#[path = "physical_data_arithmetic/directed.rs"]
mod directed;

#[path = "physical_data_arithmetic/capacity.rs"]
mod capacity;

#[path = "physical_data_arithmetic/guards.rs"]
mod guards;

#[path = "physical_data_arithmetic/reset.rs"]
mod reset;

#[test]
fn indices() {
    assert_eq!(directed::indices(), (196608, 0));
}

#[test]
fn actual_read_mutation() {
    assert_eq!(directed::mutations(), (1416, 0));
}

#[test]
fn source_code_overlap() {
    assert_eq!(directed::overlaps(), (864, 0));
}

#[test]
fn program_wrap() {
    assert_eq!(directed::program_wrap(), (864, 0));
}

#[test]
fn operand_wrap() {
    assert_eq!(directed::operand_wrap(), (816, 0));
}

#[test]
fn secondary_entry() {
    assert_eq!(directed::secondary(), (4752, 0));
}

#[test]
fn odd_secondary_ssp() {
    assert_eq!(directed::odd_ssp(), (720, 0));
}

#[test]
fn callback_reserve() {
    assert_eq!(capacity::reservations(), (472, 0));
}

#[test]
fn source_idle_capacity() {
    assert_eq!(capacity::idles(), (96, 0));
}

#[test]
fn short_complete() {
    assert_eq!(capacity::shorts(false), (1888, 0));
}

#[test]
fn short_boundary() {
    assert_eq!(capacity::shorts(true), (1416, 0));
}

#[test]
fn maximum_consumed_callback() {
    assert_eq!(capacity::maximum(), (1888, 0));
}

#[test]
fn final_tail_capacity() {
    assert_eq!(capacity::tails(), (288, 0));
}

#[test]
fn trace_irq_guards() {
    assert_eq!(guards::preflight(3), (432, 0));
}

#[test]
fn unsupported_pure_read() {
    assert_eq!(guards::pure_read(), (5056, 0));
}

#[test]
fn unsupported_rmw() {
    assert_eq!(guards::rmw(), (8064, 0));
}

#[test]
fn pc_mismatch() {
    assert_eq!(guards::preflight(1), (144, 0));
}

#[test]
fn unprimed() {
    assert_eq!(guards::preflight(2), (144, 0));
}

#[test]
fn unknown_retained_n() {
    assert_eq!(guards::unknown(), (432, 0));
}

#[test]
fn processing_classifier() {
    assert_eq!(guards::classifier(), (12288, 0));
}

#[test]
fn warm_reset() {
    assert_eq!(reset::witnesses(), (928, 0));
}
