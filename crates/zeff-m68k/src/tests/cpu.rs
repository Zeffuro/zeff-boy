use zeff_emu_common::cpu::{CpuCore, CpuStep};

use crate::bus::{AccessKind, BusFault, ByteLanes, Direction, FunctionCode};
use crate::conformance_tests::populated_registers;
use crate::test_bus::{TestBus, access};
use crate::{Cpu, Fault, Registers, STATUS_MASK, SUPERVISOR, StepOutcome, TRACE};

#[test]
fn reset_reads_all_vectors_in_supervisor_program_space_and_applies_zero_policy() {
    let mut cpu = Cpu {
        registers: populated_registers(0xdead_beef),
        ..Cpu::new()
    };
    let mut bus = TestBus::default();
    bus.seed(0, &[0xab, 0xcd, 0x01, 0x23, 0xfe, 0xdc, 0xba, 0x99]);
    cpu.reset(&mut bus).unwrap();
    let mut expected = Registers::new();
    expected.ssp = 0xabcd_0123;
    expected.pc = 0xfedc_ba99;
    assert_eq!(cpu.registers, expected);
    assert_eq!(cpu.registers.status_register(), 0x2700);
    let expected: Vec<_> = [0, 2, 4, 6]
        .map(|address| {
            access(
                address,
                address,
                FunctionCode::SupervisorProgram,
                AccessKind::ResetVector,
                ByteLanes::Both,
            )
        })
        .into();
    assert_eq!(
        bus.transfers
            .iter()
            .map(|transfer| transfer.access)
            .collect::<Vec<_>>(),
        expected
    );
    assert!(
        bus.transfers
            .iter()
            .all(|transfer| transfer.direction == Direction::Read)
    );
}

#[test]
fn every_reset_read_failure_preserves_entire_cpu_and_completed_bus_effects() {
    for failed in 0..4 {
        let mut cpu = Cpu {
            registers: populated_registers(0x1234_5678),
            ..Cpu::new()
        };
        let original = cpu.clone();
        let mut bus = TestBus {
            fail_at: Some(failed),
            ..TestBus::default()
        };
        bus.seed(0, &[0x12, 0x34, 0x56, 0x78, 0xab, 0xcd, 0xef, 0x01]);
        let address = u32::try_from(failed).unwrap() * 2;
        assert_eq!(
            cpu.reset(&mut bus),
            Err(Fault::Bus(BusFault::Transfer {
                access: access(
                    address,
                    address,
                    FunctionCode::SupervisorProgram,
                    AccessKind::ResetVector,
                    ByteLanes::Both
                ),
                direction: Direction::Read,
                source: failed,
            }))
        );
        assert_eq!(cpu, original);
        assert_eq!(bus.transfers.len(), failed + 1);
    }
}

#[test]
fn opcode_and_extension_metadata_follow_current_privilege() {
    for (sr, function_code) in [
        (0x0500, FunctionCode::UserProgram),
        (0x2500, FunctionCode::SupervisorProgram),
    ] {
        let mut cpu = Cpu {
            registers: populated_registers(0xab00_1000),
            ..Cpu::new()
        };
        cpu.registers.set_status_register(sr);
        let mut bus = TestBus::default();
        bus.word(0x1000, 0x6000);
        bus.word(0x1002, 0xfffe);
        assert_eq!(
            cpu.step_cpu(&mut bus),
            StepOutcome::Executed {
                opcode: 0x6000,
                nominal_cycles: 10
            }
        );
        assert_eq!(cpu.registers.pc, 0xab00_1000);
        assert_eq!(
            bus.transfers
                .iter()
                .map(|transfer| transfer.access)
                .collect::<Vec<_>>(),
            [
                access(
                    0xab00_1000,
                    0x1000,
                    function_code,
                    AccessKind::Opcode,
                    ByteLanes::Both
                ),
                access(
                    0xab00_1002,
                    0x1002,
                    function_code,
                    AccessKind::Extension,
                    ByteLanes::Both
                ),
            ]
        );
    }
}

#[test]
fn failed_opcode_or_extension_reports_no_progress_and_preserves_cpu() {
    for failed in [0, 1] {
        let mut cpu = Cpu {
            registers: populated_registers(0x1000),
            ..Cpu::new()
        };
        let original = cpu.clone();
        let mut bus = TestBus {
            fail_at: Some(failed),
            ..TestBus::default()
        };
        bus.word(0x1000, 0x6000);
        bus.word(0x1002, 0x1234);
        let outcome = cpu.step(&mut bus);
        assert_eq!(outcome.cpu_cycles(), None);
        assert_eq!(
            outcome,
            StepOutcome::Fault(Fault::Bus(BusFault::Transfer {
                access: access(
                    0x1000 + u32::try_from(failed).unwrap() * 2,
                    0x1000 + u32::try_from(failed).unwrap() * 2,
                    FunctionCode::SupervisorProgram,
                    if failed == 0 {
                        AccessKind::Opcode
                    } else {
                        AccessKind::Extension
                    },
                    ByteLanes::Both,
                ),
                direction: Direction::Read,
                source: failed,
            }))
        );
        assert_eq!(cpu, original);
        assert_eq!(bus.transfers.len(), failed + 1);
    }
}

#[test]
fn unsupported_encodings_and_later_instructions_are_repeatable_faults() {
    for opcode in [0x7100, 0x7fff, 0x4e74] {
        let mut cpu = Cpu {
            registers: populated_registers(0x1000),
            ..Cpu::new()
        };
        let original = cpu.clone();
        let mut bus = TestBus::default();
        bus.word(0x1000, opcode);
        for attempt in 1..=3 {
            let outcome = cpu.step(&mut bus);
            assert_eq!(
                outcome,
                StepOutcome::Fault(Fault::UnsupportedInstruction { pc: 0x1000, opcode })
            );
            assert_eq!(outcome.cpu_cycles(), None);
            assert_eq!(cpu, original);
            assert_eq!(bus.transfers.len(), attempt);
        }
    }
}

#[test]
fn trace_is_rejected_before_fetch_and_repeated_faults_remain_inert() {
    let mut cpu = Cpu {
        registers: populated_registers(0x1000),
        ..Cpu::new()
    };
    cpu.registers
        .set_status_register(cpu.registers.status_register() | TRACE);
    let original = cpu.clone();
    let mut bus = TestBus::default();
    bus.word(0x1000, 0x4e71);
    for _ in 0..3 {
        let outcome = cpu.step(&mut bus);
        assert_eq!(outcome, StepOutcome::Fault(Fault::TraceUnsupported));
        assert_eq!(outcome.cpu_cycles(), None);
        assert_eq!(cpu, original);
        assert!(bus.transfers.is_empty());
    }
}

#[test]
fn odd_branch_targets_commit_then_fault_on_next_demand_fetch() {
    for opcode in (1..=255_u16)
        .filter(|value| value & 1 != 0)
        .map(|value| 0x6000 | value)
    {
        let mut cpu = Cpu {
            registers: populated_registers(0x2000),
            ..Cpu::new()
        };
        let mut bus = TestBus::default();
        bus.word(0x2000, opcode);
        assert_eq!(
            cpu.step(&mut bus),
            StepOutcome::Executed {
                opcode,
                nominal_cycles: 10
            }
        );
        let original = cpu.clone();
        let target = cpu.registers.pc;
        for _ in 0..2 {
            let outcome = cpu.step(&mut bus);
            assert_eq!(
                outcome,
                StepOutcome::Fault(Fault::Bus(BusFault::AddressError {
                    access: access(
                        target,
                        target & 0x00ff_fffe,
                        FunctionCode::SupervisorProgram,
                        AccessKind::Opcode,
                        ByteLanes::Both
                    ),
                    direction: Direction::Read,
                }))
            );
            assert_eq!(outcome.cpu_cycles(), None);
            assert_eq!(cpu, original);
            assert_eq!(bus.transfers.len(), 1);
        }
    }
}

#[test]
fn status_mask_discards_reserved_fields_and_a7_has_one_authoritative_bank() {
    let mut registers = populated_registers(0);
    registers.set_status_register(0xffff);
    assert_eq!(registers.status_register(), STATUS_MASK);
    assert_eq!(registers.address_register(7), Some(registers.ssp));
    assert!(registers.set_address_register(7, 0x1234_5678));
    assert_eq!(registers.ssp, 0x1234_5678);
    assert_eq!(registers.usp, 0xabcd_0123);
    registers.set_status_register(0x4000 | 0x1000);
    assert_eq!(registers.status_register(), 0);
    assert_eq!(registers.address_register(7), Some(0xabcd_0123));
    assert!(registers.set_address_register(7, 0x8765_4321));
    assert_eq!(registers.usp, 0x8765_4321);
    registers.set_status_register(SUPERVISOR);
    assert_eq!(registers.stack_pointer(), 0x1234_5678);
    for index in 0..7 {
        let value = 0xf000_0000 + u32::try_from(index).unwrap();
        assert!(registers.set_address_register(index, value));
        assert_eq!(registers.address_register(index), Some(value));
    }
    let original = registers;
    for index in [8, usize::MAX] {
        assert_eq!(registers.address_register(index), None);
        assert!(!registers.set_address_register(index, 1));
        assert_eq!(registers, original);
    }
}

#[test]
fn successful_steps_report_nominal_cpu_progress() {
    for (opcode, cycles) in [(0x4e71, 4), (0x70ff, 4), (0x60fe, 10)] {
        let mut bus = TestBus::default();
        bus.word(0, opcode);
        assert_eq!(
            Cpu::new()
                .step(&mut bus)
                .cpu_cycles()
                .map(std::num::NonZeroU64::get),
            Some(cycles)
        );
    }
}

#[test]
fn reset_to_program_executes_full_subset_then_repeats_a_bounded_loop() {
    let mut bus = TestBus::default();
    bus.seed(0, &[0x12, 0x34, 0x56, 0x78, 0, 0, 0x10, 0]);
    bus.seed(0x1000, &[0x4e, 0x71, 0x7e, 0x80, 0x60, 0xfc]);
    let mut cpu = Cpu::new();
    cpu.reset(&mut bus).unwrap();
    let mut cycles = 0;
    for opcode in [0x4e71, 0x7e80, 0x60fc, 0x7e80, 0x60fc] {
        let outcome = cpu.step_cpu(&mut bus);
        let nominal_cycles = if opcode == 0x60fc { 10 } else { 4 };
        assert_eq!(
            outcome,
            StepOutcome::Executed {
                opcode,
                nominal_cycles
            }
        );
        cycles += outcome.cpu_cycles().unwrap().get();
    }
    let mut expected = Registers::new();
    expected.ssp = 0x1234_5678;
    expected.d[7] = 0xffff_ff80;
    expected.pc = 0x1002;
    expected.set_status_register(0x2708);
    assert_eq!(cpu.registers, expected);
    assert_eq!(cycles, 32);
}

#[test]
fn repeated_transfer_faults_do_not_report_forward_progress() {
    let mut cpu = Cpu {
        registers: populated_registers(0x1000),
        ..Cpu::new()
    };
    let original = cpu.clone();
    let mut bus = TestBus::default();
    bus.word(0x1000, 0x4e71);
    for attempt in 0..3 {
        bus.fail_at = Some(attempt);
        let outcome = cpu.step(&mut bus);
        assert!(
            matches!(outcome, StepOutcome::Fault(Fault::Bus(BusFault::Transfer { source, .. })) if source == attempt)
        );
        assert_eq!(outcome.cpu_cycles(), None);
        assert_eq!(cpu, original);
        assert_eq!(bus.transfers.len(), attempt + 1);
    }
}
