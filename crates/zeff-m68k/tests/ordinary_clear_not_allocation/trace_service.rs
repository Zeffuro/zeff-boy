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

fn origin(cpu: &mut PrefetchCpu, device: &mut Device, expected: &Expected, windows: &mut Windows) {
    use crate::entry_axes::Api;
    let actual = match expected.axis.api {
        Api::Instruction => windows
            .measure(|| cpu.step_instruction(device))
            .map(Completion::Origin),
        Api::Boundary => {
            let result = windows.measure(|| cpu.step_boundary(device));
            result.map(|value| match value {
                TimedOutcome::Instruction(step) => Completion::Instruction(step),
                _ => panic!("trace origin outcome"),
            })
        }
        Api::Step => windows
            .measure(|| cpu.step(device))
            .map(Completion::Instruction),
        Api::Service => panic!("origin API"),
    };
    assert_eq!(actual, expected.result());
    check(cpu, device, expected);
}

#[test]
fn all_trace_service20736_requests108480_windows() {
    positive_control();
    let mut windows = Windows::default();
    let mut count = 0;
    let mut terminal = 0;
    let mut outcomes = [0; 4];
    crate::entry_axes::each(|axis| {
        let expected = crate::post::predict(&axis.value);
        let (mut cpu, mut device, mut copy, mut copied_device) =
            prepare(&expected.before, &mut windows);
        origin(&mut cpu, &mut device, &expected.before, &mut windows);
        origin(
            &mut copy,
            &mut copied_device,
            &expected.before,
            &mut windows,
        );
        let before_cpu = cpu.clone();
        let before_device = device.clone();
        let result = windows.measure(|| cpu.service_pending_trace(&mut device));
        let copied = windows.measure(|| copy.service_pending_trace(&mut copied_device));
        assert_eq!(result, expected.result);
        assert_eq!(copied, expected.result);
        check(&cpu, &device, &expected.after);
        check(&copy, &copied_device, &expected.after);
        assert_eq!(cpu, copy);
        assert_eq!(device, copied_device);
        if expected.unchanged {
            assert_eq!(cpu, before_cpu);
            assert_eq!(device, before_device);
        }
        outcomes[match expected.outcome {
            "Some" => 0,
            "None" => 1,
            "AddressError" => 2,
            "Frozen" => 3,
            _ => panic!("outcome"),
        }] += 1;
        if expected.after.error.is_some() {
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
    assert_eq!(outcomes, [6752, 13504, 160, 320]);
    assert_eq!((count, terminal, windows.count), (20736, 480, 108480));
    println!(
        "trace-service requests={count} windows={} delta=0 positive_allocator_control=PASS",
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
