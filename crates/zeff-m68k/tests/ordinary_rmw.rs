use std::collections::BTreeMap;
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::physical::{PhysicalEntryState, PhysicalInspection};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu, TimedStep, TimingFault};
#[path = "ordinary_rmw/check.rs"]
mod check;
#[path = "ordinary_rmw/device.rs"]
mod device;
#[path = "ordinary_rmw/fixture.rs"]
mod fixture;
#[path = "ordinary_rmw/goldens.rs"]
mod goldens;

#[test]
fn first112_literal_ordinary_clear_not_staged_flags() {
    let (count, bad, sr_only) = goldens::run();
    eprintln!("ordinary RMW first cases={count} failures={bad} SR-only={sr_only}");
    assert_eq!((count, bad), (112, 0), "SR-only={sr_only}");
}

#[path = "ordinary_rmw/bulk_addressing.rs"]
mod bulk_addressing;
#[path = "ordinary_rmw/bulk_alu.rs"]
mod bulk_alu;
#[path = "ordinary_rmw/bulk_capacity.rs"]
mod bulk_capacity;
#[path = "ordinary_rmw/bulk_check.rs"]
mod bulk_check;
#[path = "ordinary_rmw/bulk_device.rs"]
mod bulk_device;
#[path = "ordinary_rmw/bulk_directed.rs"]
mod bulk_directed;
#[path = "ordinary_rmw/bulk_guards.rs"]
mod bulk_guards;
#[path = "ordinary_rmw/bulk_matrix.rs"]
mod bulk_matrix;
#[path = "ordinary_rmw/bulk_model.rs"]
mod bulk_model;
#[path = "ordinary_rmw/bulk_negative.rs"]
mod bulk_negative;
#[path = "ordinary_rmw/bulk_oracle.rs"]
mod bulk_oracle;
#[path = "ordinary_rmw/bulk_plan.rs"]
mod bulk_plan;
#[path = "ordinary_rmw/bulk_sections.rs"]
mod bulk_sections;

#[test]
fn section01_main_completion() {
    let (count, bad) = bulk_sections::section(1);
    eprintln!("ordinary RMW section=1 cases={count} failures={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[1], 0));
}

#[test]
fn section02_main_host_failure() {
    let (count, bad) = bulk_sections::section(2);
    eprintln!("ordinary RMW section=2 cases={count} failures={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[2], 0));
}

#[test]
fn section03_failed_write_no_deposit() {
    let (count, bad) = bulk_sections::section(3);
    eprintln!("ordinary RMW section=3 cases={count} failures={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[3], 0));
}

#[test]
fn section04_every_blocked_transfer() {
    let (count, bad) = bulk_sections::section(4);
    eprintln!("ordinary RMW section=4 cases={count} failures={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[4], 0));
}

#[test]
fn section05_all_index_selectors_aliases() {
    let (count, bad) = bulk_sections::section(5);
    eprintln!("ordinary RMW section=5 cases={count} failures={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[5], 0));
}

#[test]
fn section06_pc_ea_bus_wraps() {
    let (count, bad) = bulk_sections::section(6);
    eprintln!("ordinary RMW section=6 cases={count} failures={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[6], 0));
}

#[test]
fn section07_all_upper_sr_masks() {
    let (count, bad) = bulk_sections::section(7);
    eprintln!("ordinary RMW section=7 cases={count} failures={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[7], 0));
}

#[test]
fn section08_actual_callback_ram_mutations() {
    let (count, bad) = bulk_sections::section(8);
    eprintln!("ordinary RMW section=8 cases={count} failures={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[8], 0));
}

#[test]
fn section09_code_source_overlaps() {
    let (count, bad) = bulk_sections::section(9);
    eprintln!("ordinary RMW section=9 cases={count} failures={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[9], 0));
}

#[test]
fn section10_retained_n_independent_of_processing() {
    let (count, bad) = bulk_sections::section(10);
    eprintln!("ordinary RMW section=10 cases={count} failures={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[10], 0));
}

#[test]
fn section11_reserve_capacity_b_edges() {
    let (count, bad) = bulk_sections::section(11);
    eprintln!("ordinary RMW section=11 cases={count} failures={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[11], 0));
}

#[test]
fn section12_pre_index_i2_capacity() {
    let (count, bad) = bulk_sections::section(12);
    eprintln!("ordinary RMW section=12 cases={count} failures={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[12], 0));
}

#[test]
fn section13_maximum_waits_all_positions() {
    let (count, bad) = bulk_sections::section(13);
    eprintln!("ordinary RMW section=13 cases={count} failures={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[13], 0));
}

#[test]
fn section14_exact_max_final_commit() {
    let (count, bad) = bulk_sections::section(14);
    eprintln!("ordinary RMW section=14 cases={count} failures={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[14], 0));
}

#[test]
fn section15_ordinary_first_odd() {
    let (count, bad) = bulk_sections::section(15);
    eprintln!("ordinary RMW section=15 cases={count} failures={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[15], 0));
}

#[test]
fn section16_ordinary_preflight_guards() {
    let (count, bad) = bulk_sections::section(16);
    eprintln!("ordinary RMW section=16 cases={count} failures={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[16], 0));
}

#[test]
fn section17_byte_memory_unchanged() {
    let (count, bad) = bulk_sections::section(17);
    eprintln!("ordinary RMW section=17 cases={count} failures={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[17], 0));
}

#[test]
fn section18_dn_fault_publication() {
    let (count, bad) = bulk_sections::section(18);
    eprintln!("ordinary RMW section=18 cases={count} failures={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[18], 0));
}

#[test]
fn section19_other_unary_memory_unchanged() {
    let (count, bad) = bulk_sections::section(19);
    eprintln!("ordinary RMW section=19 cases={count} failures={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[19], 0));
}

#[test]
fn section20_other_unary_callback_host_unchanged() {
    let (count, bad) = bulk_sections::section(20);
    eprintln!("ordinary RMW section=20 cases={count} failures={bad}");
    assert_eq!((count, bad), (bulk_sections::COUNTS[20], 0));
}
