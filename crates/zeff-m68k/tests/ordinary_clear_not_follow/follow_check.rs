use crate::device::Device;
use crate::follow_axes::{self, Api, FollowAxis};
use crate::follow_binding;
use crate::follow_engine::{self, Expected};
use crate::literal;
use zeff_m68k::timing::{CpuClocks, PrefetchCpu, TimedOutcome, TimedStep, TimingFault};

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

fn next(
    cpu: &mut PrefetchCpu,
    device: &mut Device,
    api: Api,
) -> Result<TimedStep, TimingFault<usize>> {
    match api {
        Api::Instruction => cpu.step_instruction(device),
        Api::Step => cpu.step(device),
        Api::Boundary => cpu.step_boundary(device).map(|outcome| match outcome {
            TimedOutcome::Instruction(step) => step,
            _ => panic!("unexpected next boundary"),
        }),
    }
}

fn run_one(expected: &Expected) -> bool {
    let initial = &expected.origin.initial;
    let input = (initial.r, initial.queue, CpuClocks(initial.device.clock));
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
    let original_step = cpu.step_instruction(&mut device);
    let copied_step = copied_cpu.step_instruction(&mut copied_device);
    valid &= original_step == expected.origin.result() && copied_step == expected.origin.result();
    valid &= cpu.registers == expected.origin.r
        && cpu.clock().0 == expected.origin.device.clock
        && cpu.queued_words() == Some(expected.origin.queue);
    valid &= format!("{cpu:?}") == expected.origin.debug(false)
        && format!("{copied_cpu:?}") == expected.origin.debug(false);
    valid &= device == expected.origin_device
        && copied_device == expected.origin_device
        && cpu == copied_cpu;
    let result = next(&mut cpu, &mut device, expected.axis.api);
    let copied_result = next(&mut copied_cpu, &mut copied_device, expected.axis.api);
    valid &= result == expected.result() && copied_result == expected.result();
    valid &= cpu.registers == expected.r
        && cpu.clock().0 == expected.device.clock
        && cpu.queued_words() == expected.queue;
    valid &= cpu.stopped() == expected.stopped && !cpu.trace_pending();
    valid &=
        format!("{cpu:?}") == expected.debug() && format!("{copied_cpu:?}") == expected.debug();
    valid &= cpu == copied_cpu && device == copied_device && device == expected.device;
    if expected.error.is_some() {
        let original = frozen(&mut cpu, &mut device);
        let copied = frozen(&mut copied_cpu, &mut copied_device);
        valid &= original && copied && cpu == copied_cpu && device == copied_device;
    }
    valid
}

pub fn all(log: bool) -> (usize, usize) {
    let mut count = 0;
    let mut bad = 0;
    let mut faults = 0;
    let mut stopped = 0;
    let mut maxima = [0; 4];
    follow_axes::each(|axis| {
        let expected = follow_engine::predict(axis);
        faults += usize::from(expected.error.is_some());
        stopped += usize::from(expected.stopped);
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
                eprintln!("ordinary next row {count} mismatch");
            }
        }
        count += 1;
    });
    assert_eq!((count, faults, stopped), (6480, 60, 648));
    assert_eq!(maxima, [21, 14, 12, 12]);
    (count, bad)
}

pub fn literals(log: bool) -> (usize, usize) {
    let rows = literal::anchors();
    let mut bad = 0;
    let mut faults = 0;
    for row in &rows {
        let expected = follow_engine::predict(FollowAxis::from_value(row.get("axis")));
        faults += usize::from(expected.error.is_some());
        if follow_binding::wrapper(&expected) != *row {
            bad += 1;
            if log {
                eprintln!(
                    "ordinary next source literal {} mismatch",
                    row.get("axis").get("id").text()
                );
            }
        }
    }
    assert_eq!((rows.len(), faults), (520, 60));
    (rows.len(), bad)
}

pub fn probes(log: bool) -> (usize, usize) {
    let rows = literal::anchors();
    let mut bad = 0;
    let mut faults = 0;
    for row in &rows {
        let expected = follow_engine::predict(FollowAxis::from_value(row.get("axis")));
        faults += usize::from(expected.error.is_some());
        if !run_one(&expected) {
            bad += 1;
            if log {
                eprintln!(
                    "ordinary next bounded probe {} mismatch",
                    row.get("axis").get("id").text()
                );
            }
        }
    }
    assert_eq!((rows.len(), faults), (520, 60));
    (rows.len(), bad)
}

pub fn source_axes(log: bool) -> (usize, usize) {
    let rows = literal::anchors();
    let mut index = 0;
    let mut bad = 0;
    follow_axes::each(|axis| {
        if index < rows.len() && axis.value.get("id") == rows[index].get("axis").get("id") {
            if axis.value != *rows[index].get("axis") {
                bad += 1;
                if log {
                    eprintln!("ordinary next literal axis {index} mismatch");
                }
            }
            index += 1;
        }
    });
    assert_eq!(index, 520);
    (index, bad)
}
