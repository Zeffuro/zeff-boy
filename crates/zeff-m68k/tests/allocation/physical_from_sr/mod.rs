use super::{ALLOCATIONS, MemoryBus, black_box};
use std::sync::atomic::Ordering;
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode, M68kBus};
use zeff_m68k::timing::physical::{
    CriticalFault, EntryBus, EntryFault, EntryReport, PhysicalEntryState, PhysicalInspection,
};
use zeff_m68k::timing::physical_instruction::{
    InstructionBus, InstructionFault, InstructionOutcome, InstructionPhase, InstructionReport,
    PhysicalInstructionStep,
};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu, TimingFault};

const FIELDS: [u16; 8] = [0, 0x10, 0x18, 0x20, 0x28, 0x30, 0x38, 0x39];
const WORDS: [u32; 8] = [0, 1, 0x7fff, 0x8000, 0x8001, 0xffff, 0xfffe, 0x5555];
const BUDGET: u64 = u32::MAX as u64;

mod check;
mod device;
mod model;

use check::run;
use device::{Device, Event};
use model::{Action, Path, access, extensions};

pub(super) fn from_sr_steps(memory: &mut MemoryBus) {
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut main = 0;
    for _ in 0..5 {
        for supervisor in [false, true] {
            for ccr in 0..32 {
                for source in 0..8 {
                    for register in 0..if source < 6 { 8 } else { 1 } {
                        for value in WORDS {
                            run(
                                memory,
                                (source, register),
                                (supervisor, ccr, value),
                                Path::Complete,
                            );
                            main += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(main, 128_000);
    let mut probes = 0;
    for _ in 0..100 {
        for supervisor in [false, true] {
            for source in 0..8 {
                let count = extensions(source) + if source == 0 { 1 } else { 3 };
                for register in 0..if source < 6 { 8 } else { 1 } {
                    let form = (source, register);
                    let sample = (supervisor, 31, 0x7fff);
                    run(memory, form, sample, Path::Complete);
                    probes += 1;
                    for at in 0..count {
                        run(memory, form, sample, Path::Host(at));
                        run(memory, form, sample, Path::Blocked(at));
                        probes += 2;
                    }
                    if source == 0 {
                        for room in 0..=2 {
                            run(memory, form, sample, Path::Tail(room));
                            probes += 1;
                        }
                    }
                }
                if source != 0 {
                    run(memory, (source, 0), (supervisor, 31, 0x7fff), Path::Odd);
                    probes += 1;
                }
            }
        }
    }
    assert_eq!(probes, 77_400);
    assert_eq!(main + probes, 205_400);
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed) - before, 0);
}
