use crate::data_tests::{execute, program};
use crate::{CARRY, EXTEND, Fault, NEGATIVE, OVERFLOW, StepOutcome, ZERO};

#[test]
fn byte_extend_arithmetic_exhaustively_matches_signed_and_unsigned_integers() {
    for subtract in [false, true] {
        let (mut cpu, mut bus) = program(&[if subtract { 0x9300 } else { 0xd300 }]);
        for left in 0..=255_u32 {
            for right in 0..=255_u32 {
                for x in [0, 1] {
                    for z in [false, true] {
                        cpu.registers.pc = 0x1000;
                        cpu.registers.d[0] = 0x1234_0000 | right;
                        cpu.registers.d[1] = 0xabcd_0000 | left;
                        cpu.registers.set_status_register(
                            0x2500 | if x != 0 { EXTEND } else { 0 } | if z { ZERO } else { 0 },
                        );
                        bus.transfers.clear();
                        assert_eq!(execute(&mut cpu, &mut bus), 4);
                        let wide = if subtract {
                            left as i32 - right as i32 - x
                        } else {
                            left as i32 + right as i32 + x
                        };
                        let signed = if subtract {
                            left as u8 as i8 as i32 - right as u8 as i8 as i32 - x
                        } else {
                            left as u8 as i8 as i32 + right as u8 as i8 as i32 + x
                        };
                        let result = wide as u8;
                        let flags = if !(0..=255).contains(&wide) {
                            CARRY | EXTEND
                        } else {
                            0
                        } | if !(-128..=127).contains(&signed) {
                            OVERFLOW
                        } else {
                            0
                        } | if result & 128 != 0 { NEGATIVE } else { 0 }
                            | if z && result == 0 { ZERO } else { 0 };
                        assert_eq!(cpu.registers.d[1], 0xabcd_0000 | u32::from(result));
                        assert_eq!(cpu.registers.status_register(), 0x2500 | flags);
                        assert_eq!(cpu.registers.d[0], 0x1234_0000 | right);
                    }
                }
            }
        }
    }
}

#[test]
fn wide_carry_through_all_ones_and_sticky_zero_are_not_lost() {
    for (opcode, left, right, result, flags) in [
        (0xd340, 0, 0xffff, 0, CARRY | EXTEND | ZERO),
        (0xd380, 0, u32::MAX, 0, CARRY | EXTEND | ZERO),
        (0x9340, 0, 0xffff, 0, CARRY | EXTEND | ZERO),
        (0x9380, 0, u32::MAX, 0, CARRY | EXTEND | ZERO),
        (0xd380, 0x7fff_ffff, 0, 0x8000_0000, NEGATIVE | OVERFLOW),
        (0x9380, 0x8000_0000, 0, 0x7fff_ffff, OVERFLOW),
    ] {
        let (mut cpu, mut bus) = program(&[opcode]);
        cpu.registers.d[0] = right;
        cpu.registers.d[1] = left;
        cpu.registers.set_status_register(0x2500 | EXTEND | ZERO);
        execute(&mut cpu, &mut bus);
        assert_eq!(cpu.registers.d[1], result);
        assert_eq!(cpu.registers.status_register(), 0x2500 | flags);
    }
}

#[test]
fn extend_predecrement_a7_alias_and_negx_update_each_operand_once() {
    let (mut cpu, mut bus) = program(&[0xdf0f]);
    cpu.registers.ssp = 0x2004;
    bus.seed(0x2002, &[0xff]);
    bus.seed(0x2000, &[0]);
    cpu.registers.set_status_register(0x2500 | EXTEND | ZERO);
    assert_eq!(execute(&mut cpu, &mut bus), 18);
    assert_eq!(cpu.registers.ssp, 0x2000);
    assert_eq!(bus.peek(0x2000), 0);
    assert_eq!(cpu.registers.status_register(), 0x2515);
    for (opcode, value, expected, cycles) in [
        (0x4000, 0, 0xff, 4),
        (0x4040, 0, 0xffff, 4),
        (0x4080, 0, u32::MAX, 6),
    ] {
        let (mut cpu, mut bus) = program(&[opcode]);
        cpu.registers.d[0] = value;
        cpu.registers.set_status_register(0x2514);
        assert_eq!(execute(&mut cpu, &mut bus), cycles);
        assert_eq!(cpu.registers.d[0], expected);
        assert_eq!(cpu.registers.status_register(), 0x2519);
    }
}

#[test]
fn extend_long_memory_uses_source_then_destination_and_low_first_writes() {
    for (opcode, source, result, sr) in [
        (0xdf8f, 0xffff_ffff_u32, 0, 0x2515),
        (0x9f8f, 0xffff_ffff_u32, 0, 0x2515),
    ] {
        let (mut cpu, mut bus) = program(&[opcode]);
        cpu.registers.ssp = 0x2008;
        bus.seed(0x2004, &source.to_be_bytes());
        bus.seed(0x2000, &0_u32.to_be_bytes());
        cpu.registers.set_status_register(0x2514);
        assert_eq!(execute(&mut cpu, &mut bus), 30);
        assert_eq!(cpu.registers.ssp, 0x2000);
        assert_eq!(cpu.registers.status_register(), sr);
        assert_eq!(
            (bus.peek(0x2000), bus.peek(0x2003)),
            (result as u8, result as u8)
        );
        let addresses: Vec<_> = bus.transfers[1..]
            .iter()
            .map(|t| t.access.logical_address)
            .collect();
        assert_eq!(addresses, [0x2004, 0x2006, 0x2000, 0x2002, 0x2002, 0x2000]);
    }
}

#[test]
fn extend_and_negx_faults_restore_registers_but_keep_completed_low_write() {
    for (opcode, maximum) in [(0xdf8f, 6), (0x9f8f, 6), (0x40a7, 4)] {
        for fail in 1..=maximum {
            let (mut cpu, mut bus) = program(&[opcode]);
            cpu.registers.ssp = if opcode == 0x40a7 { 0x2004 } else { 0x2008 };
            cpu.registers.set_status_register(0x2500);
            bus.seed(0x2000, &0x1234_5678_u32.to_be_bytes());
            bus.seed(0x2004, &0x0000_0001_u32.to_be_bytes());
            let original = cpu.clone();
            bus.fail_at = Some(fail);
            assert!(matches!(
                cpu.step(&mut bus),
                StepOutcome::Fault(Fault::Bus(_))
            ));
            assert_eq!(cpu, original);
            assert_eq!(bus.transfers.len(), fail + 1);
            let first_write = maximum - 1;
            if fail > first_write {
                let expected = if opcode == 0xdf8f {
                    0x5679_u16
                } else if opcode == 0x9f8f {
                    0x5677
                } else {
                    0xa988
                };
                assert_eq!(
                    (bus.peek(0x2002), bus.peek(0x2003)),
                    ((expected >> 8) as u8, expected as u8)
                );
                assert_eq!((bus.peek(0x2000), bus.peek(0x2001)), (0x12, 0x34));
            }
        }
    }
}
