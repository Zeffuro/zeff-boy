use crate::Registers;
use crate::bus::{AccessKind, ByteLanes, Direction, FunctionCode};
use crate::ea::{self, EaError, Operand, Size};
use crate::test_bus::{TestBus, Transfer, access};

fn registers(supervisor: bool) -> Registers {
    let mut regs = Registers::new();
    regs.set_status_register(if supervisor { 0x2000 } else { 0 });
    for index in 0..8 {
        regs.d[index] = 0xabcd_1234 + index as u32;
        regs.set_address_register(index, 0xf100_2000 + index as u32 * 0x100);
    }
    regs
}

fn memory(address: u32, function_code: FunctionCode, predecrement: bool) -> Operand {
    Operand::Memory {
        address,
        function_code,
        predecrement,
    }
}

#[test]
fn all_register_modes_sizes_and_stack_banks_resolve_full_addresses() {
    for supervisor in [false, true] {
        let data = if supervisor {
            FunctionCode::SupervisorData
        } else {
            FunctionCode::UserData
        };
        let program = if supervisor {
            FunctionCode::SupervisorProgram
        } else {
            FunctionCode::UserProgram
        };
        for size in [Size::Byte, Size::Word, Size::Long] {
            for index in 0..8 {
                for mode in 0..7 {
                    let mut regs = registers(supervisor);
                    let before = regs;
                    let base = 0xf100_2000 + u32::from(index) * 0x100;
                    let mut bus = TestBus::default();
                    let mut cursor = 0xac00_1000;
                    bus.word(cursor, if mode == 6 { 0x0080 } else { 0xff80 });
                    let step = match size {
                        Size::Byte if index != 7 => 1,
                        Size::Long => 4,
                        _ => 2,
                    };
                    let expected = match mode {
                        0 => Operand::DataRegister(usize::from(index)),
                        1 => Operand::AddressRegister(usize::from(index)),
                        2 | 3 => memory(base, data, false),
                        4 => memory(base - step, data, true),
                        5 => memory(base - 128, data, false),
                        6 => memory(base + 0x1234 - 128, data, false),
                        _ => unreachable!(),
                    };
                    assert_eq!(
                        ea::resolve(&mut regs, &mut bus, &mut cursor, mode, index, size),
                        Ok(expected)
                    );
                    let expected_register = match mode {
                        3 => base + step,
                        4 => base - step,
                        _ => base,
                    };
                    assert_eq!(
                        regs.address_register(usize::from(index)),
                        Some(expected_register)
                    );
                    assert_eq!(regs.d, before.d);
                    assert_eq!(regs.status_register(), before.status_register());
                    if supervisor {
                        assert_eq!(regs.usp, before.usp);
                    } else {
                        assert_eq!(regs.ssp, before.ssp);
                    }
                    if mode >= 5 {
                        assert_eq!(cursor, 0xac00_1002);
                        assert_eq!(
                            bus.transfers,
                            [Transfer {
                                access: access(
                                    0xac00_1000,
                                    0x1000,
                                    program,
                                    AccessKind::Extension,
                                    ByteLanes::Both
                                ),
                                direction: Direction::Read,
                                value: if mode == 6 { 0x0080 } else { 0xff80 },
                            }]
                        );
                    } else {
                        assert_eq!(cursor, 0xac00_1000);
                        assert!(bus.transfers.is_empty());
                    }
                }
            }
        }
    }
}

#[test]
fn absolute_pc_and_immediate_modes_consume_only_the_required_extensions() {
    for supervisor in [false, true] {
        let data = if supervisor {
            FunctionCode::SupervisorData
        } else {
            FunctionCode::UserData
        };
        let program = if supervisor {
            FunctionCode::SupervisorProgram
        } else {
            FunctionCode::UserProgram
        };
        for size in [Size::Byte, Size::Word, Size::Long] {
            for (reg, words, expected) in [
                (0, [0x8002, 0xfeed], memory(0xffff_8002, data, false)),
                (1, [0xabcd, 0x8002], memory(0xabcd_8002, data, false)),
                (2, [0xfffc, 0xfeed], memory(0xffff_fffa, program, false)),
                (3, [0x0080, 0xfeed], memory(0x11b2, program, false)),
                (
                    4,
                    [0x89ab, 0xcdef],
                    Operand::Immediate(match size {
                        Size::Byte => 0xab,
                        Size::Word => 0x89ab,
                        Size::Long => 0x89ab_cdef,
                    }),
                ),
            ] {
                let mut regs = registers(supervisor);
                let mut bus = TestBus::default();
                let mut cursor = 0xffff_fffe;
                bus.word(cursor, words[0]);
                bus.word(0, words[1]);
                assert_eq!(
                    ea::resolve(&mut regs, &mut bus, &mut cursor, 7, reg, size),
                    Ok(expected)
                );
                let long = reg == 1 || reg == 4 && size == Size::Long;
                assert_eq!(cursor, if long { 2 } else { 0 });
                let mut transfers = vec![Transfer {
                    access: access(
                        0xffff_fffe,
                        0x00ff_fffe,
                        program,
                        AccessKind::Extension,
                        ByteLanes::Both,
                    ),
                    direction: Direction::Read,
                    value: words[0],
                }];
                if long {
                    transfers.push(Transfer {
                        access: access(0, 0, program, AccessKind::Extension, ByteLanes::Both),
                        direction: Direction::Read,
                        value: words[1],
                    });
                }
                assert_eq!(bus.transfers, transfers);
            }
        }
    }
}

#[test]
fn brief_index_covers_every_register_size_displacement_and_pc_base() {
    for supervisor in [false, true] {
        let mut regs = registers(supervisor);
        for index in 0..8 {
            regs.d[index] = 0x7fff_8000 + index as u32;
            regs.set_address_register(index, 0x8000_7ff8 + index as u32);
        }
        regs.set_address_register(0, 0xffff_fff0);
        for address_index in [false, true] {
            for index in 0..8_u16 {
                for long in [false, true] {
                    for displacement in 0..=255_u16 {
                        for pc_relative in [false, true] {
                            let extension = (u16::from(address_index) << 15)
                                | index << 12
                                | (u16::from(long) << 11)
                                | displacement;
                            let mut bus = TestBus::default();
                            let mut cursor = 0x1234_1000;
                            bus.word(cursor, extension);
                            let raw = if address_index {
                                regs.address_register(usize::from(index)).unwrap()
                            } else {
                                regs.d[usize::from(index)]
                            };
                            let index_value = if long {
                                i64::from(raw)
                            } else {
                                let word = i64::from(raw & 0xffff);
                                if word >= 32768 { word - 65536 } else { word }
                            };
                            let displacement_value = if displacement >= 128 {
                                i64::from(displacement) - 256
                            } else {
                                i64::from(displacement)
                            };
                            let base: u32 = if pc_relative {
                                0x1234_1000
                            } else {
                                0xffff_fff0
                            };
                            let expected =
                                (i64::from(base) + index_value + displacement_value) as u32;
                            let function_code = match (supervisor, pc_relative) {
                                (false, false) => FunctionCode::UserData,
                                (false, true) => FunctionCode::UserProgram,
                                (true, false) => FunctionCode::SupervisorData,
                                (true, true) => FunctionCode::SupervisorProgram,
                            };
                            assert_eq!(
                                ea::resolve(
                                    &mut regs,
                                    &mut bus,
                                    &mut cursor,
                                    if pc_relative { 7 } else { 6 },
                                    if pc_relative { 3 } else { 0 },
                                    Size::Long
                                ),
                                Ok(memory(expected, function_code, false))
                            );
                            assert_eq!(cursor, 0x1234_1002);
                            assert_eq!(bus.transfers.len(), 1);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn original_68000_ignores_reserved_index_bits_and_rejects_invalid_modes() {
    let mut regs = Registers::new();
    regs.d[0] = 0xffff_fff8;
    regs.a[0] = 0x2000;
    for ignored in 0..8_u16 {
        for (mode, reg) in [(6, 0), (7, 3)] {
            let mut bus = TestBus::default();
            let mut cursor = 0x1000;
            let extension = ignored << 8;
            bus.word(cursor, extension);
            assert_eq!(
                ea::resolve(&mut regs, &mut bus, &mut cursor, mode, reg, Size::Word),
                Ok(memory(
                    if mode == 6 { 0x1ff8 } else { 0x0ff8 },
                    if mode == 6 {
                        FunctionCode::SupervisorData
                    } else {
                        FunctionCode::SupervisorProgram
                    },
                    false
                ))
            );
            assert_eq!(cursor, 0x1002);
            assert_eq!(bus.transfers.len(), 1);
        }
    }
    for (mode, reg) in [(7, 5), (7, 6), (7, 7), (8, 0), (0, 8), (u16::MAX, u16::MAX)] {
        let mut bus = TestBus::default();
        let mut cursor = 0x1000;
        assert_eq!(
            ea::resolve(&mut regs, &mut bus, &mut cursor, mode, reg, Size::Long),
            Err(EaError::InvalidEncoding)
        );
        assert_eq!(cursor, 0x1000);
        assert!(bus.transfers.is_empty());
    }
}

#[test]
fn register_and_immediate_reads_and_writes_apply_size_and_sign_rules() {
    for supervisor in [false, true] {
        for index in 0..8 {
            for (size, expected_data, expected_address) in [
                (Size::Byte, 0xabcd_12ef, None),
                (Size::Word, 0xabcd_cdef, Some(0xffff_cdef)),
                (Size::Long, 0x89ab_cdef, Some(0x89ab_cdef)),
            ] {
                let mut regs = registers(supervisor);
                regs.d[index] = 0xabcd_1234;
                regs.set_address_register(index, 0xabcd_1234);
                let mut bus = TestBus::default();
                let expected_read = match size {
                    Size::Byte => 0x34,
                    Size::Word => 0x1234,
                    Size::Long => 0xabcd_1234,
                };
                for operand in [
                    Operand::DataRegister(index),
                    Operand::AddressRegister(index),
                    Operand::Immediate(0xabcd_1234),
                ] {
                    assert_eq!(ea::read(&regs, &mut bus, operand, size), Ok(expected_read));
                }
                ea::write(
                    &mut regs,
                    &mut bus,
                    Operand::DataRegister(index),
                    size,
                    0x89ab_cdef,
                )
                .unwrap();
                assert_eq!(regs.d[index], expected_data);
                let result = ea::write(
                    &mut regs,
                    &mut bus,
                    Operand::AddressRegister(index),
                    size,
                    0x89ab_cdef,
                );
                if let Some(value) = expected_address {
                    assert_eq!(result, Ok(()));
                    assert_eq!(regs.address_register(index), Some(value));
                } else {
                    assert_eq!(result, Err(EaError::InvalidEncoding));
                }
                assert_eq!(
                    ea::write(&mut regs, &mut bus, Operand::Immediate(0), size, 1),
                    Err(EaError::InvalidEncoding)
                );
                assert!(bus.transfers.is_empty());
            }
        }
    }
    let mut regs = Registers::new();
    let mut bus = TestBus::default();
    for operand in [Operand::DataRegister(8), Operand::AddressRegister(8)] {
        assert_eq!(
            ea::read(&regs, &mut bus, operand, Size::Long),
            Err(EaError::InvalidEncoding)
        );
        assert_eq!(
            ea::write(&mut regs, &mut bus, operand, Size::Long, 0),
            Err(EaError::InvalidEncoding)
        );
    }
}

#[path = "ea_bus_tests.rs"]
mod bus_tests;
