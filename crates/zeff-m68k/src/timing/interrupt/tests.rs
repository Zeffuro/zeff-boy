use super::*;
use crate::timing::test_bus::{Bus, Event, cpu};

#[test]
fn interrupt_frame_acknowledge_and_refill_order() {
    let mut cpu = cpu(0x0015);
    cpu.interrupt_input().set_level(4);
    let mut bus = Bus::default();
    bus.handler(28);
    let outcome = cpu.step_boundary(&mut bus).unwrap();
    assert_eq!(
        outcome,
        TimedOutcome::Interrupt {
            level: 4,
            vector: 28,
            clocks: 44
        }
    );
    assert_eq!(cpu.registers.pc, 0x1234_5000);
    assert_eq!(cpu.registers.status_register(), 0x2415);
    assert_eq!(cpu.registers.ssp, 0xffa);
    assert_eq!(cpu.registers.usp, 0x2000);
    assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
    assert_eq!(bus.words[&0xffe], 0x0100);
    assert_eq!(bus.words[&0xffa], 0x0015);
    assert_eq!(bus.words[&0xffc], 0x1234);
    let expected = [
        (6, 0xffe),
        (10, 0xfffff8),
        (18, 0xffa),
        (22, 0xffc),
        (26, 112),
        (30, 114),
        (34, 0x345000),
        (40, 0x345002),
    ];
    let actual: Vec<_> = bus
        .events
        .iter()
        .filter_map(|event| match event {
            Event::Read(access, clock)
            | Event::Write(access, _, clock)
            | Event::Ack(access, clock) => Some((*clock, access.bus_address)),
            _ => None,
        })
        .collect();
    assert_eq!(actual, expected);
    for event in &bus.events {
        match event {
            Event::Ack(access, _) => {
                assert_eq!(access.function_code, FunctionCode::CpuSpace);
                assert_eq!(access.lanes, ByteLanes::Both);
                assert_eq!(access.logical_address, 0xffff_fff8);
            }
            Event::Write(access, _, _) => {
                assert_eq!(access.function_code, FunctionCode::SupervisorData)
            }
            Event::Read(access, _) => assert_eq!(
                access.function_code,
                if access.kind == AccessKind::Vector {
                    FunctionCode::SupervisorData
                } else {
                    FunctionCode::SupervisorProgram
                }
            ),
            _ => {}
        }
    }
}

#[test]
fn levels_masks_modes_and_reply_vectors() {
    for level in 1..=7 {
        for mask in 0..=7 {
            for supervisor in [0, SUPERVISOR] {
                for reply in [
                    InterruptVector::Autovector,
                    InterruptVector::Spurious,
                    InterruptVector::Vector(0),
                    InterruptVector::Vector(255),
                ] {
                    let mut cpu = cpu(supervisor | (u16::from(mask) << 8));
                    cpu.interrupt_input().set_level(level);
                    let vector = match reply {
                        InterruptVector::Autovector => 24 + level,
                        InterruptVector::Spurious => 24,
                        InterruptVector::Vector(value) => value,
                    };
                    let mut bus = Bus {
                        response: Some(reply),
                        ..Bus::default()
                    };
                    bus.handler(vector);
                    let outcome = cpu.step_boundary(&mut bus).unwrap();
                    if level == 7 || level > mask {
                        assert_eq!(
                            outcome,
                            TimedOutcome::Interrupt {
                                level,
                                vector,
                                clocks: 44
                            }
                        );
                    } else {
                        assert!(matches!(outcome, TimedOutcome::Instruction(_)));
                        assert_eq!(bus.transfers, 1);
                    }
                }
            }
        }
    }
}

#[test]
fn level7_edge_is_one_shot_but_level_comparison_can_retrigger() {
    let mut cpu = cpu(0x2700);
    let mut bus = Bus::default();
    bus.handler(31);
    assert!(cpu.interrupt_input().set_level(7));
    assert!(matches!(
        cpu.step_boundary(&mut bus).unwrap(),
        TimedOutcome::Interrupt { level: 7, .. }
    ));
    assert!(matches!(
        cpu.step_boundary(&mut bus).unwrap(),
        TimedOutcome::Instruction(_)
    ));
    cpu.registers.set_status_register(0x2600);
    assert!(matches!(
        cpu.step_boundary(&mut bus).unwrap(),
        TimedOutcome::Interrupt { level: 7, .. }
    ));
    cpu.interrupt_input().set_level(6);
    cpu.interrupt_input().set_level(7);
    assert!(matches!(
        cpu.step_boundary(&mut bus).unwrap(),
        TimedOutcome::Interrupt { level: 7, .. }
    ));
    assert!(!cpu.interrupt_input().set_level(8));
    assert_eq!(cpu.interrupt_input().level(), 7);
}

#[test]
fn stop_uses_queued_status_and_accepted_interrupt_wakes_it() {
    let mut cpu = cpu(0x2700);
    cpu.pipeline.as_mut().unwrap().words = [0x4e72, 0x2300];
    let mut bus = Bus::default();
    bus.handler(28);
    assert_eq!(
        cpu.step_boundary(&mut bus).unwrap(),
        TimedOutcome::Instruction(TimedStep {
            opcode: 0x4e72,
            clocks: 4,
            exception: None
        })
    );
    assert_eq!(bus.events, [Event::Idle(0, 4)]);
    assert_eq!(cpu.registers.pc, 0x1234_0104);
    assert!(cpu.stopped());
    cpu.interrupt_input().set_level(3);
    assert_eq!(cpu.step_boundary(&mut bus).unwrap(), TimedOutcome::Stopped);
    assert_eq!(cpu.step(&mut bus), Err(TimingFault::Stopped));
    assert_eq!(bus.events.len(), 1);
    cpu.interrupt_input().set_level(4);
    assert!(matches!(
        cpu.step_boundary(&mut bus).unwrap(),
        TimedOutcome::Interrupt {
            level: 4,
            clocks: 44,
            ..
        }
    ));
    assert!(!cpu.stopped());
    assert_eq!(bus.words[&0xffe], 0x0104);
    assert_eq!(bus.words[&0xffa], 0x2300);
}

#[test]
fn every_interrupt_transfer_failure_accounts_waits_and_freezes_without_replay() {
    for index in 0..8 {
        let mut cpu = cpu(0x2000);
        cpu.interrupt_input().set_level(4);
        let original = cpu.registers;
        let mut bus = Bus {
            wait: 1,
            fail: Some(index),
            ..Bus::default()
        };
        bus.handler(28);
        assert!(
            matches!(cpu.step_boundary(&mut bus), Err(TimingFault::Bus { source, .. }) if source == index)
        );
        assert_eq!(cpu.registers, original);
        assert_eq!(cpu.clock().0, bus.clock);
        let events = bus.events.clone();
        assert_eq!(cpu.step_boundary(&mut bus), Err(TimingFault::Frozen));
        assert_eq!(bus.events, events);
        assert_eq!(bus.transfers, index + 1);
        if index > 0 {
            assert_eq!(bus.words[&0xffe], 0x0100);
        }
    }
}

#[test]
fn unsupported_ack_preserves_completed_pc_write_without_fallback() {
    let mut cpu = cpu(0x2000);
    cpu.interrupt_input().set_level(4);
    let mut bus = Bus {
        response: None,
        ..Bus::default()
    };
    assert_eq!(
        cpu.step_boundary(&mut bus),
        Err(TimingFault::InterruptAcknowledgeUnsupported)
    );
    assert_eq!(cpu.clock(), CpuClocks(10));
    assert_eq!(bus.words[&0xffe], 0x0100);
    assert_eq!(bus.transfers, 1);
    assert_eq!(cpu.step_boundary(&mut bus), Err(TimingFault::Frozen));
}
