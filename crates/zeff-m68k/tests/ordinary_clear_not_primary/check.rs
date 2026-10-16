use crate::binding;
use crate::device::Device;
use crate::literal;
use crate::oracle::{self, Expected};
use crate::seed::{self, Axis};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu, TimingFault};

fn frozen_apis(cpu: &mut PrefetchCpu, device: &mut Device) -> bool {
    let saved_cpu = cpu.clone();
    let saved_device = device.clone();
    let mut valid = true;
    valid &= matches!(cpu.step_instruction(device), Err(TimingFault::Frozen));
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
    let initial = &expected.initial;
    assert_eq!((initial.level, initial.edge), (0, false));
    let input = (initial.r, initial.queue, CpuClocks(initial.device.clock));
    let copied_input = input;
    let saved_device = initial.device.clone();
    let mut device = saved_device.clone();
    let mut copied_device = device.clone();
    let Some(mut cpu) = PrefetchCpu::from_prefetched(input.0, input.1, input.2) else {
        let copied_cpu =
            PrefetchCpu::from_prefetched(copied_input.0, copied_input.1, copied_input.2);
        return !expected.importable
            && copied_cpu.is_none()
            && input == copied_input
            && device == saved_device
            && copied_device == saved_device
            && device == expected.device;
    };
    if !expected.importable {
        return false;
    }
    let mut copied_cpu = cpu.clone();
    let mut valid = input == copied_input && cpu == copied_cpu && device == copied_device;
    valid &= cpu.registers == initial.r
        && cpu.clock().0 == initial.device.clock
        && cpu.queued_words() == Some(initial.queue);
    valid &= format!("{cpu:?}") == expected.debug(true)
        && format!("{copied_cpu:?}") == expected.debug(true);
    let actual = cpu.step_instruction(&mut device);
    let copied = copied_cpu.step_instruction(&mut copied_device);
    valid &= actual == expected.result() && copied == expected.result();
    valid &= cpu == copied_cpu && device == copied_device;
    valid &= cpu.registers == expected.r
        && cpu.clock().0 == expected.device.clock
        && cpu.queued_words() == Some(expected.queue);
    valid &= format!("{cpu:?}") == expected.debug(false)
        && format!("{copied_cpu:?}") == expected.debug(false);
    valid &= device == expected.device && copied_device == expected.device;
    if expected.error.is_some() {
        valid &=
            frozen_apis(&mut cpu, &mut device) && frozen_apis(&mut copied_cpu, &mut copied_device);
        valid &= cpu == copied_cpu && device == copied_device;
    }
    valid
}

pub fn run(log: bool) -> (usize, usize) {
    let axes = seed::axes();
    let mut bad = 0;
    let mut counts = [0; 4];
    let mut maxima = [0; 3];
    let mut constructor_rejections = 0;
    for (index, axis) in axes.iter().enumerate() {
        let expected = oracle::predict(seed::build(axis));
        counts[match axis.section {
            "default_completion" => 0,
            "callback_Host" => 1,
            "blocked_transfer" => 2,
            "Long_tail" => 3,
            _ => panic!("section"),
        }] += 1;
        constructor_rejections += usize::from(!expected.importable);
        for (target, value) in maxima.iter_mut().zip([
            expected.device.words.len,
            expected.device.events.len,
            expected.device.scripts.len,
        ]) {
            *target = (*target).max(value);
        }
        if !run_one(&expected) {
            bad += 1;
            if log {
                eprintln!(
                    "ordinary bounded primary OCB{index:06} word={:04x} section={} phase={:?} mismatch",
                    axis.word, axis.section, axis.phase
                );
            }
        }
    }
    assert_eq!(counts, [432, 1256, 1256, 96]);
    assert_eq!(constructor_rejections, 0);
    assert_eq!(maxima, [6, 5, 5]);
    (axes.len(), bad)
}

pub fn source_literals(log: bool) -> (usize, usize) {
    let rows = literal::anchors();
    let mut bad = 0;
    let mut seams = 0;
    for row in &rows {
        let axis = Axis::from_literal(row.get("axis"));
        seams += usize::from(axis.section == "seam_probe");
        let expected = oracle::predict(seed::build(&axis));
        if binding::wrapper(&expected, row.get("axis").clone()) != *row {
            bad += 1;
            if log {
                eprintln!(
                    "ordinary source literal {} mismatch",
                    row.get("axis").get("id").text()
                );
            }
        }
    }
    assert_eq!((rows.len(), seams), (184, 12));
    (rows.len(), bad)
}

pub fn seams(log: bool) -> (usize, usize) {
    let rows = literal::anchors();
    let mut count = 0;
    let mut bad = 0;
    let mut rejected = 0;
    for row in &rows {
        let axis = Axis::from_literal(row.get("axis"));
        if axis.section != "seam_probe" {
            continue;
        }
        let expected = oracle::predict(seed::build(&axis));
        rejected += usize::from(!expected.importable);
        if !run_one(&expected) {
            bad += 1;
            if log {
                eprintln!(
                    "ordinary seam {} mismatch",
                    row.get("axis").get("id").text()
                );
            }
        }
        count += 1;
    }
    assert_eq!((count, rejected), (12, 1));
    (count, bad)
}
