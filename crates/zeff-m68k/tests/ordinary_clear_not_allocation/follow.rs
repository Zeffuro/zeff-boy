use crate::allocation_support::{Windows, positive_control};
use crate::device::Device;
use crate::follow_axes::Api;
use crate::follow_engine::Expected;
use zeff_m68k::timing::{CpuClocks, PrefetchCpu, TimedOutcome, TimedStep, TimingFault};

fn prepare(
    expected: &Expected,
    windows: &mut Windows,
) -> (PrefetchCpu, Device, PrefetchCpu, Device) {
    let initial = &expected.origin.initial;
    let input = (initial.r, initial.queue, CpuClocks(initial.device.clock));
    let (cpu, device, copied_cpu, copied_device, owned) = windows.measure(|| {
        let owned = input;
        let cpu = PrefetchCpu::from_prefetched(input.0, input.1, input.2);
        let device = expected.initial_device.clone();
        let copy = cpu.clone();
        let copied_device = device.clone();
        (cpu, device, copy, copied_device, owned)
    });
    let cpu = cpu.unwrap();
    let copied_cpu = copied_cpu.unwrap();
    assert_eq!(input, owned);
    assert_eq!(cpu, copied_cpu);
    assert_eq!(device, copied_device);
    assert_eq!(format!("{cpu:?}"), expected.origin.debug(true));
    assert_eq!(format!("{copied_cpu:?}"), expected.origin.debug(true));
    (cpu, device, copied_cpu, copied_device)
}

fn origin(cpu: &mut PrefetchCpu, device: &mut Device, expected: &Expected, windows: &mut Windows) {
    let result = windows.measure(|| cpu.step_instruction(device));
    assert_eq!(result, expected.origin.result());
    assert_eq!(cpu.registers, expected.origin.r);
    assert_eq!(cpu.clock().0, expected.origin.device.clock);
    assert_eq!(cpu.queued_words(), Some(expected.origin.queue));
    assert_eq!(format!("{cpu:?}"), expected.origin.debug(false));
    assert_eq!(*device, expected.origin_device);
}

fn next(
    cpu: &mut PrefetchCpu,
    device: &mut Device,
    api: Api,
    windows: &mut Windows,
) -> Result<TimedStep, TimingFault<usize>> {
    match api {
        Api::Instruction => windows.measure(|| cpu.step_instruction(device)),
        Api::Step => windows.measure(|| cpu.step(device)),
        Api::Boundary => {
            let result = windows.measure(|| cpu.step_boundary(device));
            result.map(|outcome| match outcome {
                TimedOutcome::Instruction(step) => step,
                _ => panic!("next boundary outcome"),
            })
        }
    }
}

fn check(cpu: &PrefetchCpu, device: &Device, expected: &Expected) {
    assert_eq!(cpu.registers, expected.r);
    assert_eq!(cpu.clock().0, expected.device.clock);
    assert_eq!(cpu.queued_words(), expected.queue);
    assert_eq!(cpu.stopped(), expected.stopped);
    assert!(!cpu.trace_pending());
    assert_eq!(format!("{cpu:?}"), expected.debug());
    assert_eq!(*device, expected.device);
}

#[test]
fn all_follow6480_requests33000_windows() {
    positive_control();
    let mut windows = Windows::default();
    let mut count = 0;
    let mut faults = 0;
    let mut stopped = 0;
    crate::follow_axes::each(|axis| {
        let expected = crate::follow_engine::predict(axis);
        let (mut cpu, mut device, mut copy, mut copied_device) = prepare(&expected, &mut windows);
        origin(&mut cpu, &mut device, &expected, &mut windows);
        origin(&mut copy, &mut copied_device, &expected, &mut windows);
        let result = next(&mut cpu, &mut device, expected.axis.api, &mut windows);
        let copied = next(
            &mut copy,
            &mut copied_device,
            expected.axis.api,
            &mut windows,
        );
        assert_eq!(result, expected.result());
        assert_eq!(copied, expected.result());
        check(&cpu, &device, &expected);
        check(&copy, &copied_device, &expected);
        assert_eq!(cpu, copy);
        assert_eq!(device, copied_device);
        stopped += usize::from(expected.stopped);
        if expected.error.is_some() {
            guards(&mut cpu, &mut device, TimingFault::Frozen, &mut windows);
            guards(
                &mut copy,
                &mut copied_device,
                TimingFault::Frozen,
                &mut windows,
            );
            faults += 1;
        }
        count += 1;
    });
    assert_eq!(
        (count, faults, stopped, windows.count),
        (6480, 60, 648, 33000)
    );
    println!(
        "follow requests={count} windows={} delta=0 positive_allocator_control=PASS",
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
