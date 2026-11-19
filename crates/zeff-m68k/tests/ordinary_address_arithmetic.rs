use std::collections::BTreeMap;
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu, TimedStep, TimingFault};

#[path = "ordinary_address_arithmetic/boundaries.rs"]
mod boundaries;
#[path = "ordinary_address_arithmetic/cases.rs"]
mod cases;
#[path = "ordinary_address_arithmetic/device.rs"]
mod device;
#[path = "ordinary_address_arithmetic/fixture.rs"]
mod fixture;
#[path = "ordinary_address_arithmetic/oracle.rs"]
mod oracle;
#[path = "ordinary_address_arithmetic/run.rs"]
mod run;
use device::Device;
use fixture::Case;
const PC: u32 = 0x2345_2000;
const START: u64 = 37;
const NEXT: u16 = 0x7201;
const FETCH: u16 = 0x7480;
const STATUS: [u16; 4] = [0, 0x0115, 0x041a, 0x070f];
fn put(words: &mut BTreeMap<u32, u16>, address: u32, value: u16) {
    words.insert(address & 0xffffff, value);
}
fn sample(words: &BTreeMap<u32, u16>, address: u32) -> u16 {
    words.get(&(address & 0xffffff)).copied().unwrap_or(0x4e71)
}
#[test]
fn golden_partial_a7_and_owned_tail_boundaries() {
    assert_eq!(cases::goldens(), (16, 0));
    assert_eq!(boundaries::goldens(), (32, 0));
}
#[test]
fn all_encodings_complete_with_banks_sign_zero_carry_status_and_waits() {
    assert_eq!(cases::completion(), (90112, 0));
}
#[test]
fn every_encoded_host_failure_preserves_consumed_source_or_partial_result_then_freezes() {
    assert_eq!(cases::faults(), (134144, 0));
}
#[test]
fn original_index_aliases_mutations_code_and_wrap_use_real_ram() {
    assert_eq!(cases::directed(), (66928, 0));
}
#[test]
fn reserves_source_updates_and_each_owned_tail_keep_exact_capacity_state() {
    assert_eq!(boundaries::witnesses(), (572, 0));
}
