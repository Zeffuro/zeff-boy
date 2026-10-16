use crate::device::{Device, Event, ReportKind, Script};
use crate::fixed::Fixed;
use crate::oracle::{self, Image};
use crate::preflight_engine::{access, data, idle, image, program};
use crate::preflight_state::{Cpu, ResultValue};
use crate::seed;
use zeff_m68k::bus::{AccessKind, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::physical::{
    CriticalFault, EntryFault, PhysicalEntryState, PhysicalInspection, TimingQualification,
};
use zeff_m68k::timing::physical_instruction::{InstructionFault, InstructionPhase};

pub fn physical_first(
    cpu: &mut Cpu,
    device: &mut Device,
    images: &mut Fixed<Image>,
    source: &oracle::Expected,
) -> ResultValue {
    let (mode, reg, bits) = (
        source.initial.mode,
        source.initial.register,
        source.initial.bits,
    );
    if matches!(mode, 4 | 6) {
        image(images, "SetupPhysicalI2", cpu);
        idle(cpu, device, "SetupPhysicalI2", 2, false);
    }
    let (phase, a) = if mode == 0 {
        let mask = if bits == 32 {
            u32::MAX
        } else {
            (1 << bits) - 1
        };
        let old = cpu.r.d[reg];
        let value = if source.initial.axis.word & 0xff00 == 0x4200 {
            0
        } else {
            !old & mask
        };
        cpu.r
            .set_status_register(oracle::flags(cpu.r.status_register(), bits, value));
        let published = mask.min(65535);
        cpu.r.d[reg] = old & !published | value & published;
        (
            InstructionPhase::OperandRefill,
            access(
                cpu.r.pc.wrapping_add(4),
                program(&cpu.r),
                AccessKind::Prefetch,
                ByteLanes::Both,
            ),
        )
    } else if mode >= 5 {
        (
            InstructionPhase::ExtensionFirst,
            access(
                cpu.r.pc.wrapping_add(4),
                program(&cpu.r),
                AccessKind::Prefetch,
                ByteLanes::Both,
            ),
        )
    } else {
        let address = source.initial.initial_ea.unwrap();
        if matches!(mode, 3 | 4) {
            let old = seed::active(&cpu.r, reg);
            let step = if reg == 7 { 2 } else { 1 };
            seed::set_active(
                &mut cpu.r,
                reg,
                if mode == 3 {
                    old.wrapping_add(step)
                } else {
                    old.wrapping_sub(step)
                },
            );
        }
        (
            InstructionPhase::OperandReadHigh,
            access(
                address,
                data(&cpu.r),
                AccessKind::Data,
                if address & 1 == 0 {
                    ByteLanes::Upper
                } else {
                    ByteLanes::Lower
                },
            ),
        )
    };
    image(images, "SetupPhysical", cpu);
    device.events.push(Event::Physical {
        phase: "SetupPhysical",
        access: a,
        start: cpu.clock,
        elapsed: 5,
        value: device.words.sample(a.bus_address),
        outcome: ReportKind::TimingUnavailable,
        instruction: true,
        index: device.transfers,
    });
    device.scripts.push(Script::Physical {
        phase: "SetupPhysical",
        access: a,
        elapsed: 5,
        outcome: ReportKind::TimingUnavailable,
        instruction: true,
    });
    cpu.clock += 5;
    device.clock = cpu.clock;
    device.transfers += 1;
    let cause = CriticalFault::BusAbort {
        access: a,
        direction: Direction::Read,
    };
    cpu.queue = None;
    cpu.pipeline_address = None;
    cpu.physical = PhysicalEntryState::FaultUnqualified { cause };
    cpu.inspection = PhysicalInspection::Unavailable;
    ResultValue::Physical(Err(InstructionFault::TransitionTimingUnavailable {
        phase,
        cause,
    }))
}

pub fn reset_first(cpu: &mut Cpu, device: &mut Device, images: &mut Fixed<Image>) -> ResultValue {
    cpu.queue = None;
    cpu.pipeline_address = None;
    cpu.trace = false;
    cpu.stopped = false;
    cpu.imported = None;
    cpu.physical =
        PhysicalEntryState::Critical(zeff_m68k::timing::physical::EntryKind::ExternalWarmReset);
    image(images, "ResetI14", cpu);
    idle(cpu, device, "ResetI14", 14, false);
    image(images, "ResetSPHigh", cpu);
    let a = access(
        0,
        FunctionCode::SupervisorProgram,
        AccessKind::ResetVector,
        ByteLanes::Both,
    );
    device.events.push(Event::Physical {
        phase: "ResetSPHigh",
        access: a,
        start: cpu.clock,
        elapsed: 8,
        value: device.words.sample(0),
        outcome: ReportKind::CriticalAbortReleased,
        instruction: false,
        index: 0,
    });
    device.scripts.push(Script::Physical {
        phase: "ResetSPHigh",
        access: a,
        elapsed: 8,
        outcome: ReportKind::CriticalAbortReleased,
        instruction: false,
    });
    cpu.clock += 8;
    device.clock = cpu.clock;
    device.transfers += 1;
    device.reset_transfers = 1;
    let cause = CriticalFault::BusAbort {
        access: a,
        direction: Direction::Read,
    };
    cpu.physical = PhysicalEntryState::DoubleFaultHalted {
        cause,
        timing: TimingQualification::Qualified,
    };
    cpu.inspection = PhysicalInspection::Unavailable;
    ResultValue::Reset(Err(EntryFault::DoubleFaultHalted(cause)))
}
