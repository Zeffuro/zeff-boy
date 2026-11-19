use crate::allocation_support::{Windows, positive_control};
use crate::reset_axes;
use crate::reset_device::{DefaultDetector, Device};
use crate::reset_engine::{self, Cpu, Expected};
use zeff_m68k::timing::physical::{
    EntryFault, EntryStep, ExternalResetBoundary, PhysicalEntryState,
};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu, TimingFault};

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

fn input(cpu: &mut PrefetchCpu, device: &mut Device, expected: &Expected) {
    assert!(cpu.interrupt_input().set_level(if expected.input_cpu.edge {
        7
    } else {
        expected.input_cpu.level
    }));
    if expected.input_cpu.edge {
        assert!(cpu.interrupt_input().set_level(expected.input_cpu.level));
    }
    device.ordinary.clock = expected.boundary.sampled_release.0;
    assert!(expected.input_cpu.matches(cpu));
    assert_eq!(*device, expected.input_device);
}

fn reset(
    cpu: &mut PrefetchCpu,
    device: &mut Device,
    boundary: ExternalResetBoundary,
    omitted: bool,
) -> Result<EntryStep, EntryFault<usize>> {
    if omitted {
        cpu.external_warm_reset(&mut DefaultDetector(device), boundary)
    } else {
        cpu.external_warm_reset(device, boundary)
    }
}

fn execute(expected: &Expected, windows: &mut Windows) -> (bool, bool, bool) {
    let seed = &expected.origin.initial;
    let initial = (seed.r, seed.queue, CpuClocks(seed.device.clock));
    let mut device = expected.initial_device.clone();
    let (cpu, copied_cpu, mut copied_device, owned_input) = windows.measure(|| {
        let owned_input = initial;
        let cpu = PrefetchCpu::from_prefetched(initial.0, initial.1, initial.2);
        let copied_cpu = cpu.clone();
        let copied_device = device.clone();
        (cpu, copied_cpu, copied_device, owned_input)
    });
    let mut cpu = cpu.unwrap();
    let mut copied_cpu = copied_cpu.unwrap();
    assert_eq!(initial, owned_input);
    assert_eq!(cpu, copied_cpu);
    assert_eq!(device, copied_device);
    assert_eq!(format!("{cpu:?}"), expected.origin.debug(true));
    assert_eq!(format!("{copied_cpu:?}"), expected.origin.debug(true));
    let original = windows.measure(|| cpu.step_instruction(&mut device));
    let copied = windows.measure(|| copied_cpu.step_instruction(&mut copied_device));
    assert_eq!(original, expected.origin.result());
    assert_eq!(copied, expected.origin.result());
    let origin_cpu = Cpu::origin(&expected.origin);
    assert!(origin_cpu.matches(&cpu));
    assert!(origin_cpu.matches(&copied_cpu));
    assert_eq!(cpu, copied_cpu);
    assert_eq!(device, copied_device);
    assert_eq!(device, expected.origin_device);
    input(&mut cpu, &mut device, expected);
    input(&mut copied_cpu, &mut copied_device, expected);
    assert_eq!(cpu, copied_cpu);
    assert_eq!(device, copied_device);
    let boundary = expected.boundary;
    let omitted = device.detector.omitted;
    let copied_omitted = copied_device.detector.omitted;
    let result = windows.measure(|| reset(&mut cpu, &mut device, boundary, omitted));
    device.record_default(&result);
    let copied = windows.measure(|| {
        reset(
            &mut copied_cpu,
            &mut copied_device,
            boundary,
            copied_omitted,
        )
    });
    copied_device.record_default(&copied);
    assert_eq!(result, expected.result);
    assert_eq!(copied, expected.result);
    assert!(expected.cpu.matches(&cpu));
    assert!(expected.cpu.matches(&copied_cpu));
    assert_eq!(cpu, copied_cpu);
    assert_eq!(device, copied_device);
    assert_eq!(device, expected.device);
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
    let terminal = fault.is_some();
    if let Some(fault) = fault {
        guards(&mut cpu, &mut device, fault.clone(), windows);
        guards(&mut copied_cpu, &mut copied_device, fault, windows);
        assert_eq!(cpu, copied_cpu);
        assert_eq!(device, copied_device);
    }
    (
        terminal,
        matches!(result, Err(EntryFault::Input(_))),
        result.is_ok(),
    )
}

#[test]
fn all_reset_134128_requests_1869360_windows() {
    positive_control();
    let mut windows = Windows::default();
    let mut counts = [0; 6];
    let mut terminals = [0; 6];
    let mut input_rejected = 0;
    let mut successes = 0;
    reset_axes::each(|axis, request| {
        let section = axis.get("section").number() as usize - 33;
        let expected = reset_engine::predict(axis, request);
        let (terminal, rejected, success) = execute(&expected, &mut windows);
        counts[section] += 1;
        terminals[section] += usize::from(terminal);
        input_rejected += usize::from(rejected);
        successes += usize::from(success);
    });
    assert_eq!(counts, [71712, 15936, 3984, 23904, 7968, 10624]);
    assert_eq!(terminals, [63936, 15072, 2688, 23040, 7968, 7168]);
    assert_eq!((input_rejected, successes), (864, 13392));
    assert_eq!(
        (
            counts.iter().sum::<usize>(),
            terminals.iter().sum::<usize>(),
            windows.count
        ),
        (134128, 119872, 1869360)
    );
    println!(
        "reset requests=134128 windows={} delta=0 positive_allocator_control=PASS",
        windows.count
    );
}
