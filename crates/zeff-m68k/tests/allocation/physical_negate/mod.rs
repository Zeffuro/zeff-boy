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

const FIELDS: [u16; 7] = [0x10, 0x18, 0x20, 0x28, 0x30, 0x38, 0x39];
const WORDS: [u32; 8] = [0, 1, 0x7fff, 0x8000, 0x8001, 0xffff, 0xfffe, 0x5555];
const LONGS: [u32; 8] = [
    0,
    1,
    0x8000,
    0x8000_0000,
    0x8000_0001,
    0xffff_ffff,
    0xffff_0000,
    0x5555_5555,
];
const BUDGET: u64 = u32::MAX as u64;

mod check;
mod device;
mod model;

use check::run;
use device::{Device, Event};
use model::{Action, Path, access, extensions, stages};

pub(super) fn negate_steps(memory: &mut MemoryBus) {
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut main = 0;
    for _ in 0..5 {
        for supervisor in [false, true] {
            for ccr in 0..32 {
                for family in [0x4000, 0x4400] {
                    for long in [false, true] {
                        for source in 0..7 {
                            for register in 0..if source < 5 { 8 } else { 1 } {
                                for value in if long { LONGS } else { WORDS } {
                                    run(
                                        memory,
                                        (family, long, source, register),
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
        }
    }
    assert_eq!(main, 430_080);
    let mut probes = 0;
    for _ in 0..100 {
        for supervisor in [false, true] {
            for family in [0x4000, 0x4400] {
                for long in [false, true] {
                    let value = if long { 0x8000 } else { 0x7fff };
                    for source in 0..7 {
                        let count = extensions(source) + if long { 5 } else { 3 };
                        for register in 0..if source < 5 { 8 } else { 1 } {
                            let form = (family, long, source, register);
                            let sample = (supervisor, 31, value);
                            run(memory, form, sample, Path::Complete);
                            probes += 1;
                            for at in 0..count {
                                run(memory, form, sample, Path::Host(at));
                                run(memory, form, sample, Path::Blocked(at));
                                probes += 2;
                            }
                        }
                        run(
                            memory,
                            (family, long, source, 0),
                            (supervisor, 31, value),
                            Path::Odd,
                        );
                        probes += 1;
                    }
                }
            }
        }
    }
    assert_eq!(probes, 338_400);
    assert_eq!(main + probes, 768_480);
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed) - before, 0);
}
