use std::collections::BTreeMap;
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu, TimedStep, TimingFault};

#[path = "ordinary_data_arithmetic/boundaries.rs"]
mod boundaries;
#[path = "ordinary_data_arithmetic/device.rs"]
mod device;
#[path = "ordinary_data_arithmetic/directed.rs"]
mod directed;
#[path = "ordinary_data_arithmetic/fixture.rs"]
mod fixture;
#[path = "ordinary_data_arithmetic/goldens.rs"]
mod goldens;
#[path = "ordinary_data_arithmetic/matrix.rs"]
mod matrix;
#[path = "ordinary_data_arithmetic/oracle.rs"]
mod oracle;
#[path = "ordinary_data_arithmetic/run.rs"]
mod run;

const PC: u32 = 0x2345_2000;
const NEXT: u16 = 0x7201;
const FETCH: u16 = 0x7480;
const START: u64 = 37;
const WAIT: u32 = 3;

fn put(words: &mut BTreeMap<u32, u16>, address: u32, value: u16) {
    words.insert(address & 0xff_ffff, value);
}

fn sample(words: &BTreeMap<u32, u16>, address: u32) -> u16 {
    words.get(&(address & 0xff_ffff)).copied().unwrap_or(0x4e71)
}

#[test]
fn golden_word_flags_reserve_and_long_low_before_final_fetch() {
    assert_eq!(goldens::run(), (48, 0));
}
#[test]
fn all_encoded_completions_banks_twelve_pairs_status_and_waits() {
    assert_eq!(matrix::completion(), (540672, 0));
}
#[test]
fn every_encoded_host_failure_preserves_source_or_low_flags_then_freezes() {
    assert_eq!(matrix::faults(), (804864, 0));
}
#[test]
fn all_ccr_values_use_full_sized_flags_with_unchanged_nonccr() {
    assert_eq!(matrix::all_ccr(), (110592, 0));
}
#[test]
fn every_original_index_alias_width_sign_and_ignored_scale_pattern() {
    assert_eq!(directed::indices(), (131072, 0));
}
#[test]
fn actual_mutated_reads_change_source_extensions_queue_and_result() {
    assert_eq!(directed::mutation(), (944, 0));
}
#[test]
fn source_code_overlap_uses_actual_retained_and_fetched_words() {
    assert_eq!(directed::overlap(), (576, 0));
}
#[test]
fn program_logical_wrap_keeps_ordered_real_bus_reads() {
    assert_eq!(directed::program_wrap(), (576, 0));
}
#[test]
fn operand_logical_and_bus_wrap_preserve_both_long_halves() {
    assert_eq!(directed::operand_wrap(), (544, 0));
}
#[test]
fn callback_capacity_keeps_source_policy_and_blocks_flags_before_final_fetch() {
    assert_eq!(boundaries::capacity(), (472, 0));
}
#[test]
fn ordinary_source_idle_precedes_update_and_read_reservation() {
    assert_eq!(boundaries::source_capacity(), (96, 0));
}
#[test]
fn owned_long_tail_publishes_high_before_idle_and_word_commits_exact_max() {
    assert_eq!(boundaries::tails(), (288, 0));
}
#[test]
fn maximum_consumed_callbacks_preserve_each_publication_boundary() {
    assert_eq!(boundaries::consumed_maximum(), (944, 0));
}
#[test]
fn ordinary_odd_source_preserves_completed_update_without_group0_entry() {
    assert_eq!(boundaries::address_errors(), (144, 0));
}
