use crate::Registers;
use crate::bus::{AccessKind, BusFault, ByteLanes, Direction, FunctionCode};
use crate::ea::{self, EaError, Operand, Size};
use crate::test_bus::{TestBus, Transfer, access};

fn memory(address: u32, function_code: FunctionCode, predecrement: bool) -> Operand {
    Operand::Memory {
        address,
        function_code,
        predecrement,
    }
}

#[test]
fn memory_reads_writes_preserve_function_code_purpose_and_byte_lanes() {
    for function_code in [
        FunctionCode::UserData,
        FunctionCode::SupervisorData,
        FunctionCode::UserProgram,
        FunctionCode::SupervisorProgram,
    ] {
        for (size, address, expected, lane) in [
            (Size::Byte, 0xab00_1000, 0x12, ByteLanes::Upper),
            (Size::Byte, 0xab00_1001, 0x34, ByteLanes::Lower),
            (Size::Word, 0xab00_1000, 0x1234, ByteLanes::Both),
            (Size::Long, 0xab00_1000, 0x1234_5678, ByteLanes::Both),
        ] {
            let mut regs = Registers::new();
            let mut bus = TestBus::default();
            bus.seed(0x1000, &[0x12, 0x34, 0x56, 0x78]);
            let operand = memory(address, function_code, false);
            assert_eq!(ea::read(&regs, &mut bus, operand, size), Ok(expected));
            ea::write(&mut regs, &mut bus, operand, size, 0x9abc_def0).unwrap();
            let mut expected_transfers = vec![Transfer {
                access: access(address, 0x1000, function_code, AccessKind::Data, lane),
                direction: Direction::Read,
                value: 0x1234,
            }];
            if size == Size::Long {
                expected_transfers.push(Transfer {
                    access: access(
                        address + 2,
                        0x1002,
                        function_code,
                        AccessKind::Data,
                        ByteLanes::Both,
                    ),
                    direction: Direction::Read,
                    value: 0x5678,
                });
            }
            expected_transfers.push(Transfer {
                access: access(address, 0x1000, function_code, AccessKind::Data, lane),
                direction: Direction::Write,
                value: match size {
                    Size::Byte if lane == ByteLanes::Upper => 0xf000,
                    Size::Byte => 0x00f0,
                    Size::Word => 0xdef0,
                    Size::Long => 0x9abc,
                },
            });
            if size == Size::Long {
                expected_transfers.push(Transfer {
                    access: access(
                        address + 2,
                        0x1002,
                        function_code,
                        AccessKind::Data,
                        ByteLanes::Both,
                    ),
                    direction: Direction::Write,
                    value: 0xdef0,
                });
            }
            assert_eq!(bus.transfers, expected_transfers);
            let expected_bytes = match (size, lane) {
                (Size::Byte, ByteLanes::Upper) => [0xf0, 0x34, 0x56, 0x78],
                (Size::Byte, ByteLanes::Lower) => [0x12, 0xf0, 0x56, 0x78],
                (Size::Word, _) => [0xde, 0xf0, 0x56, 0x78],
                (Size::Long, _) => [0x9a, 0xbc, 0xde, 0xf0],
                _ => unreachable!(),
            };
            assert_eq!(
                (0x1000..0x1004)
                    .map(|address| bus.peek(address))
                    .collect::<Vec<_>>(),
                expected_bytes
            );
        }
    }
}

#[test]
fn predecrement_long_writes_low_first_with_observable_partial_failure() {
    for address in [0xab00_1000_u32, 0x00ff_fffe, 0xffff_fffe] {
        for fail_at in [None, Some(0), Some(1)] {
            let mut regs = Registers::new();
            let mut bus = TestBus {
                fail_at,
                ..TestBus::default()
            };
            bus.seed(address, &[0xaa, 0xbb, 0xcc, 0xdd]);
            let function_code = FunctionCode::SupervisorData;
            let result = ea::write(
                &mut regs,
                &mut bus,
                memory(address, function_code, true),
                Size::Long,
                0x1234_5678,
            );
            let second = address.wrapping_add(2);
            let expected_transfers = [
                Transfer {
                    access: access(
                        second,
                        second & 0x00ff_fffe,
                        function_code,
                        AccessKind::Data,
                        ByteLanes::Both,
                    ),
                    direction: Direction::Write,
                    value: 0x5678,
                },
                Transfer {
                    access: access(
                        address,
                        address & 0x00ff_fffe,
                        function_code,
                        AccessKind::Data,
                        ByteLanes::Both,
                    ),
                    direction: Direction::Write,
                    value: 0x1234,
                },
            ];
            let count = if fail_at == Some(0) { 1 } else { 2 };
            assert_eq!(bus.transfers, expected_transfers[..count]);
            if let Some(failed) = fail_at {
                assert_eq!(
                    result,
                    Err(EaError::Bus(BusFault::Transfer {
                        access: expected_transfers[failed].access,
                        direction: Direction::Write,
                        source: failed,
                    }))
                );
            } else {
                assert_eq!(result, Ok(()));
            }
            let bytes = (0..4)
                .map(|offset| bus.peek(address.wrapping_add(offset)))
                .collect::<Vec<_>>();
            assert_eq!(
                bytes,
                match fail_at {
                    Some(0) => [0xaa, 0xbb, 0xcc, 0xdd],
                    Some(1) => [0xaa, 0xbb, 0x56, 0x78],
                    _ => [0x12, 0x34, 0x56, 0x78],
                }
            );
            let mut read_bus = TestBus::default();
            read_bus.seed(address, &[0x12, 0x34, 0x56, 0x78]);
            assert_eq!(
                ea::read(
                    &regs,
                    &mut read_bus,
                    memory(address, function_code, true),
                    Size::Long
                ),
                Ok(0x1234_5678)
            );
            assert_eq!(
                read_bus
                    .transfers
                    .iter()
                    .map(|transfer| transfer.access.logical_address)
                    .collect::<Vec<_>>(),
                [address, second]
            );
        }
    }
}

#[test]
fn extension_faults_preserve_completed_transfers_and_do_not_advance_failed_fetch() {
    for (mode, reg, size, count) in [
        (5, 0, Size::Word, 1),
        (6, 0, Size::Word, 1),
        (7, 0, Size::Word, 1),
        (7, 1, Size::Long, 2),
        (7, 2, Size::Word, 1),
        (7, 3, Size::Word, 1),
        (7, 4, Size::Long, 2),
    ] {
        for failed in 0..count {
            let mut regs = registers(false);
            let before = regs;
            let mut bus = TestBus {
                fail_at: Some(failed),
                ..TestBus::default()
            };
            let mut cursor = 0xffff_fffe;
            let failed_address = if failed == 0 { 0xffff_fffe } else { 0 };
            assert_eq!(
                ea::resolve(&mut regs, &mut bus, &mut cursor, mode, reg, size),
                Err(EaError::Bus(BusFault::Transfer {
                    access: access(
                        failed_address,
                        failed_address & 0x00ff_fffe,
                        FunctionCode::UserProgram,
                        AccessKind::Extension,
                        ByteLanes::Both
                    ),
                    direction: Direction::Read,
                    source: failed,
                }))
            );
            assert_eq!(regs, before);
            assert_eq!(cursor, failed_address);
            assert_eq!(bus.transfers.len(), failed + 1);
        }
    }
    let mut bus = TestBus::default();
    let mut cursor = 0x1001;
    assert!(matches!(
        ea::fetch_word(&mut bus, &mut cursor, FunctionCode::SupervisorProgram),
        Err(EaError::Bus(BusFault::AddressError {
            direction: Direction::Read,
            ..
        }))
    ));
    assert_eq!(cursor, 0x1001);
    assert!(bus.transfers.is_empty());
}

#[test]
fn auto_update_wraps_and_memory_faults_keep_the_resolved_register_update() {
    for mode in [3, 4] {
        for size in [Size::Byte, Size::Word, Size::Long] {
            let mut regs = Registers::new();
            regs.a[0] = if mode == 3 { u32::MAX } else { 0 };
            let before = regs.a[0];
            let mut bus = TestBus {
                fail_at: Some(0),
                ..TestBus::default()
            };
            let mut cursor = 0;
            let operand = ea::resolve(&mut regs, &mut bus, &mut cursor, mode, 0, size).unwrap();
            assert_eq!(
                regs.a[0],
                if mode == 3 {
                    before.wrapping_add(size.bytes())
                } else {
                    before.wrapping_sub(size.bytes())
                }
            );
            assert!(matches!(
                ea::read(&regs, &mut bus, operand, size),
                Err(EaError::Bus(_))
            ));
            assert_eq!(cursor, 0);
        }
    }
}

#[test]
fn nominal_ea_cycles_match_all_legal_entries_in_the_68000_table() {
    for reg in 0..8 {
        for (mode, word, long) in [
            (0, 0, 0),
            (1, 0, 0),
            (2, 4, 8),
            (3, 4, 8),
            (4, 6, 10),
            (5, 8, 12),
            (6, 10, 14),
        ] {
            assert_eq!(ea::read_cycles(mode, reg, Size::Byte), Some(word));
            assert_eq!(ea::read_cycles(mode, reg, Size::Word), Some(word));
            assert_eq!(ea::read_cycles(mode, reg, Size::Long), Some(long));
        }
    }
    for (reg, word, long) in [(0, 8, 12), (1, 12, 16), (2, 8, 12), (3, 10, 14), (4, 4, 8)] {
        assert_eq!(ea::read_cycles(7, reg, Size::Byte), Some(word));
        assert_eq!(ea::read_cycles(7, reg, Size::Word), Some(word));
        assert_eq!(ea::read_cycles(7, reg, Size::Long), Some(long));
    }
    for (mode, reg) in [(7, 5), (7, 6), (7, 7), (8, 0), (0, 8)] {
        assert_eq!(ea::read_cycles(mode, reg, Size::Long), None);
    }
}
use super::registers;
