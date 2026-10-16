use super::device::{Device, access};
use super::fixed::Event;
use super::model::{Case, Failure};
use super::oracle::Expected;
use super::{
    CpuClocks, Group0Capture, InstructionFault, PrefetchCpu, ProcessingClass, SswProfile,
    TimedStep, TimingFault,
};
use zeff_m68k::timing::physical::ImportedGroup0Boundary;

pub(super) fn prepare(
    c: Case,
    mut b: Device,
    route: usize,
    mark: fn(bool),
) -> Option<(
    PrefetchCpu,
    Device,
    Expected,
    InstructionFault<&'static str>,
)> {
    if route >= 8 {
        assert!(route < 11);
        let mut c = c;
        if route == 10 {
            c.handler = 0x4001;
            b = Device::seed(c);
        }
        let kind = [Failure::Host, Failure::Unavailable, Failure::Boundary][route - 8];
        b.fault(
            c.p.extensions() + usize::from(c.p.memory()),
            kind,
            [6, 5, 8][route - 8],
        );
        let e = super::oracle::predict(c, &b);
        let (cpu, b) = super::check::actual(c, b, &e, mark)?;
        let fault = InstructionFault::Execution(e.guard.clone()?);
        return Some((cpu, b, e, fault));
    }
    let mut e = super::oracle::initial(c, &b);
    let fault = match route {
        0 => InstructionFault::TraceUnqualified,
        1 | 2 => InstructionFault::InterruptUnqualified,
        3 | 4 => InstructionFault::Execution(TimingFault::Unprimed),
        5 => InstructionFault::Execution(TimingFault::Stopped),
        6 => InstructionFault::Execution(TimingFault::PendingTrace),
        7 => InstructionFault::Execution(TimingFault::ImportedBoundaryPending),
        _ => panic!("preflight route"),
    };
    let mut initial = c.r;
    let mut words = [c.word, c.retained];
    match route {
        0 => e.r.set_status_register(c.r.status_register() | 0x8000),
        1 => e.level = 6,
        2 => e.edge = true,
        3 => e.r.pc = c.r.pc.wrapping_add(2),
        4 => {
            e.clock = 0;
            e.queue = None;
            e.pipeline_address = None;
        }
        5 => {
            initial.pc = c.r.pc.wrapping_sub(4);
            initial.set_status_register(c.r.status_register() | 0x2000);
            words = [0x4e72, c.r.status_register()];
            e.queue = None;
            e.pipeline_address = None;
            e.stopped = true;
            e.events.push(Event::Idle(c.start, 4));
            e.ordinary[1] += 1;
            e.counts[4] += 1;
            e.clock += 4;
        }
        6 => {
            initial.pc = c.r.pc.wrapping_sub(2);
            initial.set_status_register(c.r.status_register() | 0x8000);
            words = [0x4e71, c.word];
            e.r.set_status_register(c.r.status_register() | 0x8000);
            e.pending_trace = true;
            let a = access(
                c.r.pc.wrapping_add(2),
                c.r.supervisor(),
                true,
                super::AccessKind::Prefetch,
                false,
            );
            e.events.push(Event::Transfer {
                stage: 4,
                phase: None,
                access: a,
                direction: super::Direction::Read,
                value: Some(b.words.sample(a.bus_address)),
                start: c.start,
                elapsed: 4,
                kind: Failure::Complete,
                deposited: false,
            });
            e.queue = Some([c.word, b.words.sample(a.bus_address)]);
            e.ordinary[0] += 1;
            e.clock += 4;
        }
        7 => {
            e.queue = None;
            e.pipeline_address = None;
            e.imported = Some(ImportedGroup0Boundary {
                registers: c.r,
                clock: CpuClocks(c.start),
                capture: Group0Capture {
                    vector: 2,
                    internal_pc: c.r.pc.wrapping_add(c.p.length() + 2),
                    phase_sr: (c.r.status_register() & !31) | 27,
                    ird: 0x4e71,
                    logical_address: c.r.pc.wrapping_add(c.p.length() + 2),
                    function_code: if c.r.supervisor() {
                        super::FunctionCode::SupervisorProgram
                    } else {
                        super::FunctionCode::UserProgram
                    },
                    direction: super::Direction::Read,
                    processing_class: ProcessingClass::InstructionOrGroup2,
                    ssw_profile: SswProfile::SelectedIrdUpperBits,
                },
            });
        }
        _ => (),
    }
    mark(true);
    let mut cpu = if let Some(boundary) = e.imported {
        PrefetchCpu::from_group0_boundary(boundary).unwrap()
    } else if route == 4 {
        let mut cpu = PrefetchCpu::default();
        cpu.registers = c.r;
        cpu
    } else {
        PrefetchCpu::from_prefetched(initial, words, CpuClocks(c.start)).unwrap()
    };
    let setup = match route {
        0 => {
            cpu.registers = e.r;
            true
        }
        1 => {
            cpu.interrupt_input().set_level(6);
            true
        }
        2 => {
            cpu.interrupt_input().set_level(7);
            cpu.interrupt_input().set_level(0);
            true
        }
        3 => {
            cpu.registers.pc = e.r.pc;
            true
        }
        5 | 6 => {
            b.allow_legacy = true;
            let result = cpu.step_instruction(&mut b);
            b.allow_legacy = false;
            result
                == Ok(TimedStep {
                    opcode: words[0],
                    clocks: 4,
                    exception: None,
                })
        }
        _ => true,
    };
    mark(false);
    if !setup || !super::check::verify_at(&cpu, &b, &e, if route == 4 { c.start } else { e.clock })
    {
        return None;
    }
    Some((cpu, b, e, fault))
}
pub(super) fn attempt(
    mut cpu: PrefetchCpu,
    mut b: Device,
    fault: InstructionFault<&'static str>,
    mark: fn(bool),
) -> bool {
    mark(true);
    let before = cpu.clone();
    let device = b.clone();
    let mut copy = cpu.clone();
    let mut copied_bus = b.clone();
    let good =
        super::check::step(&mut cpu, &mut b) == Err(fault.clone()) && cpu == before && b == device;
    if !good {
        mark(false);
        return false;
    }
    let good = super::check::step(&mut copy, &mut copied_bus) == Err(fault)
        && copy == before
        && copied_bus == device;
    mark(false);
    good
}
pub(super) fn run(mark: fn(bool)) -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    super::seed::each(|base| {
        for route in 0..11 {
            let c = super::matrix::status(base, 31, if route == 1 { 0 } else { 5 });
            let good = prepare(c, Device::seed(c), route, mark).is_some_and(
                |(mut cpu, mut b, e, fault)| {
                    if let Some(guard) = e.guard {
                        super::check::guards(&mut cpu, &mut b, guard, mark)
                    } else {
                        attempt(cpu, b, fault, mark)
                    }
                },
            );
            count += 1;
            bad += u32::from(!good);
        }
    });
    (count, bad)
}
