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
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu, TimingFault};

#[path = "physical_compare_ird/check.rs"]
mod check;
#[path = "physical_compare_ird/classifier.rs"]
mod classifier;
#[path = "physical_compare_ird/decode.rs"]
mod decode;
#[path = "physical_compare_ird/device.rs"]
mod device;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Event {
    tag: u8,
    clock: u64,
    access: BusAccess,
    value: u16,
}

struct Row {
    id: u64,
    opcode: u16,
    count: usize,
    final_fetch: bool,
    outcome: u8,
    initial: Registers,
    initial_queue: [u16; 2],
    initial_clock: u64,
    initial_memory: BTreeMap<u32, u16>,
    phase_registers: Registers,
    capture: Group0Capture,
    ssw: u16,
    instruction_clock: u64,
    expected: Registers,
    queue: Option<[u16; 2]>,
    clock: u64,
    entry_count: usize,
    memory: BTreeMap<u32, u16>,
    events: Vec<Event>,
}

pub fn section(section: u32) -> (u32, u32) {
    if section != 0 {
        return classifier::section(section);
    }
    let rows = decode::rows();
    let bad = rows.iter().filter(|row| !check::run(row)).count();
    (rows.len() as u32, bad as u32)
}

#[test]
fn first64_final_ird_frame_and_secondary_outcomes() {
    assert_eq!(section(0), (64, 0));
}

#[test]
fn added_word_f_classifies_all_1400_cmp_next_words() {
    assert_eq!(section(1), (1400, 0));
}

#[test]
fn added_word_f_classifies_all_976_cmpa_next_words() {
    assert_eq!(section(2), (976, 0));
}
