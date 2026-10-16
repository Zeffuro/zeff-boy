use crate::device::Device;
use crate::entry_axes::{self, Api, EntryAxis};
use crate::entry_binding;
use crate::entry_engine::{self, Completion, Expected};
use crate::literal;
use zeff_m68k::timing::{CpuClocks, PrefetchCpu, TimedOutcome, TimingFault};

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

fn boundary(cpu: &mut PrefetchCpu, device: &mut Device) -> Result<Completion, TimingFault<usize>> {
    cpu.step_boundary(device).map(|outcome| match outcome {
        TimedOutcome::Instruction(step) => Completion::Instruction(step),
        TimedOutcome::Interrupt {
            level,
            vector,
            clocks,
        } => Completion::Interrupt {
            level,
            vector,
            clocks,
        },
        _ => panic!("unexpected boundary outcome"),
    })
}

fn execute(
    cpu: &mut PrefetchCpu,
    device: &mut Device,
    expected: &Expected,
    valid: &mut bool,
) -> Result<Completion, TimingFault<usize>> {
    if expected.axis.trace && expected.axis.api == Api::Step {
        return cpu.step(device).map(Completion::Instruction);
    }
    let origin = cpu.step_instruction(device);
    *valid &= origin == expected.origin.result();
    origin?;
    *valid &= cpu.registers == expected.origin.r
        && cpu.clock().0 == expected.origin.device.clock
        && cpu.queued_words() == Some(expected.origin.queue)
        && format!("{cpu:?}") == expected.origin.debug(false)
        && *device == expected.origin_device;
    if expected.axis.trace {
        assert_eq!(expected.axis.api, Api::Service);
        return cpu.service_pending_trace(device).map(Completion::Trace);
    }
    assert!(cpu.interrupt_input().set_level(if expected.axis.edge {
        7
    } else {
        expected.axis.irq
    }));
    if expected.axis.edge {
        assert!(cpu.interrupt_input().set_level(expected.axis.irq));
    }
    boundary(cpu, device)
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
    let actual = execute(&mut cpu, &mut device, expected, &mut valid);
    let copied = execute(&mut copied_cpu, &mut copied_device, expected, &mut valid);
    valid &= actual == expected.result() && copied == expected.result();
    let image = expected.cpu_image();
    valid &= cpu.registers == expected.r
        && cpu.clock().0 == expected.clock
        && cpu.queued_words() == Some(expected.queue);
    valid &=
        format!("{cpu:?}") == image.debug(false) && format!("{copied_cpu:?}") == image.debug(false);
    valid &= cpu == copied_cpu && device == copied_device && device == expected.device;
    if expected.error.is_some() {
        valid &= frozen(&mut cpu, &mut device) && frozen(&mut copied_cpu, &mut copied_device);
        valid &= cpu == copied_cpu && device == copied_device;
    }
    valid
}

pub fn all(log: bool) -> (usize, usize) {
    let mut count = 0;
    let mut bad = 0;
    let mut counts = [0; 4];
    let mut maxima = [0; 4];
    entry_axes::each(|axis| {
        counts[usize::from(axis.section - 29)] += 1;
        let expected = entry_engine::predict(axis);
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
                eprintln!(
                    "entry row {count} mismatch section={}",
                    expected.axis.section
                );
            }
        }
        count += 1;
    });
    assert_eq!(counts, [58752, 19440, 6480, 295488]);
    assert_eq!(maxima, [33, 16, 13, 13]);
    (count, bad)
}

fn literal_rows(rows: Vec<literal::Value>, log: bool, probe: bool) -> (usize, usize) {
    let mut bad = 0;
    for row in &rows {
        let expected = entry_engine::predict(EntryAxis::from_value(row.get("axis")));
        let bound = entry_binding::wrapper(&expected) == *row;
        if !bound || probe && !run_one(&expected) {
            bad += 1;
            if log {
                eprintln!(
                    "fault literal {} mismatch",
                    row.get("axis").get("id").text()
                );
            }
        }
    }
    (rows.len(), bad)
}

fn both(log: bool, probe: bool) -> (usize, usize) {
    let first = literal_rows(literal::anchors(), log, probe);
    let second = literal_rows(literal::reserve_anchors(), log, probe);
    (first.0 + second.0, first.1 + second.1)
}

pub fn report_literals(log: bool) -> (usize, usize) {
    literal_rows(literal::anchors(), log, false)
}
pub fn reserve_literals(log: bool) -> (usize, usize) {
    literal_rows(literal::reserve_anchors(), log, false)
}
pub fn literals(log: bool) -> (usize, usize) {
    both(log, false)
}
pub fn bounded(log: bool) -> (usize, usize) {
    both(log, true)
}
