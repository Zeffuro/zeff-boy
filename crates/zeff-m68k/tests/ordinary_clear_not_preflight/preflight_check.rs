use crate::device::Device;
use crate::literal::{self, Value};
use crate::preflight_axes::{self, Api, PreflightAxis, State};
use crate::preflight_binding;
use crate::preflight_engine;
use crate::preflight_state::{Constructor, Cpu, Expected, ResultValue};
use zeff_m68k::timing::physical::{ExternalResetBoundary, ResetQualification};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu, TimingFault};

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

fn construct(constructor: &Constructor) -> PrefetchCpu {
    match constructor {
        Constructor::Prefetched { r, words, clock } => {
            PrefetchCpu::from_prefetched(*r, *words, CpuClocks(*clock)).unwrap()
        }
        Constructor::Default => PrefetchCpu::default(),
        Constructor::Imported(boundary) => PrefetchCpu::from_group0_boundary(*boundary).unwrap(),
    }
}

fn setup(cpu: &mut PrefetchCpu, device: &mut Device, expected: &Expected) -> bool {
    let mut index = 0;
    let mut valid = true;
    let mut checked = |cpu: &PrefetchCpu, device: &Device, result: ResultValue| {
        let record = &expected.records[index];
        valid &= result == record.result && matches(cpu, &record.cpu) && *device == record.device;
        index += 1;
    };
    match expected.axis.state {
        State::PCMismatch => {
            cpu.registers.pc = cpu.registers.pc.wrapping_add(2);
            checked(cpu, device, ResultValue::Unit);
        }
        State::Unprimed => {
            checked(cpu, device, ResultValue::Unit);
            cpu.registers = expected.source.initial.r;
            checked(cpu, device, ResultValue::Unit);
        }
        State::Stopped | State::PendingTrace | State::HostFrozen | State::DoubleHalt => {
            let result = cpu.step_instruction(device);
            checked(cpu, device, ResultValue::Step(result));
            if expected.axis.state == State::DoubleHalt {
                let boundary = ExternalResetBoundary {
                    sampled_release: cpu.clock(),
                    qualification: ResetQualification::QualifiedSafeAssertionAndRelease,
                };
                let result = cpu.external_warm_reset(device, boundary);
                checked(cpu, device, ResultValue::Reset(result));
            }
        }
        State::ImportedBoundary => checked(cpu, device, ResultValue::Unit),
        State::PhysicalUnqualified => {
            let result = cpu.step_physical_instruction(device);
            checked(cpu, device, ResultValue::Physical(result));
        }
    }
    let before = cpu.clone();
    device.begin_probe();
    let unchanged = *cpu == before;
    checked(cpu, device, ResultValue::Unit);
    assert_eq!(index, expected.records.len());
    valid && unchanged && matches(cpu, &expected.input_cpu) && *device == expected.input_device
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

fn frozen(cpu: &mut PrefetchCpu, device: &mut Device) -> bool {
    let saved_cpu = cpu.clone();
    let saved_device = device.clone();
    let mut valid = matches!(cpu.step_instruction(device), Err(TimingFault::Frozen));
    valid &= *cpu == saved_cpu && *device == saved_device;
    valid &= matches!(cpu.step_boundary(device), Err(TimingFault::Frozen));
    valid &= *cpu == saved_cpu && *device == saved_device;
    valid &= matches!(cpu.prime(device), Err(TimingFault::Frozen));
    valid &= *cpu == saved_cpu && *device == saved_device;
    valid &= matches!(cpu.service_pending_trace(device), Err(TimingFault::Frozen));
    valid &= *cpu == saved_cpu && *device == saved_device;
    valid &= matches!(cpu.step(device), Err(TimingFault::Frozen));
    valid &= *cpu == saved_cpu && *device == saved_device;
    valid
}

fn run_one(expected: &Expected) -> bool {
    let mut cpu = construct(&expected.constructor);
    let mut copied_cpu = cpu.clone();
    let mut device = expected.source.initial.device.clone();
    device.configure(expected.axis.state);
    let mut copied_device = device.clone();
    let mut valid = matches(&cpu, &expected.constructor_cpu)
        && matches(&copied_cpu, &expected.constructor_cpu)
        && cpu == copied_cpu
        && device == copied_device;
    let original_setup = setup(&mut cpu, &mut device, expected);
    let copied_setup = setup(&mut copied_cpu, &mut copied_device, expected);
    valid &= original_setup && copied_setup && cpu == copied_cpu && device == copied_device;
    let actual = probe(&mut cpu, &mut device, expected.axis.api);
    let copied = probe(&mut copied_cpu, &mut copied_device, expected.axis.api);
    valid &= actual == expected.result && copied == expected.result;
    valid &= matches(&cpu, &expected.cpu) && matches(&copied_cpu, &expected.cpu);
    valid &= cpu == copied_cpu && device == copied_device && device == expected.device;
    if expected.cpu.frozen {
        let original = frozen(&mut cpu, &mut device);
        let copied = frozen(&mut copied_cpu, &mut copied_device);
        valid &= original && copied && cpu == copied_cpu && device == copied_device;
    }
    valid
}

pub fn expected_rows(mut apply: impl FnMut(Value)) {
    preflight_axes::each(|axis| {
        apply(preflight_binding::wrapper(&preflight_engine::predict(axis)))
    });
}

pub fn all(log: bool) -> (usize, usize) {
    let mut count = 0;
    let mut bad = 0;
    let mut frozen = 0;
    let mut maxima = [0; 4];
    preflight_axes::each(|axis| {
        let expected = preflight_engine::predict(axis);
        frozen += usize::from(expected.cpu.frozen);
        for (slot, value) in maxima.iter_mut().zip([
            expected.device.words.len,
            expected.device.events.len,
            expected.device.scripts.len,
            expected.device.phase_count,
        ]) {
            *slot = (*slot).max(value);
        }
        if !run_one(&expected) {
            bad += 1;
            if log {
                eprintln!("ordinary preflight row {count} mismatch");
            }
        }
        count += 1;
    });
    assert_eq!((count, frozen), (17280, 2180));
    assert_eq!(maxima, [19, 14, 12, 22]);
    (count, bad)
}

pub fn literals(log: bool) -> (usize, usize) {
    let rows = literal::anchors();
    let mut bad = 0;
    for row in &rows {
        let expected = preflight_engine::predict(PreflightAxis::from_value(row.get("axis")));
        if preflight_binding::wrapper(&expected) != *row {
            bad += 1;
            if log {
                eprintln!(
                    "preflight literal {} mismatch",
                    row.get("axis").get("id").text()
                );
            }
        }
    }
    assert_eq!(rows.len(), 491);
    (rows.len(), bad)
}

pub fn probes(log: bool) -> (usize, usize) {
    let rows = literal::anchors();
    let mut bad = 0;
    for row in &rows {
        let expected = preflight_engine::predict(PreflightAxis::from_value(row.get("axis")));
        if !run_one(&expected) {
            bad += 1;
            if log {
                eprintln!(
                    "preflight bounded probe {} mismatch",
                    row.get("axis").get("id").text()
                );
            }
        }
    }
    assert_eq!(rows.len(), 491);
    (rows.len(), bad)
}

pub fn source_axes(log: bool) -> (usize, usize) {
    let rows = literal::anchors();
    let mut index = 0;
    let mut bad = 0;
    preflight_axes::each(|axis| {
        if index < rows.len() && axis.value.get("id") == rows[index].get("axis").get("id") {
            if axis.value != *rows[index].get("axis") {
                bad += 1;
                if log {
                    eprintln!("preflight source axis {index} mismatch");
                }
            }
            index += 1;
        }
    });
    assert_eq!(index, 491);
    (index, bad)
}
