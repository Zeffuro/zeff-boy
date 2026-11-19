use crate::bus::{AccessKind, Direction, FunctionCode};
use crate::data_tests::{execute, program};
use crate::multiply::{multiply_cycles, signed_divide_cycles, unsigned_divide_cycles};
use crate::{CARRY, EXTEND, Fault, NEGATIVE, OVERFLOW, SUPERVISOR, StepOutcome, ZERO};

#[test]
fn all_multiplier_words_match_products_flags_and_independent_event_counts() {
    for signed in [false, true] {
        let (mut cpu, mut bus) = program(&[if signed { 0xc1c1 } else { 0xc0c1 }]);
        for source in 0..=u16::MAX {
            let mut previous = 0;
            let mut events = 0;
            for bit in 0..16 {
                let current = (source >> bit) & 1;
                events += u32::from(if signed {
                    current != previous
                } else {
                    current != 0
                });
                previous = current;
            }
            for destination in [0x1234_0000, 0xabcd_8000, 0x5555_ffff] {
                cpu.registers.pc = 0x1000;
                cpu.registers.d[0] = destination;
                cpu.registers.d[1] = u32::from(source);
                cpu.registers.set_status_register(0x251f);
                bus.transfers.clear();
                let product = if signed {
                    (i64::from(destination as i16) * i64::from(source as i16)) as u32
                } else {
                    u32::from(destination as u16) * u32::from(source)
                };
                assert_eq!(execute(&mut cpu, &mut bus), 38 + 2 * events);
                assert_eq!(cpu.registers.d[0], product);
                assert_eq!(cpu.registers.d[1], u32::from(source));
                let expected = if product == 0 { ZERO } else { 0 }
                    | if product >> 31 != 0 { NEGATIVE } else { 0 };
                assert_eq!(cpu.registers.status_register(), 0x2500 | EXTEND | expected);
                assert_eq!(bus.transfers.len(), 1);
            }
        }
    }
}

#[test]
fn multiply_same_register_reads_both_original_words() {
    for signed in [false, true] {
        let (mut cpu, mut bus) = program(&[if signed { 0xc1c0 } else { 0xc0c0 }]);
        for source in 0..=u16::MAX {
            cpu.registers.pc = 0x1000;
            cpu.registers.d[0] = 0x1234_0000 | u32::from(source);
            bus.transfers.clear();
            execute(&mut cpu, &mut bus);
            let value = if signed {
                i64::from(source as i16)
            } else {
                i64::from(source)
            };
            assert_eq!(cpu.registers.d[0], (value * value) as u32);
        }
    }
}

#[test]
fn multiply_clears_carry_overflow_and_replaces_nz_for_every_initial_ccr() {
    for (opcode, destination, source, result, flags) in [
        (0xc0c1, 0xffff, 0xffff, 0xfffe_0001, NEGATIVE),
        (0xc1c1, 0x8000, 0x8000, 0x4000_0000, 0),
        (0xc1c1, 0xffff, 1, u32::MAX, NEGATIVE),
        (0xc0c1, 0xffff, 0, 0, ZERO),
    ] {
        for ccr in 0..32 {
            let (mut cpu, mut bus) = program(&[opcode]);
            cpu.registers.d[0] = destination;
            cpu.registers.d[1] = source;
            cpu.registers.set_status_register(0x2500 | ccr);
            execute(&mut cpu, &mut bus);
            assert_eq!(cpu.registers.d[0], result);
            assert_eq!(
                cpu.registers.status_register(),
                0x2500 | (ccr & EXTEND) | flags
            );
        }
    }
}

#[test]
fn division_signs_remainders_quotient_boundaries_and_all_initial_flags() {
    for (signed, dividend, divisor, result, flags) in [
        (false, 0, 3, 0, ZERO),
        (false, 7, 3, 0x0001_0002, 0),
        (false, 0xffff, 1, 0x0000_ffff, NEGATIVE),
        (false, 0x1_0000, 1, 0x1_0000, NEGATIVE | OVERFLOW),
        (false, u32::MAX, 0xffff, u32::MAX, NEGATIVE | OVERFLOW),
        (true, 7, 3, 0x0001_0002, 0),
        (true, (-7i32) as u32, 3, 0xffff_fffe, NEGATIVE),
        (true, 7, (-3i16) as u16, 0x0001_fffe, NEGATIVE),
        (true, (-7i32) as u32, (-3i16) as u16, 0xffff_0002, 0),
        (true, (-1i32) as u32, 2, 0xffff_0000, ZERO),
        (true, 32767, 1, 32767, 0),
        (true, 32768, 1, 32768, NEGATIVE | OVERFLOW),
        (true, (-32768i32) as u32, 1, 0x8000, NEGATIVE),
        (
            true,
            (-32769i32) as u32,
            1,
            (-32769i32) as u32,
            NEGATIVE | OVERFLOW,
        ),
        (true, 32768, 0xffff, 0x8000, NEGATIVE),
        (
            true,
            i32::MIN as u32,
            0xffff,
            i32::MIN as u32,
            NEGATIVE | OVERFLOW,
        ),
        (
            true,
            i32::MIN as u32,
            0x8000,
            i32::MIN as u32,
            NEGATIVE | OVERFLOW,
        ),
    ] {
        for ccr in 0..32 {
            let (mut cpu, mut bus) = program(&[if signed { 0x81c1 } else { 0x80c1 }]);
            cpu.registers.d[0] = dividend;
            cpu.registers.d[1] = u32::from(divisor);
            cpu.registers.set_status_register(0x2500 | ccr);
            execute(&mut cpu, &mut bus);
            assert_eq!(cpu.registers.d[0], result);
            assert_eq!(cpu.registers.d[1], u32::from(divisor));
            assert_eq!(
                cpu.registers.status_register(),
                0x2500 | (ccr & EXTEND) | flags
            );
        }
    }
}

#[test]
fn variable_cycle_witnesses_include_early_and_late_overflow() {
    for (source, unsigned, signed) in [
        (0, 38, 38),
        (1, 40, 42),
        (0xffff, 70, 40),
        (0x5555, 54, 70),
        (0x8000, 40, 40),
    ] {
        assert_eq!(multiply_cycles(source, false), unsigned);
        assert_eq!(multiply_cycles(source, true), signed);
    }
    for (dividend, divisor, cycles) in [
        (0, 1, 136),
        (0xffff, 1, 106),
        (0xfffe_ffff, 0xffff, 76),
        (0xffff_ffff, 0xffff, 10),
        (0x1_0000, 1, 10),
    ] {
        assert_eq!(unsigned_divide_cycles(dividend, divisor), cycles);
    }
    for (dividend, divisor, cycles) in [
        (0, 1, 150),
        (65535, 1, 120),
        (65535, -1, 122),
        (-65535, 1, 126),
        (-65535, -1, 124),
        (65536, 1, 16),
        (-65536, 1, 18),
        (i32::MIN, -1, 18),
    ] {
        assert_eq!(signed_divide_cycles(dividend, divisor), cycles);
    }
}

#[test]
fn all_data_source_modes_apply_word_ea_cycles_and_side_effects_once() {
    for (mode, reg, extension, address, ea_cycles, final_a0) in [
        (0, 1, vec![], None, 0, 0x2000),
        (2, 0, vec![], Some(0x2000), 4, 0x2000),
        (3, 0, vec![], Some(0x2000), 4, 0x2002),
        (4, 0, vec![], Some(0x1ffe), 6, 0x1ffe),
        (5, 0, vec![4], Some(0x2004), 8, 0x2000),
        (6, 0, vec![0x2004], Some(0x2004), 10, 0x2000),
        (7, 0, vec![0x2000], Some(0x2000), 8, 0x2000),
        (7, 1, vec![0, 0x2000], Some(0x2000), 12, 0x2000),
        (7, 2, vec![4], Some(0x1006), 8, 0x2000),
        (7, 3, vec![0x2004], Some(0x1006), 10, 0x2000),
        (7, 4, vec![2], None, 4, 0x2000),
    ] {
        for family in [0xc0c0, 0xc1c0, 0x80c0, 0x81c0, 0x4180] {
            let opcode = family | (mode << 3) | reg;
            let mut words = vec![opcode];
            words.extend(&extension);
            let (mut cpu, mut bus) = program(&words);
            cpu.registers.a[0] = 0x2000;
            cpu.registers.d[0] = if family == 0x4180 { 1 } else { 7 };
            cpu.registers.d[1] = 2;
            cpu.registers.d[2] = 0;
            if let Some(address) = address {
                bus.word(address, 2);
            }
            let memory = bus.memory.clone();
            let base = match family {
                0xc0c0 => 40,
                0xc1c0 => 42,
                0x80c0 => unsigned_divide_cycles(7, 2),
                0x81c0 => signed_divide_cycles(7, 2),
                0x4180 => 10,
                _ => unreachable!(),
            };
            assert_eq!(execute(&mut cpu, &mut bus), base + ea_cycles);
            assert_eq!(cpu.registers.a[0], final_a0);
            assert_eq!(cpu.registers.pc, 0x1002 + extension.len() as u32 * 2);
            assert_eq!(bus.memory, memory);
            assert!(bus.transfers.iter().all(|t| t.direction == Direction::Read));
            assert_eq!(
                cpu.registers.d[0],
                match family {
                    0xc0c0 | 0xc1c0 => 14,
                    0x80c0 | 0x81c0 => 0x0001_0003,
                    _ => 1,
                }
            );
        }
    }
}

#[test]
fn division_destination_alias_and_stack_pointer_postincrement() {
    let (mut cpu, mut bus) = program(&[0x80c0]);
    cpu.registers.d[0] = 0x0001_0002;
    execute(&mut cpu, &mut bus);
    assert_eq!(cpu.registers.d[0], 0x8001);
    let (mut cpu, mut bus) = program(&[0x81df]);
    cpu.registers.ssp = 0x2000;
    cpu.registers.d[0] = (-7i32) as u32;
    bus.word(0x2000, 3);
    execute(&mut cpu, &mut bus);
    assert_eq!(cpu.registers.ssp, 0x2002);
    assert_eq!(cpu.registers.d[0], 0xffff_fffe);
}

#[test]
fn divide_zero_and_chk_stack_following_pc_with_selected_original_flags() {
    for (opcode, source, dividend, vector, cycles, negative) in [
        (0x80fc, 0, 0x1234_5678, 5, 42, false),
        (0x81fc, 0, 0x1234_5678, 5, 42, false),
        (0x41bc, 7, 0xabcd_ffff, 6, 44, true),
        (0x41bc, 7, 0xabcd_0008, 6, 42, false),
        (0x41bc, 0xffff, 0, 6, 42, false),
        (0x41bc, 0xfffe, 0xffff, 6, 42, true),
        (0x41bc, 21984, (-15513_i16) as u16 as u32, 6, 42, true),
        (
            0x41bc,
            (-32233_i16) as u16,
            (-31280_i16) as u16 as u32,
            6,
            42,
            true,
        ),
    ] {
        for supervisor in [false, true] {
            let (mut cpu, mut bus) = program(&[opcode, source]);
            let sr = 0x051f | if supervisor { SUPERVISOR } else { 0 };
            cpu.registers.set_status_register(sr);
            cpu.registers.ssp = 0x2000;
            cpu.registers.usp = 0x3000;
            cpu.registers.d[0] = dividend;
            bus.word(u32::from(vector) * 4, 0);
            bus.word(u32::from(vector) * 4 + 2, 0x4000);
            assert_eq!(
                cpu.step(&mut bus),
                StepOutcome::Exception {
                    opcode,
                    vector,
                    nominal_cycles: cycles
                }
            );
            assert_eq!(cpu.registers.d[0], dividend);
            assert_eq!(cpu.registers.pc, 0x4000);
            assert_eq!(cpu.registers.ssp, 0x1ffa);
            assert_eq!(cpu.registers.usp, 0x3000);
            assert_eq!(bus.transfers[2].value, 0x1004);
            let saved_sr = bus.transfers[3].value;
            assert_eq!(saved_sr & EXTEND, EXTEND);
            assert_eq!(saved_sr & NEGATIVE != 0, negative);
            if vector == 6 {
                assert_eq!(
                    saved_sr & (ZERO | OVERFLOW | CARRY),
                    if dividend as u16 == 0 { ZERO } else { 0 }
                );
            } else {
                assert_eq!(
                    saved_sr & (ZERO | OVERFLOW | CARRY),
                    if opcode == 0x81fc { ZERO } else { 0 }
                );
            }
            assert!(
                bus.transfers[2..]
                    .iter()
                    .all(|t| t.access.function_code == FunctionCode::SupervisorData)
            );
            assert!(
                bus.transfers[2..5]
                    .iter()
                    .all(|t| t.access.kind == AccessKind::Stack)
            );
            assert!(
                bus.transfers[5..]
                    .iter()
                    .all(|t| t.access.kind == AccessKind::Vector)
            );
        }
    }
    for value in [0, 7] {
        let (mut cpu, mut bus) = program(&[0x41bc, 7]);
        cpu.registers.d[0] = 0xabcd_0000 | value;
        cpu.registers.set_status_register(0x251f);
        assert_eq!(execute(&mut cpu, &mut bus), 14);
        assert_eq!(
            cpu.registers.status_register(),
            0x2500 | EXTEND | if value == 0 { ZERO } else { 0 }
        );
    }
}

#[test]
fn invalid_sources_and_original_68000_chk_long_fault_before_extensions() {
    for family in [0xc0c0, 0xc1c0, 0x80c0, 0x81c0, 0x4180] {
        for ea in [0x08, 0x0f, 0x3d, 0x3e, 0x3f] {
            let opcode = family | ea;
            let (mut cpu, mut bus) = program(&[opcode]);
            let original = cpu.clone();
            assert_eq!(
                cpu.step(&mut bus),
                StepOutcome::Fault(Fault::UnsupportedInstruction { pc: 0x1000, opcode })
            );
            assert_eq!(cpu, original);
            assert_eq!(bus.transfers.len(), 1);
        }
    }
    let (mut cpu, mut bus) = program(&[0x413c, 7]);
    assert!(matches!(
        cpu.step(&mut bus),
        StepOutcome::Fault(Fault::UnsupportedInstruction { .. })
    ));
    assert_eq!(bus.transfers.len(), 1);
}

#[test]
fn operand_and_trap_faults_roll_back_cpu_and_keep_completed_writes() {
    for opcode in [0xc0d8, 0xc1d8, 0x80d8, 0x81d8, 0x4198] {
        let (mut cpu, mut bus) = program(&[opcode]);
        cpu.registers.a[0] = 0x2000;
        bus.word(0x2000, 1);
        let original = cpu.clone();
        bus.fail_at = Some(1);
        assert!(matches!(
            cpu.step(&mut bus),
            StepOutcome::Fault(Fault::Bus(crate::bus::BusFault::Transfer { source: 1, .. }))
        ));
        assert_eq!(cpu, original);
    }
    for opcode in [0x80fc, 0x81fc, 0x41bc] {
        for fail in 1..=6 {
            let (mut cpu, mut bus) = program(&[opcode, 0]);
            cpu.registers.d[0] = 1;
            cpu.registers.ssp = 0x2000;
            let original = cpu.clone();
            bus.fail_at = Some(fail);
            assert!(matches!(cpu.step(&mut bus), StepOutcome::Fault(Fault::Bus(
                crate::bus::BusFault::Transfer { source, .. }
            )) if source == fail));
            assert_eq!(cpu, original);
            assert_eq!(bus.transfers.len(), fail + 1);
            if fail > 2 {
                assert_eq!((bus.peek(0x1ffe), bus.peek(0x1fff)), (0x10, 4));
            }
        }
    }
}
