use crate::allocation_support::{Windows, positive_control};
use crate::device::Device;
use crate::entry_engine::{Completion, Expected};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu, TimedOutcome, TimingFault};

fn check(cpu: &PrefetchCpu, device: &Device, expected: &Expected) {
    assert_eq!(cpu.registers, expected.r);
    assert_eq!(cpu.clock().0, expected.clock);
    assert_eq!(cpu.queued_words(), Some(expected.queue));
    assert_eq!(cpu.trace_pending(), expected.trace);
    assert_eq!(format!("{cpu:?}"), expected.cpu_image().debug(false));
    assert_eq!(*device, expected.device);
}

fn prepare(
    expected: &Expected,
    windows: &mut Windows,
) -> (PrefetchCpu, Device, PrefetchCpu, Device) {
    let seed = &expected.origin.initial;
    let input = (seed.r, seed.queue, CpuClocks(seed.device.clock));
    let (cpu, device, copied_cpu, copied_device, owned_input) = windows.measure(|| {
        let owned_input = input;
        let cpu = PrefetchCpu::from_prefetched(input.0, input.1, input.2);
        let device = expected.initial_device.clone();
        let copied_cpu = cpu.clone();
        let copied_device = device.clone();
        (cpu, device, copied_cpu, copied_device, owned_input)
    });
    let cpu = cpu.unwrap();
    let copied_cpu = copied_cpu.unwrap();
    assert_eq!(input, owned_input);
    assert_eq!(cpu, copied_cpu);
    assert_eq!(device, copied_device);
    assert_eq!(format!("{cpu:?}"), expected.origin.debug(true));
    assert_eq!(format!("{copied_cpu:?}"), expected.origin.debug(true));
    (cpu, device, copied_cpu, copied_device)
}

fn boundary(
    result: Result<TimedOutcome, TimingFault<usize>>,
) -> Result<Completion, TimingFault<usize>> {
    result.map(|value| match value {
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
        _ => panic!("entry outcome"),
    })
}

fn origin(cpu: &mut PrefetchCpu, device: &mut Device, expected: &Expected, windows: &mut Windows) {
    let result = windows.measure(|| cpu.step_instruction(device));
    assert_eq!(result, expected.origin.result());
    assert_eq!(cpu.registers, expected.origin.r);
    assert_eq!(cpu.clock().0, expected.origin.device.clock);
    assert_eq!(cpu.queued_words(), Some(expected.origin.queue));
    assert_eq!(format!("{cpu:?}"), expected.origin.debug(false));
    assert_eq!(*device, expected.origin_device);
    if !expected.axis.trace {
        assert!(cpu.interrupt_input().set_level(if expected.axis.edge {
            7
        } else {
            expected.axis.irq
        }));
        if expected.axis.edge {
            assert!(cpu.interrupt_input().set_level(expected.axis.irq));
        }
    }
}

#[test]
fn all_alias124416_requests622080_windows() {
    positive_control();
    let mut windows = Windows::default();
    let mut count = 0;
    let mut terminal = 0;
    crate::entry_axes::each(|axis| {
        assert_eq!(axis.section, 39);
        let expected = crate::entry_engine::predict(axis);
        let (mut cpu, mut device, mut copy, mut copied_device) = prepare(&expected, &mut windows);
        origin(&mut cpu, &mut device, &expected, &mut windows);
        origin(&mut copy, &mut copied_device, &expected, &mut windows);
        let (result, copied) = if expected.axis.trace {
            let result = windows.measure(|| cpu.service_pending_trace(&mut device));
            let copied = windows.measure(|| copy.service_pending_trace(&mut copied_device));
            (result.map(Completion::Trace), copied.map(Completion::Trace))
        } else {
            let result = windows.measure(|| cpu.step_boundary(&mut device));
            let copied = windows.measure(|| copy.step_boundary(&mut copied_device));
            (boundary(result), boundary(copied))
        };
        assert_eq!(result, expected.result());
        assert_eq!(copied, expected.result());
        check(&cpu, &device, &expected);
        check(&copy, &copied_device, &expected);
        assert_eq!(cpu, copy);
        assert_eq!(device, copied_device);
        if expected.error.is_some() {
            guards(&mut cpu, &mut device, TimingFault::Frozen, &mut windows);
            guards(
                &mut copy,
                &mut copied_device,
                TimingFault::Frozen,
                &mut windows,
            );
            terminal += 1;
        }
        count += 1;
    });
    assert_eq!((count, terminal, windows.count), (124416, 0, 622080));
    println!(
        "alias requests={count} windows={} delta=0 positive_allocator_control=PASS",
        windows.count
    );
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
