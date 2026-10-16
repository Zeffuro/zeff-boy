use super::*;
use crate::timing::test_bus::{Bus, Event, cpu};

#[test]
fn trace_saves_completed_pc_and_postinstruction_status() {
    let mut cpu = cpu(0xa010);
    cpu.pipeline.as_mut().unwrap().words = [0x7080, 0x4e71];
    let mut bus = Bus::default();
    bus.handler(9);
    let outcome = cpu.step_boundary(&mut bus).unwrap();
    assert_eq!(
        outcome,
        TimedOutcome::Instruction(TimedStep {
            opcode: 0x7080,
            clocks: 38,
            exception: Some(9)
        })
    );
    assert_eq!(cpu.registers.d[0], 0xffff_ff80);
    assert_eq!(bus.words[&0xffe], 0x0102);
    assert_eq!(bus.words[&0xffa], 0xa018);
    assert_eq!(cpu.registers.status_register(), 0x2018);
    let times: Vec<_> = bus
        .events
        .iter()
        .filter_map(|event| match event {
            Event::Read(access, time) | Event::Write(access, _, time) => {
                Some((*time, access.bus_address))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        times,
        [
            (0, 0x340104),
            (8, 0xffe),
            (12, 0xffa),
            (16, 0xffc),
            (20, 36),
            (24, 38),
            (28, 0x345000),
            (34, 0x345002)
        ]
    );
}

#[test]
fn stopped_trace_uses_entry_latch_and_loaded_status() {
    for (old, new, trace) in [
        (0xa700, 0x2000, true),
        (0x2700, 0xa000, false),
        (0x2700, 0x2000, false),
        (0xa700, 0xa000, true),
    ] {
        let mut cpu = cpu(old);
        cpu.pipeline.as_mut().unwrap().words = [0x4e72, new];
        let mut bus = Bus::default();
        bus.handler(9);
        let step = cpu.step(&mut bus).unwrap();
        assert_eq!(step.clocks, if trace { 38 } else { 4 });
        assert_eq!(step.exception, if trace { Some(9) } else { None });
        assert_eq!(cpu.stopped(), !trace);
        if trace {
            assert_eq!(bus.words[&0xffa], new);
            assert_eq!(bus.words[&0xffe], 0x0104);
        } else {
            assert_eq!(bus.events, [Event::Idle(0, 4)]);
            assert_eq!(cpu.step_boundary(&mut bus).unwrap(), TimedOutcome::Stopped);
        }
    }
}

#[test]
fn short_traps_and_privilege_suppress_or_retain_trace_as_required() {
    for (opcode, sr, vector, saved, trace) in [
        (0x4afc, 0xa000, 4, 0x100, false),
        (0xa123, 0xa000, 10, 0x100, false),
        (0xf123, 0xa000, 11, 0x100, false),
        (0x4e72, 0x8000, 8, 0x100, false),
        (0x4e73, 0x8000, 8, 0x100, false),
        (0x4e40, 0x2000, 32, 0x102, false),
        (0x4e40, 0xa000, 32, 0x102, true),
    ] {
        let mut cpu = cpu(sr);
        cpu.pipeline.as_mut().unwrap().words = [opcode, 0x2345];
        let mut bus = Bus::default();
        bus.handler(vector);
        bus.handler(9);
        let step = cpu.step(&mut bus).unwrap();
        assert_eq!(step.clocks, if trace { 68 } else { 34 });
        assert_eq!(step.exception, Some(if trace { 9 } else { vector }));
        assert_eq!(bus.words[&0xffe], saved);
        assert_eq!(bus.words[&0xffa], sr);
        assert_eq!(cpu.registers.ssp, if trace { 0xff4 } else { 0xffa });
        assert!(!bus.events.iter().any(
            |event| matches!(event, Event::Read(access, _) if access.bus_address == 0x340102)
        ));
        if trace {
            assert_eq!(bus.words[&0xff8], 0x5000);
            assert_eq!(bus.words[&0xff4], 0x2000);
        }
    }
}

#[test]
fn rte_restores_bank_before_handler_fetch_and_delays_new_trace() {
    let mut cpu = cpu(0x2000);
    cpu.pipeline.as_mut().unwrap().words = [0x4e73, 0x4e71];
    let mut bus = Bus::default();
    bus.words.extend([
        (0x1000, 0x8011),
        (0x1002, 0),
        (0x1004, 0x3000),
        (0x3000, 0x4e71),
        (0x3002, 0x4e71),
    ]);
    let step = cpu.step(&mut bus).unwrap();
    assert_eq!(step.clocks, 20);
    assert_eq!(step.exception, None);
    assert_eq!(cpu.registers.pc, 0x3000);
    assert_eq!(cpu.registers.ssp, 0x1006);
    assert_eq!(cpu.registers.stack_pointer(), 0x2000);
    for (index, event) in bus.events.iter().enumerate() {
        let Event::Read(access, time) = event else {
            panic!("unexpected cycle")
        };
        assert_eq!(*time, index as u64 * 4);
        assert_eq!(
            access.function_code,
            if index < 3 {
                FunctionCode::SupervisorData
            } else {
                FunctionCode::UserProgram
            }
        );
    }
    bus.handler(9);
    assert_eq!(cpu.step(&mut bus).unwrap().exception, Some(9));
}

#[test]
fn rts_reads_active_stack_then_two_target_words() {
    for supervisor in [0, SUPERVISOR] {
        let mut cpu = cpu(supervisor);
        cpu.pipeline.as_mut().unwrap().words = [0x4e75, 0x4e71];
        let sp = cpu.registers.stack_pointer();
        let mut bus = Bus::default();
        bus.words.extend([(sp, 0), (sp + 2, 0x3000)]);
        assert_eq!(cpu.step(&mut bus).unwrap().clocks, 16);
        assert_eq!(cpu.registers.stack_pointer(), sp + 4);
        let addresses: Vec<_> = bus
            .events
            .iter()
            .map(|event| match event {
                Event::Read(access, _) => access.logical_address,
                _ => panic!("unexpected idle"),
            })
            .collect();
        assert_eq!(addresses, [sp, sp + 2, 0x3000, 0x3002]);
    }
}

#[test]
fn status_changes_refetch_with_new_function_code_and_entry_trace() {
    for (opcode, immediate, clocks, first_target) in [
        (0x027c, 0x7fff, 54, 0x104),
        (0x46fc, 0, 50, 0x104),
        (0x46c0, 0, 46, 0x102),
    ] {
        let mut cpu = cpu(0xa000);
        cpu.pipeline.as_mut().unwrap().words = [opcode, immediate];
        let mut bus = Bus::default();
        bus.handler(9);
        let step = cpu.step(&mut bus).unwrap();
        assert_eq!(step.clocks, clocks);
        assert_eq!(step.exception, Some(9));
        assert_eq!(bus.words[&0xffe], first_target);
        assert_eq!(bus.words[&0xffa], if opcode == 0x027c { 0x2000 } else { 0 });
        let prefetched: Vec<_> = bus
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Read(access, _) if access.kind == AccessKind::Prefetch => {
                    Some((access.logical_address, access.function_code))
                }
                _ => None,
            })
            .collect();
        let expected_fc = if opcode == 0x027c {
            FunctionCode::SupervisorProgram
        } else {
            FunctionCode::UserProgram
        };
        assert!(prefetched.contains(&(0x1234_0000 | u32::from(first_target), expected_fc)));
    }
}

#[test]
fn aborted_refill_cancels_trace_and_retains_completed_external_effects() {
    let mut cpu = cpu(0xa000);
    let mut bus = Bus {
        fail: Some(0),
        ..Bus::default()
    };
    assert!(matches!(cpu.step(&mut bus), Err(TimingFault::Bus { .. })));
    assert_eq!(bus.transfers, 1);
    assert_eq!(cpu.step_boundary(&mut bus), Err(TimingFault::Frozen));
}

#[test]
fn every_short_exception_transfer_failure_preserves_elapsed_effects() {
    for index in 0..7 {
        let mut cpu = cpu(0x2000);
        cpu.pipeline.as_mut().unwrap().words[0] = 0x4e40;
        let mut bus = Bus {
            wait: 2,
            fail: Some(index),
            ..Bus::default()
        };
        bus.handler(32);
        assert!(
            matches!(cpu.step(&mut bus), Err(TimingFault::Bus { source, .. }) if source == index)
        );
        assert_eq!(cpu.clock().0, bus.clock);
        assert_eq!(bus.transfers, index + 1);
        let events = bus.events.clone();
        assert_eq!(cpu.step_boundary(&mut bus), Err(TimingFault::Frozen));
        assert_eq!(bus.events, events);
        if index > 0 {
            assert_eq!(bus.words[&0xffe], 0x0102);
        }
    }
}

#[test]
fn status_setters_intentionally_refetch_old_address_after_fc_change() {
    for (opcode, old, immediate, expected, clocks) in [
        (0x007c, 0x2000, 0x801f, 0xa01f, 20),
        (0x027c, 0x201f, 0x0011, 0x0011, 20),
        (0x0a7c, 0x2000, 0x2003, 0x0003, 20),
        (0x46fc, 0x2000, 0x001f, 0x001f, 16),
    ] {
        let mut cpu = cpu(old);
        cpu.pipeline.as_mut().unwrap().words = [opcode, immediate];
        let mut bus = Bus::default();
        let step = cpu.step(&mut bus).unwrap();
        assert_eq!(step.clocks, clocks);
        assert_eq!(step.exception, None);
        assert_eq!(cpu.registers.status_register(), expected);
        let reads: Vec<_> = bus
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Read(access, _) => Some((access.logical_address, access.function_code)),
                _ => None,
            })
            .collect();
        let new_fc = if expected & SUPERVISOR != 0 {
            FunctionCode::SupervisorProgram
        } else {
            FunctionCode::UserProgram
        };
        assert_eq!(
            reads,
            [
                (0x1234_0104, FunctionCode::SupervisorProgram),
                (0x1234_0104, new_fc),
                (0x1234_0106, new_fc)
            ]
        );
    }
}

#[test]
fn privilege_violation_never_reads_sr_operands() {
    for opcode in [0x007c, 0x027c, 0x0a7c, 0x46fc, 0x46c0, 0x46c7] {
        let mut cpu = cpu(0);
        cpu.pipeline.as_mut().unwrap().words = [opcode, 0xffff];
        let mut bus = Bus::default();
        bus.handler(8);
        let step = cpu.step(&mut bus).unwrap();
        assert_eq!(step.clocks, 34);
        assert_eq!(step.exception, Some(8));
        assert!(!bus.events.iter().any(|event| matches!(event, Event::Read(access, _) if (0x340100..0x340108).contains(&access.bus_address))));
    }
}

#[test]
fn idle_overflow_freezes_after_status_commit_and_never_replays() {
    let mut cpu = cpu(0x2000);
    cpu.pipeline.as_mut().unwrap().words[0] = 0x46c0;
    cpu.registers.d[0] = 0x0011;
    cpu.clock = CpuClocks(u64::MAX - 3);
    let mut bus = Bus {
        clock: cpu.clock.0,
        ..Bus::default()
    };
    assert_eq!(cpu.step(&mut bus), Err(TimingFault::ClockOverflow));
    assert_eq!(cpu.registers.status_register(), 0x0011);
    assert_eq!(bus.events, [Event::Idle(u64::MAX - 3, 2)]);
    assert_eq!(cpu.step(&mut bus), Err(TimingFault::Frozen));
    assert_eq!(bus.events.len(), 1);
}

#[test]
fn return_refill_host_failures_retain_completed_stack_and_status_changes() {
    for opcode in [0x4e73, 0x4e75] {
        let mut cpu = cpu(0x2000);
        cpu.pipeline.as_mut().unwrap().words[0] = opcode;
        let rte = opcode == 0x4e73;
        let mut bus = Bus {
            fail: Some(if rte { 3 } else { 2 }),
            ..Bus::default()
        };
        if rte {
            bus.words
                .extend([(0x1000, 0x0011), (0x1002, 0), (0x1004, 0x3000)]);
        } else {
            bus.words.extend([(0x1000, 0), (0x1002, 0x3000)]);
        }
        assert!(matches!(cpu.step(&mut bus), Err(TimingFault::Bus { .. })));
        assert_eq!(cpu.registers.ssp, if rte { 0x1006 } else { 0x1004 });
        assert_eq!(
            cpu.registers.status_register(),
            if rte { 0x0011 } else { 0x2000 }
        );
        assert_eq!(cpu.registers.pc, 0x1234_0100);
        assert_eq!(cpu.step(&mut bus), Err(TimingFault::Frozen));
    }
}

#[test]
fn repeated_status_refetch_uses_the_second_sampled_word() {
    for opcode in [0x027c, 0x46fc] {
        let mut cpu = cpu(0x2000);
        cpu.pipeline.as_mut().unwrap().words = [opcode, 0x0011];
        let mut bus = Bus {
            read_values: vec![(0, 0x7001), (1, 0x7202), (2, 0x7403)],
            ..Bus::default()
        };
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.queued_words(), Some([0x7202, 0x7403]));
        assert_eq!(
            cpu.registers.status_register(),
            if opcode == 0x027c { 0 } else { 0x0011 }
        );
    }
}

#[test]
fn control_routes_fail_at_every_transfer_without_replay() {
    for (opcode, transfers) in [
        (0x4e73, 5),
        (0x4e75, 4),
        (0x007c, 3),
        (0x027c, 3),
        (0x0a7c, 3),
        (0x46fc, 3),
        (0x46c0, 2),
    ] {
        for index in 0..transfers {
            let mut cpu = cpu(0x2000);
            cpu.pipeline.as_mut().unwrap().words = [opcode, 0x0011];
            let mut bus = Bus {
                fail: Some(index),
                wait: 1,
                ..Bus::default()
            };
            bus.words.extend([
                (0x1000, if opcode == 0x4e73 { 0x0011 } else { 0 }),
                (0x1002, if opcode == 0x4e73 { 0 } else { 0x3000 }),
                (0x1004, 0x3000),
            ]);
            assert!(
                matches!(cpu.step(&mut bus), Err(TimingFault::Bus { source, .. }) if source == index)
            );
            assert_eq!(cpu.clock().0, bus.clock);
            assert_eq!(bus.transfers, index + 1);
            let events = bus.events.clone();
            assert_eq!(cpu.step(&mut bus), Err(TimingFault::Frozen));
            assert_eq!(bus.events, events);
        }
    }
}

#[test]
fn traced_branch_finishes_target_refills_before_trace_vector_reads() {
    for (opcode, flags, normal_clocks) in [(0x6006, 0, 10), (0x6706, 0, 8), (0x6700, 0, 12)] {
        let mut cpu = cpu(0xa000 | flags);
        cpu.pipeline.as_mut().unwrap().words = [opcode, 6];
        let mut bus = Bus::default();
        bus.handler(9);
        let step = cpu.step(&mut bus).unwrap();
        assert_eq!(step.clocks, normal_clocks + 34);
        assert_eq!(step.exception, Some(9));
        assert_eq!(
            bus.words[&0xffe],
            if opcode == 0x6006 {
                0x0108
            } else if opcode as u8 == 0 {
                0x0104
            } else {
                0x0102
            }
        );
        let first_stack = bus
            .events
            .iter()
            .position(|event| matches!(event, Event::Write(_, _, _)))
            .unwrap();
        assert!(bus.events[..first_stack].iter().any(
            |event| matches!(event, Event::Read(access, _) if access.kind == AccessKind::Prefetch)
        ));
    }
}

#[test]
fn trap_trace_and_next_boundary_interrupt_stack_three_distinct_entries() {
    let mut cpu = cpu(0xa000);
    cpu.pipeline.as_mut().unwrap().words[0] = 0x4e40;
    let mut bus = Bus::default();
    bus.handler(32);
    bus.handler(9);
    bus.handler(27);
    cpu.interrupt_input().set_level(3);
    let step = cpu.step(&mut bus).unwrap();
    assert_eq!(step.clocks, 68);
    assert!(
        !bus.events
            .iter()
            .any(|event| matches!(event, Event::Ack(_, _)))
    );
    assert_eq!(cpu.registers.ssp, 0xff4);
    assert_eq!(
        cpu.step_boundary(&mut bus).unwrap(),
        TimedOutcome::Interrupt {
            level: 3,
            vector: 27,
            clocks: 44
        }
    );
    assert_eq!(cpu.registers.ssp, 0xfee);
    assert_eq!(bus.words[&0xffa], 0xa000);
    assert_eq!(bus.words[&0xff4], 0x2000);
    assert_eq!(bus.words[&0xfee], 0x2000);
    assert_eq!(cpu.registers.status_register(), 0x2300);
}

#[test]
fn instruction_prefix_guard_clone_and_trace_priority_survive_stopped_state() {
    let mut original = cpu(0xa000);
    original.pipeline.as_mut().unwrap().words = [0x4e72, 0x2000];
    let mut bus = Bus::default();
    bus.handler(9);
    bus.handler(27);
    assert_eq!(original.step_instruction(&mut bus).unwrap().clocks, 4);
    assert!(original.stopped());
    assert!(original.trace_pending());
    assert_eq!(
        original.step_instruction(&mut bus),
        Err(TimingFault::PendingTrace)
    );
    assert_eq!(bus.events.len(), 1);
    let mut restored = original.clone();
    original.interrupt_input().set_level(3);
    restored.interrupt_input().set_level(3);
    let mut second_bus = Bus {
        clock: bus.clock,
        ..Bus::default()
    };
    second_bus.handler(9);
    second_bus.handler(27);
    assert_eq!(
        original.step_boundary(&mut bus),
        Ok(TimedOutcome::Trace { clocks: 34 })
    );
    assert_eq!(
        restored.step_boundary(&mut second_bus),
        Ok(TimedOutcome::Trace { clocks: 34 })
    );
    assert_eq!(original, restored);
    assert!(!original.stopped());
    assert!(!original.trace_pending());
    assert!(
        !bus.events
            .iter()
            .any(|event| matches!(event, Event::Ack(_, _)))
    );
    assert_eq!(
        original.step_boundary(&mut bus),
        Ok(TimedOutcome::Interrupt {
            level: 3,
            vector: 27,
            clocks: 44
        })
    );
}

#[test]
fn separate_trace_entry_faults_freeze_at_each_transfer() {
    for index in 1..8 {
        let mut cpu = cpu(0xa000);
        let mut bus = Bus {
            fail: Some(index),
            wait: 1,
            ..Bus::default()
        };
        bus.handler(9);
        cpu.step_instruction(&mut bus).unwrap();
        assert!(cpu.trace_pending());
        assert!(
            matches!(cpu.service_pending_trace(&mut bus), Err(TimingFault::Bus { source, .. }) if source == index)
        );
        assert_eq!(cpu.clock().0, bus.clock);
        assert_eq!(bus.transfers, index + 1);
        let events = bus.events.clone();
        assert_eq!(cpu.step_boundary(&mut bus), Err(TimingFault::Frozen));
        assert_eq!(bus.events, events);
    }
}
