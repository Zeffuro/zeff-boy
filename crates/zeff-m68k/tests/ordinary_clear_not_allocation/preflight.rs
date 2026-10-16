use crate::allocation_support::{Windows, positive_control};
use crate::device::Device;
use crate::preflight_axes::{self, Api, State};
use crate::preflight_engine;
use crate::preflight_state::{Constructor, Cpu, Expected, ResultValue};
use zeff_m68k::Registers;
use zeff_m68k::timing::physical::{ExternalResetBoundary, PhysicalEntryState, ResetQualification};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu, TimingFault};

struct Snapshot {
    cpu: PrefetchCpu,
    device: Device,
    result: ResultValue,
}
struct Setup {
    cpu: PrefetchCpu,
    device: Device,
    constructor_cpu: PrefetchCpu,
    constructor_device: Device,
    records: [Option<Snapshot>; 3],
    count: usize,
}
impl Setup {
    fn record(&mut self, result: ResultValue) {
        self.records[self.count] = Some(Snapshot {
            cpu: self.cpu.clone(),
            device: self.device.clone(),
            result,
        });
        self.count += 1;
    }
}

fn setup(
    constructor: &Constructor,
    state: State,
    registers: Registers,
    device: Device,
) -> Option<Setup> {
    let cpu = match constructor {
        Constructor::Prefetched { r, words, clock } => {
            PrefetchCpu::from_prefetched(*r, *words, CpuClocks(*clock))?
        }
        Constructor::Default => PrefetchCpu::default(),
        Constructor::Imported(boundary) => PrefetchCpu::from_group0_boundary(*boundary).ok()?,
    };
    let mut setup = Setup {
        constructor_cpu: cpu.clone(),
        constructor_device: device.clone(),
        cpu,
        device,
        records: std::array::from_fn(|_| None),
        count: 0,
    };
    match state {
        State::PCMismatch => {
            setup.cpu.registers.pc = setup.cpu.registers.pc.wrapping_add(2);
            setup.record(ResultValue::Unit);
        }
        State::Unprimed => {
            setup.record(ResultValue::Unit);
            setup.cpu.registers = registers;
            setup.record(ResultValue::Unit);
        }
        State::Stopped | State::PendingTrace | State::HostFrozen | State::DoubleHalt => {
            let result = setup.cpu.step_instruction(&mut setup.device);
            setup.record(ResultValue::Step(result));
            if state == State::DoubleHalt {
                let boundary = ExternalResetBoundary {
                    sampled_release: setup.cpu.clock(),
                    qualification: ResetQualification::QualifiedSafeAssertionAndRelease,
                };
                let result = setup.cpu.external_warm_reset(&mut setup.device, boundary);
                setup.record(ResultValue::Reset(result));
            }
        }
        State::ImportedBoundary => setup.record(ResultValue::Unit),
        State::PhysicalUnqualified => {
            let result = setup.cpu.step_physical_instruction(&mut setup.device);
            setup.record(ResultValue::Physical(result));
        }
    }
    setup.device.begin_probe();
    setup.record(ResultValue::Unit);
    Some(setup)
}

fn matches(cpu: &PrefetchCpu, expected: &Cpu) -> bool {
    cpu.registers == expected.r
        && cpu.clock().0 == expected.clock
        && cpu.queued_words() == expected.queue
        && cpu.stopped() == expected.stopped
        && cpu.trace_pending() == expected.trace
        && cpu.physical_entry_state() == expected.physical
        && cpu.physical_inspection() == expected.inspection
        && format!("{cpu:?}") == expected.debug()
}

fn probe(cpu: &mut PrefetchCpu, device: &mut Device, api: Api) -> ResultValue {
    match api {
        Api::Instruction => ResultValue::Step(cpu.step_instruction(device)),
        Api::Step => ResultValue::Step(cpu.step(device)),
        Api::Boundary => ResultValue::Boundary(cpu.step_boundary(device)),
        Api::Prime => ResultValue::Prime(cpu.prime(device)),
        Api::Trace => ResultValue::Trace(cpu.service_pending_trace(device)),
    }
}

fn guards(
    cpu: &mut PrefetchCpu,
    device: &mut Device,
    fault: TimingFault<usize>,
    windows: &mut Windows,
) {
    let saved_cpu = cpu.clone();
    let saved_device = device.clone();
    assert_eq!(
        windows.measure(|| cpu.step_instruction(device)),
        Err(fault.clone())
    );
    assert_eq!(*cpu, saved_cpu);
    assert_eq!(*device, saved_device);
    assert_eq!(
        windows.measure(|| cpu.step_boundary(device)),
        Err(fault.clone())
    );
    assert_eq!(*cpu, saved_cpu);
    assert_eq!(*device, saved_device);
    assert_eq!(windows.measure(|| cpu.prime(device)), Err(fault.clone()));
    assert_eq!(*cpu, saved_cpu);
    assert_eq!(*device, saved_device);
    assert_eq!(
        windows.measure(|| cpu.service_pending_trace(device)),
        Err(fault.clone())
    );
    assert_eq!(*cpu, saved_cpu);
    assert_eq!(*device, saved_device);
    assert_eq!(windows.measure(|| cpu.step(device)), Err(fault));
    assert_eq!(*cpu, saved_cpu);
    assert_eq!(*device, saved_device);
}

fn execute(expected: &Expected, windows: &mut Windows) -> bool {
    let mut initial_device = expected.source.initial.device.clone();
    initial_device.configure(expected.axis.state);
    let initial_snapshot = initial_device.clone();
    let constructor = &expected.constructor;
    let state = expected.axis.state;
    let registers = expected.source.initial.r;
    let api = expected.axis.api;
    let prepared = windows.measure(|| setup(constructor, state, registers, initial_device));
    let prepared = prepared.unwrap();
    assert!(matches(
        &prepared.constructor_cpu,
        &expected.constructor_cpu
    ));
    assert_eq!(prepared.constructor_device, initial_snapshot);
    assert_eq!(prepared.count, expected.records.len());
    for (actual, expected) in prepared.records.iter().flatten().zip(&expected.records) {
        assert_eq!(actual.result, expected.result);
        assert!(matches(&actual.cpu, &expected.cpu));
        assert_eq!(actual.device, expected.device);
    }
    let previous = prepared.records[prepared.count - 2].as_ref().unwrap();
    assert_eq!(prepared.cpu, previous.cpu);
    assert!(matches(&prepared.cpu, &expected.input_cpu));
    assert_eq!(prepared.device, expected.input_device);
    let mut cpu = prepared.cpu;
    let mut device = prepared.device;
    let (mut copied_cpu, mut copied_device) = windows.measure(|| (cpu.clone(), device.clone()));
    assert_eq!(cpu, copied_cpu);
    assert_eq!(device, copied_device);
    let actual = windows.measure(|| probe(&mut cpu, &mut device, api));
    let copied = windows.measure(|| probe(&mut copied_cpu, &mut copied_device, api));
    assert_eq!(actual, expected.result);
    assert_eq!(copied, expected.result);
    assert!(matches(&cpu, &expected.cpu));
    assert!(matches(&copied_cpu, &expected.cpu));
    assert_eq!(cpu, copied_cpu);
    assert_eq!(device, copied_device);
    assert_eq!(device, expected.device);
    let fault = if expected.cpu.frozen {
        Some(TimingFault::Frozen)
    } else if matches!(
        expected.cpu.physical,
        PhysicalEntryState::DoubleFaultHalted { .. }
    ) {
        Some(TimingFault::DoubleFaultHalted)
    } else if matches!(
        expected.cpu.physical,
        PhysicalEntryState::FaultUnqualified { .. }
    ) {
        Some(TimingFault::PhysicalFaultUnqualified)
    } else if expected.cpu.imported.is_some() {
        Some(TimingFault::ImportedBoundaryPending)
    } else {
        None
    };
    if let Some(fault) = fault {
        guards(&mut cpu, &mut device, fault.clone(), windows);
        guards(&mut copied_cpu, &mut copied_device, fault, windows);
        assert_eq!(cpu, copied_cpu);
        assert_eq!(device, copied_device);
        true
    } else {
        false
    }
}

#[test]
fn all_preflight_17280_requests_155720_windows() {
    positive_control();
    let mut windows = Windows::default();
    let mut count = 0;
    let mut terminal = 0;
    preflight_axes::each(|axis| {
        let expected = preflight_engine::predict(axis);
        terminal += usize::from(execute(&expected, &mut windows));
        count += 1;
    });
    assert_eq!((count, terminal, windows.count), (17280, 8660, 155720));
    println!(
        "preflight requests={count} windows={} delta=0 positive_allocator_control=PASS",
        windows.count
    );
}
