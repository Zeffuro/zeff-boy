use super::{ALLOCATIONS, black_box};
use std::sync::atomic::Ordering;
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::physical::{
    CriticalFault, EntryBus, EntryFault, EntryKind, EntryOutcome, EntryReport, Group0Capture,
    PhysicalEntryState, PhysicalInspection, ProcessingClass, SswProfile,
};
use zeff_m68k::timing::physical_instruction::{
    InstructionBus, InstructionFault, InstructionOutcome, InstructionPhase, InstructionReport,
    PhysicalInstructionStep,
};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu, TimingFault};

mod check;
mod device;
mod model;

use check::run;
use device::{Device, Event, Ram};
use model::{Path, Report, Spec};

const BUDGET: u64 = u32::MAX as u64;
const SOURCES: [u16; 8] = [0, 31, 16, 0x2000, 0x8000, 0xa71f, 0x58e0, 0xffff];
const ROOMS: [u64; 7] = [0, 1, 2, 3, BUDGET, BUDGET + 1, BUDGET + 2];

pub(super) fn status_register_steps() {
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut main = 0;
    for _ in 0..5 {
        for context in 0..3 {
            for form in 0..9 {
                for ccr in 0..32 {
                    for mask in 0..8 {
                        for source in SOURCES {
                            run(Spec::new(context, form, ccr, mask, source), Path::Complete);
                            main += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(main, 276_480);
    let mut probes = 0;
    for _ in 0..100 {
        for context in 0..3 {
            for form in 0..9 {
                let spec = Spec::new(context, form, 31, 7, 0xa71f);
                run(spec, Path::Complete);
                probes += 1;
                for at in 0..spec.transfers() {
                    for report in [
                        Report::Host,
                        Report::Boundary,
                        Report::Unavailable,
                        Report::Short,
                    ] {
                        run(spec, Path::Report(at, report));
                        probes += 1;
                    }
                    run(spec, Path::Blocked(at));
                    probes += 1;
                }
                for idle in 0..2 {
                    for room in ROOMS {
                        run(spec, Path::Idle(idle, room));
                        probes += 1;
                    }
                }
            }
        }
    }
    assert_eq!(probes, 69_000);
    assert_eq!(main + probes, 345_480);
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed) - before, 0);
}
