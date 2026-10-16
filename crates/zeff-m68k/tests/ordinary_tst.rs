use std::collections::BTreeMap;
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BUS_ADDRESS_MASK, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu, TimedStep, TimingFault};

#[path = "ordinary_tst/check.rs"]
mod check;
#[path = "ordinary_tst/device.rs"]
mod device;
#[path = "ordinary_tst/model.rs"]
mod model;

pub const COUNTS: [u32; 7] = [98304, 244736, 7648, 3072, 1024, 3072, 960];

pub fn section(index: u32) -> (u32, u32) {
    let result = check::section(index);
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!(
        "ordinary TST section={index} count={} bad={}",
        result.0, result.1
    );
    result
}

#[test]
fn completions() {
    assert_eq!(section(0), (COUNTS[0], 0));
}
#[test]
fn consumed_failures() {
    assert_eq!(section(1), (COUNTS[1], 0));
}
#[test]
fn capacity_before_publication() {
    assert_eq!(section(2), (COUNTS[2], 0));
}
#[test]
fn exact_maximum_completion() {
    assert_eq!(section(3), (COUNTS[3], 0));
}
#[test]
fn idle_capacity_preserves_prefix() {
    assert_eq!(section(4), (COUNTS[4], 0));
}
#[test]
fn final_fetch_uses_ram_and_preserves_latched_source() {
    assert_eq!(section(5), (COUNTS[5], 0));
}

#[test]
fn register_flags_use_the_complete_operand_size() {
    assert_eq!(section(6), (COUNTS[6], 0));
}
