use std::collections::BTreeMap;
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::physical::{PhysicalEntryState, PhysicalInspection};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu, TimedStep, TimingFault};
#[path = "ordinary_negate/check.rs"]
mod check;
#[path = "ordinary_negate/device.rs"]
mod device;
#[path = "ordinary_negate/fixture.rs"]
mod fixture;
#[path = "ordinary_negate/goldens.rs"]
mod goldens;
#[path = "ordinary_negate/private.rs"]
mod private;

#[test]
fn first432_literal_ordinary_negate_staged_flags() {
    let (count, bad, sr_only) = goldens::run();
    eprintln!("ordinary NEG/NEGX first count={count} bad={bad} SR-only={sr_only}");
    assert_eq!((count, bad, sr_only), (432, 0, 0));
}

#[path = "ordinary_negate/bulk_addressing.rs"]
mod bulk_addressing;
#[path = "ordinary_negate/bulk_alu.rs"]
mod bulk_alu;
#[path = "ordinary_negate/bulk_capacity.rs"]
mod bulk_capacity;
#[path = "ordinary_negate/bulk_check.rs"]
mod bulk_check;
#[path = "ordinary_negate/bulk_device.rs"]
mod bulk_device;
#[path = "ordinary_negate/bulk_directed.rs"]
mod bulk_directed;
#[path = "ordinary_negate/bulk_guards.rs"]
mod bulk_guards;
#[path = "ordinary_negate/bulk_matrix.rs"]
mod bulk_matrix;
#[path = "ordinary_negate/bulk_model.rs"]
mod bulk_model;
#[path = "ordinary_negate/bulk_negative.rs"]
mod bulk_negative;
#[path = "ordinary_negate/bulk_oracle.rs"]
mod bulk_oracle;
#[path = "ordinary_negate/bulk_plan.rs"]
mod bulk_plan;
#[path = "ordinary_negate/bulk_private.rs"]
mod bulk_private;
#[path = "ordinary_negate/bulk_retry.rs"]
mod bulk_retry;
#[path = "ordinary_negate/bulk_sections.rs"]
mod bulk_sections;

#[test]
fn section01_main_completion() {
    let (count, bad) = bulk_sections::section(1);
    eprintln!("ordinary NEG/NEGX section=1 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[1], 0));
}

#[test]
fn section02_main_host_failure() {
    let (count, bad) = bulk_sections::section(2);
    eprintln!("ordinary NEG/NEGX section=2 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[2], 0));
}

#[test]
fn section03_failed_write_no_deposit() {
    let (count, bad) = bulk_sections::section(3);
    eprintln!("ordinary NEG/NEGX section=3 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[3], 0));
}

#[test]
fn section04_every_blocked_transfer() {
    let (count, bad) = bulk_sections::section(4);
    eprintln!("ordinary NEG/NEGX section=4 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[4], 0));
}

#[test]
fn section05_all_index_selectors_aliases() {
    let (count, bad) = bulk_sections::section(5);
    eprintln!("ordinary NEG/NEGX section=5 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[5], 0));
}

#[test]
fn section06_pc_ea_bus_wraps() {
    let (count, bad) = bulk_sections::section(6);
    eprintln!("ordinary NEG/NEGX section=6 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[6], 0));
}

#[test]
fn section07_all_upper_sr_masks() {
    let (count, bad) = bulk_sections::section(7);
    eprintln!("ordinary NEG/NEGX section=7 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[7], 0));
}

#[test]
fn section08_actual_callback_ram_mutations() {
    let (count, bad) = bulk_sections::section(8);
    eprintln!("ordinary NEG/NEGX section=8 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[8], 0));
}

#[test]
fn section09_code_source_overlaps() {
    let (count, bad) = bulk_sections::section(9);
    eprintln!("ordinary NEG/NEGX section=9 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[9], 0));
}

#[test]
fn section10_retained_n_independent_of_processing() {
    let (count, bad) = bulk_sections::section(10);
    eprintln!("ordinary NEG/NEGX section=10 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[10], 0));
}

#[test]
fn section11_reserve_capacity_b_edges() {
    let (count, bad) = bulk_sections::section(11);
    eprintln!("ordinary NEG/NEGX section=11 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[11], 0));
}

#[test]
fn section12_pre_index_i2_capacity() {
    let (count, bad) = bulk_sections::section(12);
    eprintln!("ordinary NEG/NEGX section=12 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[12], 0));
}

#[test]
fn section13_maximum_waits_all_positions() {
    let (count, bad) = bulk_sections::section(13);
    eprintln!("ordinary NEG/NEGX section=13 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[13], 0));
}

#[test]
fn section14_exact_max_final_commit() {
    let (count, bad) = bulk_sections::section(14);
    eprintln!("ordinary NEG/NEGX section=14 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[14], 0));
}

#[test]
fn section15_ordinary_first_odd() {
    let (count, bad) = bulk_sections::section(15);
    eprintln!("ordinary NEG/NEGX section=15 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[15], 0));
}

#[test]
fn section16_ordinary_preflight_guards() {
    let (count, bad) = bulk_sections::section(16);
    eprintln!("ordinary NEG/NEGX section=16 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[16], 0));
}

#[test]
fn section17_byte_memory_unchanged() {
    let (count, bad) = bulk_sections::section(17);
    eprintln!("ordinary NEG/NEGX section=17 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[17], 0));
}

#[test]
fn section18_dn_publication() {
    let (count, bad) = bulk_sections::section(18);
    eprintln!("ordinary NEG/NEGX section=18 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[18], 0));
}

#[test]
fn section19_other_unary_memory_unchanged() {
    let (count, bad) = bulk_sections::section(19);
    eprintln!("ordinary NEG/NEGX section=19 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[19], 0));
}

#[test]
fn section20_other_unary_callback_host_unchanged() {
    let (count, bad) = bulk_sections::section(20);
    eprintln!("ordinary NEG/NEGX section=20 count={count} bad={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[20], 0));
}
