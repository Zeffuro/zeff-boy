use crate::allocation_support::{Windows, positive_control};
use crate::device::Device;
use crate::entry_axes::{self, Api};
use crate::entry_engine::{self, Completion, Expected};
use std::hint::black_box;
use zeff_m68k::timing::{CpuClocks, PrefetchCpu, TimedOutcome, TimingFault};

fn frozen(cpu: &mut PrefetchCpu, device: &mut Device, windows: &mut Windows) {
    let saved_cpu = cpu.clone();
    let saved_device = device.clone();
    let result = windows.measure(|| cpu.step_instruction(device));
    assert_eq!(result, Err(TimingFault::Frozen));
    assert_eq!(*cpu, saved_cpu);
    assert_eq!(*device, saved_device);
    let result = windows.measure(|| cpu.step_boundary(device));
    assert_eq!(result, Err(TimingFault::Frozen));
    assert_eq!(*cpu, saved_cpu);
    assert_eq!(*device, saved_device);
    let result = windows.measure(|| cpu.prime(device));
    assert_eq!(result, Err(TimingFault::Frozen));
    assert_eq!(*cpu, saved_cpu);
    assert_eq!(*device, saved_device);
    let result = windows.measure(|| cpu.service_pending_trace(device));
    assert_eq!(result, Err(TimingFault::Frozen));
    assert_eq!(*cpu, saved_cpu);
    assert_eq!(*device, saved_device);
    let result = windows.measure(|| cpu.step(device));
    assert_eq!(result, Err(TimingFault::Frozen));
    assert_eq!(*cpu, saved_cpu);
    assert_eq!(*device, saved_device);
}

fn final_image(cpu: &PrefetchCpu, device: &Device, expected: &Expected) {
    assert_eq!(cpu.registers, expected.r);
    assert_eq!(cpu.clock().0, expected.clock);
    assert_eq!(cpu.queued_words(), Some(expected.queue));
    assert_eq!(format!("{cpu:?}"), expected.cpu_image().debug(false));
    assert_eq!(*device, expected.device);
}
fn origin_image(cpu: &PrefetchCpu, device: &Device, expected: &Expected) {
    assert_eq!(cpu.registers, expected.origin.r);
    assert_eq!(cpu.clock().0, expected.origin.device.clock);
    assert_eq!(cpu.queued_words(), Some(expected.origin.queue));
    assert_eq!(format!("{cpu:?}"), expected.origin.debug(false));
    assert_eq!(*device, expected.origin_device);
}
fn boundary(outcome: TimedOutcome) -> Completion {
    match outcome {
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
    }
}

fn execute(expected: &Expected, windows: &mut Windows) {
    let seed = &expected.origin.initial;
    let input = (seed.r, seed.queue, CpuClocks(seed.device.clock));
    let initial_debug = expected.origin.debug(true);
    let before = windows.count;
    let (owned_input, original, copied, mut device, mut copied_device) = windows.measure(|| {
        let owned_input = black_box(input);
        let cpu = PrefetchCpu::from_prefetched(
            black_box(input.0),
            black_box(input.1),
            black_box(input.2),
        );
        let copied_cpu = cpu.clone();
        let device = expected.initial_device.clone();
        let copied_device = device.clone();
        (owned_input, cpu, copied_cpu, device, copied_device)
    });
    assert_eq!(input, owned_input);
    let mut cpu = original.unwrap();
    let mut copied_cpu = copied.unwrap();
    assert_eq!(cpu, copied_cpu);
    assert_eq!(device, expected.initial_device);
    assert_eq!(copied_device, expected.initial_device);
    assert_eq!(cpu.registers, seed.r);
    assert_eq!(cpu.clock().0, input.2.0);
    assert_eq!(cpu.queued_words(), Some(seed.queue));
    assert_eq!(format!("{cpu:?}"), initial_debug);
    assert_eq!(format!("{copied_cpu:?}"), initial_debug);
    let composed = expected.axis.trace && expected.axis.api == Api::Step;
    if composed {
        let result = windows
            .measure(|| cpu.step(&mut device))
            .map(Completion::Instruction);
        assert_eq!(result, expected.result());
        final_image(&cpu, &device, expected);
        assert_eq!(format!("{copied_cpu:?}"), initial_debug);
        assert_eq!(copied_device, expected.initial_device);
        let copied_result = windows
            .measure(|| copied_cpu.step(&mut copied_device))
            .map(Completion::Instruction);
        assert_eq!(copied_result, expected.result());
        final_image(&copied_cpu, &copied_device, expected);
    } else {
        let result = windows.measure(|| cpu.step_instruction(&mut device));
        assert_eq!(result, expected.origin.result());
        origin_image(&cpu, &device, expected);
        assert_eq!(format!("{copied_cpu:?}"), initial_debug);
        assert_eq!(copied_device, expected.initial_device);
        let copied_result = windows.measure(|| copied_cpu.step_instruction(&mut copied_device));
        assert_eq!(copied_result, expected.origin.result());
        origin_image(&copied_cpu, &copied_device, expected);
        assert_eq!(cpu, copied_cpu);
        assert_eq!(device, copied_device);
        if expected.axis.trace {
            assert_eq!(expected.axis.api, Api::Service);
            let origin_debug = expected.origin.debug(false);
            let result = windows
                .measure(|| cpu.service_pending_trace(&mut device))
                .map(Completion::Trace);
            assert_eq!(result, expected.result());
            final_image(&cpu, &device, expected);
            assert_eq!(format!("{copied_cpu:?}"), origin_debug);
            assert_eq!(copied_device, expected.origin_device);
            let copied_result = windows
                .measure(|| copied_cpu.service_pending_trace(&mut copied_device))
                .map(Completion::Trace);
            assert_eq!(copied_result, expected.result());
            final_image(&copied_cpu, &copied_device, expected);
        } else {
            for image in [&mut cpu, &mut copied_cpu] {
                assert!(image.interrupt_input().set_level(if expected.axis.edge {
                    7
                } else {
                    expected.axis.irq
                }));
                if expected.axis.edge {
                    assert!(image.interrupt_input().set_level(expected.axis.irq));
                }
            }
            let mut irq_input = expected.origin.clone();
            irq_input.initial.level = expected.axis.irq;
            irq_input.initial.edge = expected.axis.edge;
            let irq_debug = irq_input.debug(false);
            assert_eq!(format!("{cpu:?}"), irq_debug);
            assert_eq!(format!("{copied_cpu:?}"), irq_debug);
            let result = windows
                .measure(|| cpu.step_boundary(&mut device))
                .map(boundary);
            assert_eq!(result, expected.result());
            final_image(&cpu, &device, expected);
            assert_eq!(format!("{copied_cpu:?}"), irq_debug);
            assert_eq!(copied_device, expected.origin_device);
            let copied_result = windows
                .measure(|| copied_cpu.step_boundary(&mut copied_device))
                .map(boundary);
            assert_eq!(copied_result, expected.result());
            final_image(&copied_cpu, &copied_device, expected);
        }
    }
    assert_eq!(cpu, copied_cpu);
    assert_eq!(device, copied_device);
    if expected.error.is_some() {
        frozen(&mut cpu, &mut device, windows);
        frozen(&mut copied_cpu, &mut copied_device, windows);
        assert_eq!(cpu, copied_cpu);
        assert_eq!(device, copied_device);
    }
    let base_windows = if composed { 3 } else { 5 };
    let guard_windows = if expected.error.is_some() { 10 } else { 0 };
    assert_eq!(windows.count - before, base_windows + guard_windows);
    assert_eq!(input, owned_input);
}

#[test]
fn all_fault380160_requests4175088_windows() {
    positive_control();
    let mut windows = Windows::default();
    let (mut requests, mut composed, mut terminals) = (0, 0, 0);
    let mut counts = [0; 4];
    let mut terminal_counts = [0; 4];
    entry_axes::each(|axis| {
        let section = usize::from(axis.section - 29);
        counts[section] += 1;
        composed += usize::from(axis.trace && axis.api == Api::Step);
        let expected = entry_engine::predict(axis);
        assert!(expected.origin.importable && expected.origin.error.is_none());
        terminals += usize::from(expected.error.is_some());
        terminal_counts[section] += usize::from(expected.error.is_some());
        execute(&expected, &mut windows);
        requests += 1;
    });
    assert_eq!(counts, [58752, 19440, 6480, 295488]);
    assert_eq!(terminal_counts, [35120, 17752, 6480, 173952]);
    assert_eq!((composed, terminals), (29376, 233304));
    assert_eq!((requests, windows.count), (380160, 4175088));
    println!(
        "entry-fault requests={requests} windows={} delta=0 positive_allocator_control=PASS",
        windows.count
    );
}
