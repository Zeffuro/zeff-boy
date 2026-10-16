use crate::device::{Device, Event, Script};
use crate::fixed::Fixed;
use crate::literal::Value;
use crate::oracle::{self, Image};
use crate::preflight_axes::{Api, PreflightAxis, State};
use crate::preflight_state::{Constructor, Cpu, Expected, Record, ResultValue};
use crate::seed;
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BUS_ADDRESS_MASK, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::physical::{
    Group0Capture, ImportedGroup0Boundary, PhysicalEntryState, ProcessingClass, SswProfile,
};
use zeff_m68k::timing::{CpuClocks, TimedOutcome, TimedStep, TimingFault};

pub fn access(address: u32, fc: FunctionCode, kind: AccessKind, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & BUS_ADDRESS_MASK & !1,
        function_code: fc,
        kind,
        lanes,
    }
}
pub(super) fn program(r: &Registers) -> FunctionCode {
    if r.status_register() & 0x2000 != 0 {
        FunctionCode::SupervisorProgram
    } else {
        FunctionCode::UserProgram
    }
}
pub(super) fn data(r: &Registers) -> FunctionCode {
    if r.status_register() & 0x2000 != 0 {
        FunctionCode::SupervisorData
    } else {
        FunctionCode::UserData
    }
}

fn record(
    records: &mut Vec<Record>,
    method: &'static str,
    arguments: Value,
    result: ResultValue,
    cpu: &Cpu,
    device: &Device,
) {
    records.push(Record {
        method,
        arguments,
        result,
        cpu: cpu.clone(),
        device: device.clone(),
    });
}
pub(super) fn image(images: &mut Fixed<Image>, phase: &'static str, cpu: &Cpu) {
    images.push(Image {
        phase,
        registers: cpu.r,
    });
}

pub(super) fn idle(
    cpu: &mut Cpu,
    device: &mut Device,
    phase: &'static str,
    clocks: u32,
    probe: bool,
) {
    device.events.push(Event::Idle {
        phase,
        start: cpu.clock,
        clocks,
    });
    cpu.clock += u64::from(clocks);
    device.clock = cpu.clock;
    if probe {
        device.probe_idles += 1;
    }
}

fn cycle(
    cpu: &mut Cpu,
    device: &mut Device,
    phase: &'static str,
    a: BusAccess,
    payload: Option<u16>,
    probe: bool,
) -> u16 {
    let sampled = device.words.sample(a.bus_address);
    let direction = if payload.is_some() {
        Direction::Write
    } else {
        Direction::Read
    };
    let index = device.transfers;
    if let Some(value) = payload {
        device.words.insert(a.bus_address, value);
    }
    device.events.push(Event::Transfer {
        phase,
        access: a,
        direction,
        value: payload.unwrap_or(sampled),
        start: cpu.clock,
        wait: 0,
        elapsed: 4,
        failed: false,
        deposited: payload.is_some(),
        index,
    });
    device.scripts.push(Script::Ordinary {
        phase,
        access: a,
        direction,
        wait: 0,
        result: Ok(payload.is_none().then_some(sampled)),
    });
    device.transfers += 1;
    cpu.clock += 4;
    device.clock = cpu.clock;
    if probe {
        device.probe_transfers += 1;
    }
    sampled
}

fn imported(source: &oracle::Expected, word: u16) -> ImportedGroup0Boundary {
    let r = source.initial.r;
    let logical = source.initial.initial_ea.unwrap_or(r.pc.wrapping_add(4));
    ImportedGroup0Boundary {
        registers: r,
        clock: CpuClocks(source.initial.device.clock),
        capture: Group0Capture {
            vector: 2,
            internal_pc: r.pc.wrapping_add(4),
            phase_sr: r.status_register(),
            ird: word,
            logical_address: logical,
            function_code: if source.initial.mode == 0 {
                program(&r)
            } else {
                data(&r)
            },
            direction: Direction::Read,
            processing_class: ProcessingClass::InstructionOrGroup2,
            ssw_profile: SswProfile::SelectedIrdUpperBits,
        },
    }
}

fn fault(api: Api, error: TimingFault<usize>) -> ResultValue {
    match api {
        Api::Instruction | Api::Step => ResultValue::Step(Err(error)),
        Api::Boundary => ResultValue::Boundary(Err(error)),
        Api::Prime => ResultValue::Prime(Err(error)),
        Api::Trace => ResultValue::Trace(Err(error)),
    }
}

fn trace(cpu: &mut Cpu, device: &mut Device, api: Api) -> ResultValue {
    let (pc, sp, sr, start) = (cpu.r.pc, cpu.r.ssp, cpu.r.status_register(), cpu.clock);
    idle(cpu, device, "TraceI4", 4, true);
    if sp & 1 != 0 {
        cpu.frozen = true;
        return fault(
            api,
            TimingFault::AddressError {
                address: sp.wrapping_sub(2),
            },
        );
    }
    for (phase, address, value) in [
        ("TR_PCL", sp.wrapping_sub(2), pc as u16),
        ("TR_SR", sp.wrapping_sub(6), sr),
        ("TR_PCH", sp.wrapping_sub(4), (pc >> 16) as u16),
    ] {
        cycle(
            cpu,
            device,
            phase,
            access(
                address,
                FunctionCode::SupervisorData,
                AccessKind::Stack,
                ByteLanes::Both,
            ),
            Some(value),
            true,
        );
    }
    let high = cycle(
        cpu,
        device,
        "TR_VH",
        access(
            36,
            FunctionCode::SupervisorData,
            AccessKind::Vector,
            ByteLanes::Both,
        ),
        None,
        true,
    );
    let low = cycle(
        cpu,
        device,
        "TR_VL",
        access(
            38,
            FunctionCode::SupervisorData,
            AccessKind::Vector,
            ByteLanes::Both,
        ),
        None,
        true,
    );
    let target = u32::from(high) << 16 | u32::from(low);
    let first = cycle(
        cpu,
        device,
        "TR_H0",
        access(
            target,
            FunctionCode::SupervisorProgram,
            AccessKind::Prefetch,
            ByteLanes::Both,
        ),
        None,
        true,
    );
    idle(cpu, device, "TraceI2", 2, true);
    let second = cycle(
        cpu,
        device,
        "TR_H1",
        access(
            target.wrapping_add(2),
            FunctionCode::SupervisorProgram,
            AccessKind::Prefetch,
            ByteLanes::Both,
        ),
        None,
        true,
    );
    cpu.r.ssp = sp.wrapping_sub(6);
    cpu.r.pc = target;
    cpu.r.set_status_register((sr | 0x2000) & !0x8000);
    cpu.queue = Some([first, second]);
    cpu.pipeline_address = Some(target);
    cpu.stopped = false;
    cpu.trace = false;
    if api == Api::Trace {
        ResultValue::Trace(Ok(Some(cpu.clock - start)))
    } else {
        ResultValue::Boundary(Ok(TimedOutcome::Trace {
            clocks: cpu.clock - start,
        }))
    }
}

fn probe(cpu: &mut Cpu, device: &mut Device, api: Api) -> (ResultValue, Option<&'static str>) {
    let guard = if cpu.frozen {
        Some(("Frozen", TimingFault::Frozen))
    } else if matches!(cpu.physical, PhysicalEntryState::DoubleFaultHalted { .. }) {
        Some(("DoubleFaultHalted", TimingFault::DoubleFaultHalted))
    } else if matches!(cpu.physical, PhysicalEntryState::FaultUnqualified { .. }) {
        Some((
            "PhysicalFaultUnqualified",
            TimingFault::PhysicalFaultUnqualified,
        ))
    } else if cpu.imported.is_some() {
        Some((
            "ImportedBoundaryPending",
            TimingFault::ImportedBoundaryPending,
        ))
    } else {
        None
    };
    if let Some((name, error)) = guard {
        return (fault(api, error), Some(name));
    }
    let result = if api == Api::Prime {
        if cpu.stopped {
            fault(api, TimingFault::Stopped)
        } else {
            let pc = cpu.r.pc;
            let fc = program(&cpu.r);
            let first = cycle(
                cpu,
                device,
                "ProbeFetch0",
                access(pc, fc, AccessKind::Prefetch, ByteLanes::Both),
                None,
                true,
            );
            let second = cycle(
                cpu,
                device,
                "ProbeFetch1",
                access(
                    pc.wrapping_add(2),
                    fc,
                    AccessKind::Prefetch,
                    ByteLanes::Both,
                ),
                None,
                true,
            );
            cpu.queue = Some([first, second]);
            cpu.pipeline_address = Some(pc);
            ResultValue::Prime(Ok(()))
        }
    } else if matches!(api, Api::Trace | Api::Boundary) && cpu.trace {
        trace(cpu, device, api)
    } else if api == Api::Trace {
        ResultValue::Trace(Ok(None))
    } else if cpu.trace {
        fault(api, TimingFault::PendingTrace)
    } else if cpu.stopped {
        if api == Api::Boundary {
            ResultValue::Boundary(Ok(TimedOutcome::Stopped))
        } else {
            fault(api, TimingFault::Stopped)
        }
    } else {
        assert!(cpu.pipeline_address != Some(cpu.r.pc));
        fault(api, TimingFault::Unprimed)
    };
    (result, None)
}

pub fn predict(axis: PreflightAxis) -> Expected {
    let source = oracle::predict(seed::build(&axis.primary));
    let mut cpu = Cpu::primary(&source, true);
    let mut device = source.initial.device.clone();
    device.configure(axis.state);
    let mut constructor = Constructor::Prefetched {
        r: cpu.r,
        words: cpu.queue.unwrap(),
        clock: cpu.clock,
    };
    let mut constructor_cpu = cpu.clone();
    let mut records = Vec::new();
    let mut images = Fixed::default();
    match axis.state {
        State::PCMismatch => {
            cpu.r.pc = cpu.r.pc.wrapping_add(2);
            record(
                &mut records,
                "public registers.pc assignment",
                crate::preflight_axes::object(vec![("delta", crate::preflight_axes::n(2u8))]),
                ResultValue::Unit,
                &cpu,
                &device,
            );
        }
        State::Unprimed => {
            cpu = Cpu::new(Registers::new(), None, 0);
            constructor = Constructor::Default;
            constructor_cpu = cpu.clone();
            record(
                &mut records,
                "PrefetchCpu::default",
                Value::Null,
                ResultValue::Unit,
                &cpu,
                &device,
            );
            cpu.r = source.initial.r;
            record(
                &mut records,
                "public registers assignment",
                crate::binding::registers(&cpu.r),
                ResultValue::Unit,
                &cpu,
                &device,
            );
        }
        State::Stopped => {
            let sr = cpu.r.status_register();
            cpu.r.set_status_register(sr | 0x2000);
            cpu.queue = Some([0x4e72, sr]);
            constructor = Constructor::Prefetched {
                r: cpu.r,
                words: cpu.queue.unwrap(),
                clock: 0,
            };
            constructor_cpu = cpu.clone();
            image(&mut images, "SetupStopI4", &cpu);
            idle(&mut cpu, &mut device, "SetupStopI4", 4, false);
            cpu.r.set_status_register(sr);
            cpu.r.pc = cpu.r.pc.wrapping_add(4);
            cpu.queue = None;
            cpu.pipeline_address = None;
            cpu.stopped = true;
            record(
                &mut records,
                "step_instruction",
                Value::Null,
                ResultValue::Step(Ok(TimedStep {
                    opcode: 0x4e72,
                    clocks: 4,
                    exception: None,
                })),
                &cpu,
                &device,
            );
        }
        State::ImportedBoundary => {
            let boundary = imported(&source, axis.primary.word);
            cpu = Cpu::new(boundary.registers, None, 0);
            cpu.imported = Some(boundary);
            constructor = Constructor::Imported(boundary);
            constructor_cpu = cpu.clone();
            record(
                &mut records,
                "PrefetchCpu::from_group0_boundary",
                crate::preflight_binding::boundary(boundary),
                ResultValue::Unit,
                &cpu,
                &device,
            );
        }
        State::PendingTrace | State::HostFrozen | State::DoubleHalt => {
            cpu = Cpu::primary(&source, false);
            device = source.device.clone();
            device.configure(axis.state);
            images = source.images.clone();
            record(
                &mut records,
                "step_instruction",
                Value::Null,
                ResultValue::Step(source.result()),
                &cpu,
                &device,
            );
            if axis.state == State::DoubleHalt {
                let release = cpu.clock;
                let result =
                    crate::preflight_setup::reset_first(&mut cpu, &mut device, &mut images);
                let arguments = crate::preflight_axes::object(vec![
                    ("sampled_release", Value::Decimal(release)),
                    (
                        "qualification",
                        crate::preflight_axes::s("QualifiedSafeAssertionAndRelease"),
                    ),
                ]);
                record(
                    &mut records,
                    "external_warm_reset",
                    arguments,
                    result,
                    &cpu,
                    &device,
                );
            }
        }
        State::PhysicalUnqualified => {
            let result =
                crate::preflight_setup::physical_first(&mut cpu, &mut device, &mut images, &source);
            record(
                &mut records,
                "step_physical_instruction",
                Value::Null,
                result,
                &cpu,
                &device,
            );
        }
    }
    device.begin_probe();
    record(
        &mut records,
        "Device::begin_probe",
        Value::Null,
        ResultValue::Unit,
        &cpu,
        &device,
    );
    let (input_cpu, input_device) = (cpu.clone(), device.clone());
    let (result, guard) = probe(&mut cpu, &mut device, axis.api);
    Expected {
        axis,
        source,
        constructor,
        constructor_cpu,
        records,
        images,
        input_cpu,
        input_device,
        cpu,
        device,
        result,
        guard,
    }
}
