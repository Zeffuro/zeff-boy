use crate::allocation_support::{Windows, positive_control};
use crate::binding::{b, n, object, s};
use crate::device::Device;
use crate::oracle::{self, Expected};
use crate::seed::{self, Axis, RESERVE};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu, TimingFault};

fn axes() -> Vec<Axis> {
    let contexts = seed::contexts();
    let mut rows = contexts.clone();
    for section in ["callback_Host", "blocked_transfer"] {
        for base in &contexts {
            let initial = seed::build(base);
            for phase in initial.device.phases[..initial.device.phase_count]
                .iter()
                .flatten()
            {
                let mut axis = base.clone();
                axis.section = section;
                axis.phase = Some(phase);
                axis.room = if section == "blocked_transfer" {
                    RESERVE - 1
                } else {
                    0
                };
                rows.push(axis);
            }
        }
    }
    assert_eq!(rows.len(), 2944);
    for base in &contexts {
        let mode = base.word >> 3 & 7;
        if matches!(mode, 4 | 6) {
            for room in 0u64..2 {
                rows.push(Axis::from_literal(&object(vec![
                    ("word", n(base.word)),
                    ("bank", n(base.bank)),
                    ("lane", n(base.lane)),
                    ("section", s("default_completion")),
                    ("start", n(u64::MAX - room)),
                ])));
            }
        }
    }
    assert_eq!(rows.len(), 3200);
    for base in &contexts {
        if base.word >> 6 & 3 == 2 {
            for room in 0..2 {
                let mut axis = base.clone();
                axis.section = "Long_tail";
                axis.room = room;
                rows.push(axis);
            }
        }
    }
    assert_eq!(rows.len(), 3264);
    for base in &contexts {
        if base.word >> 3 & 7 != 0 {
            rows.push(Axis::from_literal(&object(vec![
                ("word", n(base.word)),
                ("bank", n(base.bank)),
                ("lane", n(base.lane)),
                ("section", s("callback_Host")),
                ("fail_phase", s("W")),
                ("failed_write_deposit", b(true)),
            ])));
        }
    }
    assert_eq!(rows.len(), 3600);
    rows
}

fn frozen(cpu: &mut PrefetchCpu, bus: &mut Device, windows: &mut Windows) {
    let saved_cpu = cpu.clone();
    let saved_bus = bus.clone();
    assert!(matches!(
        windows.measure(|| cpu.step_instruction(bus)),
        Err(TimingFault::Frozen)
    ));
    assert_eq!(*cpu, saved_cpu);
    assert_eq!(*bus, saved_bus);
    assert!(matches!(
        windows.measure(|| cpu.step_boundary(bus)),
        Err(TimingFault::Frozen)
    ));
    assert_eq!(*cpu, saved_cpu);
    assert_eq!(*bus, saved_bus);
    assert!(matches!(
        windows.measure(|| cpu.prime(bus)),
        Err(TimingFault::Frozen)
    ));
    assert_eq!(*cpu, saved_cpu);
    assert_eq!(*bus, saved_bus);
    assert!(matches!(
        windows.measure(|| cpu.service_pending_trace(bus)),
        Err(TimingFault::Frozen)
    ));
    assert_eq!(*cpu, saved_cpu);
    assert_eq!(*bus, saved_bus);
    assert!(matches!(
        windows.measure(|| cpu.step(bus)),
        Err(TimingFault::Frozen)
    ));
    assert_eq!(*cpu, saved_cpu);
    assert_eq!(*bus, saved_bus);
}

fn execute(expected: &Expected, windows: &mut Windows) {
    let initial = &expected.initial;
    let input = (initial.r, initial.queue, CpuClocks(initial.device.clock));
    let mut bus = initial.device.clone();
    let (cpu, copied_cpu, mut copied_bus, owned_input) = windows.measure(|| {
        let owned_input = input;
        let cpu = PrefetchCpu::from_prefetched(input.0, input.1, input.2);
        let copied_cpu = cpu.clone();
        let copied_bus = bus.clone();
        (cpu, copied_cpu, copied_bus, owned_input)
    });
    let mut cpu = cpu.unwrap();
    let mut copied_cpu = copied_cpu.unwrap();
    assert_eq!(owned_input, input);
    assert_eq!(cpu, copied_cpu);
    assert_eq!(bus, copied_bus);
    assert_eq!(format!("{cpu:?}"), expected.debug(true));
    assert_eq!(format!("{copied_cpu:?}"), expected.debug(true));
    let result = windows.measure(|| cpu.step_instruction(&mut bus));
    let copied = windows.measure(|| copied_cpu.step_instruction(&mut copied_bus));
    assert_eq!(result, expected.result());
    assert_eq!(copied, expected.result());
    assert_eq!(cpu, copied_cpu);
    assert_eq!(bus, copied_bus);
    assert_eq!(bus, expected.device);
    assert_eq!(cpu.registers, expected.r);
    assert_eq!(cpu.clock().0, expected.device.clock);
    assert_eq!(cpu.queued_words(), Some(expected.queue));
    assert_eq!(format!("{cpu:?}"), expected.debug(false));
    assert_eq!(format!("{copied_cpu:?}"), expected.debug(false));
    if expected.error.is_some() {
        frozen(&mut cpu, &mut bus, windows);
        frozen(&mut copied_cpu, &mut copied_bus, windows);
        assert_eq!(cpu, copied_cpu);
        assert_eq!(bus, copied_bus);
    }
}

#[test]
fn all_declared_primary_360000_requests_4248000_windows() {
    positive_control();
    let rows = axes();
    let expected = rows
        .into_iter()
        .map(|axis| oracle::predict(seed::build(&axis)))
        .collect::<Vec<_>>();
    assert_eq!(
        expected.iter().filter(|row| row.error.is_some()).count(),
        3168
    );
    assert!(expected.iter().all(|row| row.importable));
    let mut windows = Windows::default();
    let mut requests = 0;
    for _ in 0..100 {
        for row in &expected {
            execute(row, &mut windows);
            requests += 1;
        }
    }
    assert_eq!((requests, windows.count), (360000, 4248000));
    println!(
        "primary requests={requests} windows={} delta=0 positive_allocator_control=PASS",
        windows.count
    );
}
