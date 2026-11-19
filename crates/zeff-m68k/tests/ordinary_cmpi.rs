use std::collections::BTreeMap;
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::physical::{PhysicalEntryState, PhysicalInspection};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu, TimedStep, TimingFault};
#[path = "ordinary_cmpi/addressing.rs"]
mod addressing;
#[path = "ordinary_cmpi/arithmetic.rs"]
mod arithmetic;
#[path = "ordinary_cmpi/capacity.rs"]
mod capacity;
#[path = "ordinary_cmpi/check.rs"]
mod check;
#[path = "ordinary_cmpi/device.rs"]
mod device;
#[path = "ordinary_cmpi/directed.rs"]
mod directed;
#[path = "ordinary_cmpi/fixture.rs"]
mod fixture;
#[path = "ordinary_cmpi/goldens.rs"]
mod goldens;
#[path = "ordinary_cmpi/matrix.rs"]
mod matrix;
#[path = "ordinary_cmpi/model.rs"]
mod model;
#[path = "ordinary_cmpi/negative.rs"]
mod negative;
#[path = "ordinary_cmpi/oracle.rs"]
mod oracle;
#[path = "ordinary_cmpi/plan.rs"]
mod plan;
#[path = "ordinary_cmpi/remainder.rs"]
mod remainder;
#[path = "ordinary_cmpi/sections.rs"]
mod sections;
#[path = "ordinary_cmpi/special.rs"]
mod special;

#[test]
fn remaining_cmpi_first40() {
    assert_eq!(remainder::section(0), (40, 0));
}
#[test]
fn full_remaining_cmpi_sections() {
    for (index, count) in remainder::COUNTS.into_iter().enumerate() {
        let result = remainder::section(index as u32);
        eprintln!(
            "ordinary CMPI section={index} count={} bad={}",
            result.0, result.1
        );
        assert_eq!(result, (count, 0));
    }
}
pub fn remainder_section(index: u32) -> (u32, u32) {
    remainder::section(index)
}
#[test]
fn first32_literal_ordinary_cmpi_capacity_and_source_publication() {
    assert_eq!(goldens::run(), (32, 0));
    assert_eq!(sections::COUNTS.into_iter().sum::<u32>(), 766618);
    let _ = sections::run as fn() -> (u32, u32);
}
#[test]
fn completion() {
    assert_eq!(sections::section(1), (107520, 0));
}
#[test]
fn every_encoded_host_failure() {
    assert_eq!(sections::section(2), (239360, 0));
}
#[test]
fn every_encoded_reservation_failure() {
    assert_eq!(sections::section(3), (239360, 0));
}
#[test]
fn all_index_selectors_widths_bits_and_signs() {
    assert_eq!(sections::section(4), (131072, 0));
}
#[test]
fn actual_callback_ram_mutation() {
    assert_eq!(sections::section(5), (5984, 0));
}
#[test]
fn source_code_overlap() {
    assert_eq!(sections::section(6), (168, 0));
}
#[test]
fn program_wrap() {
    assert_eq!(sections::section(7), (168, 0));
}
#[test]
fn operand_wrap() {
    assert_eq!(sections::section(8), (156, 0));
}
#[test]
fn ordinary_odd_source_addresses() {
    assert_eq!(sections::section(9), (1344, 0));
}
#[test]
fn whole_status_and_trace_guard() {
    assert_eq!(sections::section(10), (8064, 0));
}
#[test]
fn maximum_wait_every_callback() {
    assert_eq!(sections::section(11), (23936, 0));
}
#[test]
fn exact_final_maximum_clock() {
    assert_eq!(sections::section(12), (2688, 0));
}
#[test]
fn source_idle_capacity() {
    assert_eq!(sections::section(13), (128, 0));
}
#[test]
fn byte_memory_and_all_data_register_widths() {
    assert_eq!(sections::section(14), (648, 0));
}
#[test]
fn other_immediate_families() {
    assert_eq!(sections::section(15), (5760, 0));
}
#[test]
fn all_illegal_cmpi_encodings() {
    assert_eq!(sections::section(16), (212, 0));
}
#[test]
fn ccr_special_repeated_fetch_schedule() {
    assert_eq!(sections::section(17), (18, 0));
}
