use crate::literal;
use crate::reset_axes;
use crate::reset_binding;
use crate::reset_device::{DefaultDetector, Device};
use crate::reset_engine::{self, Expected};
use zeff_m68k::timing::physical::{EntryFault, EntryStep, PhysicalEntryState};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu, TimingFault};

fn guards(cpu: &mut PrefetchCpu, device: &mut Device, fault: TimingFault<usize>) -> bool {
    let saved_cpu = cpu.clone();
    let saved_device = device.clone();
    let mut valid = cpu.step_instruction(device) == Err(fault.clone());
    valid &= *cpu == saved_cpu && *device == saved_device;
    valid &= cpu.step_boundary(device) == Err(fault.clone());
    valid &= *cpu == saved_cpu && *device == saved_device;
    valid &= cpu.prime(device) == Err(fault.clone());
    valid &= *cpu == saved_cpu && *device == saved_device;
    valid &= cpu.service_pending_trace(device) == Err(fault.clone());
    valid &= *cpu == saved_cpu && *device == saved_device;
    valid &= cpu.step(device) == Err(fault);
    valid &= *cpu == saved_cpu && *device == saved_device;
    valid
}

fn execute(
    cpu: &mut PrefetchCpu,
    device: &mut Device,
    expected: &Expected,
    valid: &mut bool,
) -> Result<EntryStep, EntryFault<usize>> {
    let origin = cpu.step_instruction(device);
    *valid &= origin == expected.origin.result();
    *valid &= crate::reset_engine::Cpu::origin(&expected.origin).matches(cpu);
    *valid &= *device == expected.origin_device;
    assert!(cpu.interrupt_input().set_level(if expected.input_cpu.edge {
        7
    } else {
        expected.input_cpu.level
    }));
    if expected.input_cpu.edge {
        assert!(cpu.interrupt_input().set_level(expected.input_cpu.level));
    }
    device.ordinary.clock = expected.boundary.sampled_release.0;
    *valid &= expected.input_cpu.matches(cpu) && *device == expected.input_device;
    let result = if device.detector.omitted {
        cpu.external_warm_reset(&mut DefaultDetector(device), expected.boundary)
    } else {
        cpu.external_warm_reset(device, expected.boundary)
    };
    device.record_default(&result);
    result
}

fn run_one(expected: &Expected) -> bool {
    let seed = &expected.origin.initial;
    let input = (seed.r, seed.queue, CpuClocks(seed.device.clock));
    let owned_input = input;
    let Some(mut cpu) = PrefetchCpu::from_prefetched(input.0, input.1, input.2) else {
        return false;
    };
    let mut copied_cpu = cpu.clone();
    let mut device = expected.initial_device.clone();
    let mut copied_device = device.clone();
    let mut valid = input == owned_input && cpu == copied_cpu && device == copied_device;
    valid &= format!("{cpu:?}") == expected.origin.debug(true)
        && format!("{copied_cpu:?}") == expected.origin.debug(true);
    let result = execute(&mut cpu, &mut device, expected, &mut valid);
    let copied = execute(&mut copied_cpu, &mut copied_device, expected, &mut valid);
    valid &= result == expected.result && copied == expected.result;
    valid &= expected.cpu.matches(&cpu) && expected.cpu.matches(&copied_cpu);
    valid &= cpu == copied_cpu && device == copied_device && device == expected.device;
    let fault = if expected.cpu.frozen {
        Some(TimingFault::Frozen)
    } else if matches!(
        expected.cpu.state,
        PhysicalEntryState::DoubleFaultHalted { .. }
    ) {
        Some(TimingFault::DoubleFaultHalted)
    } else {
        None
    };
    if let Some(fault) = fault {
        let original = guards(&mut cpu, &mut device, fault.clone());
        let copied = guards(&mut copied_cpu, &mut copied_device, fault);
        valid &= original && copied;
        valid &= cpu == copied_cpu && device == copied_device;
    }
    valid
}

pub fn all(log: bool) -> (usize, usize) {
    let (mut count, mut bad, mut host_frozen) = (0, 0, 0);
    let mut counts = [0; 6];
    let mut maxima = [0; 3];
    reset_axes::each(|axis, request| {
        counts[axis.get("section").number() as usize - 33] += 1;
        let expected = reset_engine::predict(axis, request);
        host_frozen += usize::from(expected.result == Err(EntryFault::HostFrozen));
        for (slot, value) in maxima.iter_mut().zip([
            expected.device.ordinary.words.len,
            expected.device.ordinary.events.len + expected.device.events.len,
            expected.device.ordinary.scripts.len + expected.device.scripts.len,
        ]) {
            *slot = (*slot).max(value);
        }
        if !run_one(&expected) {
            bad += 1;
            if log {
                eprintln!(
                    "ordinary reset row {count} mismatch section={}",
                    expected.axis.get("section").number()
                );
            }
        }
        count += 1;
    });
    assert_eq!(counts, [71712, 15936, 3984, 23904, 7968, 10624]);
    assert_eq!(host_frozen, 90496);
    assert_eq!(maxima, [12, 13, 11]);
    (count, bad)
}

fn anchors(log: bool, probe: bool) -> (usize, usize) {
    let rows = literal::anchors();
    let mut bad = 0;
    for row in &rows {
        let expected =
            reset_engine::predict(row.get("axis").clone(), row.get("reset_request").clone());
        if reset_binding::wrapper(&expected) != *row || probe && !run_one(&expected) {
            bad += 1;
            if log {
                eprintln!(
                    "ordinary reset literal {} mismatch",
                    row.get("axis").get("id").text()
                );
            }
        }
    }
    (rows.len(), bad)
}
pub fn literals(log: bool) -> (usize, usize) {
    anchors(log, false)
}
pub fn bounded(log: bool) -> (usize, usize) {
    anchors(log, true)
}
