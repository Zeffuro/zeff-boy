use crate::bus::{
    AccessKind, BusFault, ByteLanes, Direction, FunctionCode, read_byte, read_long, read_word,
    write_byte, write_long, write_word,
};
use crate::test_bus::{TestBus, Transfer, access};

#[test]
fn bytes_select_physical_lanes_preserve_neighbor_and_keep_full_logical_address() {
    for function_code in [
        FunctionCode::UserData,
        FunctionCode::UserProgram,
        FunctionCode::SupervisorData,
        FunctionCode::SupervisorProgram,
    ] {
        for kind in [
            AccessKind::Data,
            AccessKind::Opcode,
            AccessKind::Extension,
            AccessKind::ResetVector,
        ] {
            let mut bus = TestBus::default();
            bus.word(0x1234, 0xa1b2);
            assert_eq!(
                read_byte(&mut bus, 0xcd00_1234, function_code, kind),
                Ok(0xa1)
            );
            assert_eq!(
                read_byte(&mut bus, 0xcd00_1235, function_code, kind),
                Ok(0xb2)
            );
            write_byte(&mut bus, 0xcd00_1234, 0x3c, function_code, kind).unwrap();
            assert_eq!((bus.peek(0x1234), bus.peek(0x1235)), (0x3c, 0xb2));
            write_byte(&mut bus, 0xcd00_1235, 0x4d, function_code, kind).unwrap();
            assert_eq!((bus.peek(0x1234), bus.peek(0x1235)), (0x3c, 0x4d));
            let upper = access(0xcd00_1234, 0x1234, function_code, kind, ByteLanes::Upper);
            let lower = access(0xcd00_1235, 0x1234, function_code, kind, ByteLanes::Lower);
            assert_eq!(
                bus.transfers,
                [
                    Transfer {
                        access: upper,
                        direction: Direction::Read,
                        value: 0xa1b2
                    },
                    Transfer {
                        access: lower,
                        direction: Direction::Read,
                        value: 0xa1b2
                    },
                    Transfer {
                        access: upper,
                        direction: Direction::Write,
                        value: 0x3c00
                    },
                    Transfer {
                        access: lower,
                        direction: Direction::Write,
                        value: 0x004d
                    },
                ]
            );
        }
    }
}

#[test]
fn longs_are_ordered_big_endian_words_across_external_and_architectural_wrap() {
    for start in [0xab00_1234, 0x00ff_fffe, 0xffff_fffe] {
        let mut bus = TestBus::default();
        bus.seed(start, &[0x12, 0x34, 0xab, 0xcd]);
        assert_eq!(
            read_long(&mut bus, start, FunctionCode::UserData, AccessKind::Data),
            Ok(0x1234_abcd)
        );
        write_long(
            &mut bus,
            start,
            0x7654_fedc,
            FunctionCode::SupervisorData,
            AccessKind::Data,
        )
        .unwrap();
        assert_eq!(
            (0..4)
                .map(|offset| bus.peek(start.wrapping_add(offset)))
                .collect::<Vec<_>>(),
            [0x76, 0x54, 0xfe, 0xdc]
        );
        let second = start.wrapping_add(2);
        assert_eq!(
            bus.transfers,
            [
                Transfer {
                    access: access(
                        start,
                        start & 0x00ff_fffe,
                        FunctionCode::UserData,
                        AccessKind::Data,
                        ByteLanes::Both
                    ),
                    direction: Direction::Read,
                    value: 0x1234
                },
                Transfer {
                    access: access(
                        second,
                        second & 0x00ff_fffe,
                        FunctionCode::UserData,
                        AccessKind::Data,
                        ByteLanes::Both
                    ),
                    direction: Direction::Read,
                    value: 0xabcd
                },
                Transfer {
                    access: access(
                        start,
                        start & 0x00ff_fffe,
                        FunctionCode::SupervisorData,
                        AccessKind::Data,
                        ByteLanes::Both
                    ),
                    direction: Direction::Write,
                    value: 0x7654
                },
                Transfer {
                    access: access(
                        second,
                        second & 0x00ff_fffe,
                        FunctionCode::SupervisorData,
                        AccessKind::Data,
                        ByteLanes::Both
                    ),
                    direction: Direction::Write,
                    value: 0xfedc
                },
            ]
        );
    }
}

#[test]
fn odd_words_and_longs_reject_before_any_callback() {
    let address = 0xdead_beef;
    let metadata = access(
        address,
        0x00ad_beee,
        FunctionCode::SupervisorData,
        AccessKind::Data,
        ByteLanes::Both,
    );
    let mut bus = TestBus::default();
    let read_fault = BusFault::AddressError {
        access: metadata,
        direction: Direction::Read,
    };
    let write_fault = BusFault::AddressError {
        access: metadata,
        direction: Direction::Write,
    };
    assert_eq!(
        read_word(
            &mut bus,
            address,
            FunctionCode::SupervisorData,
            AccessKind::Data
        ),
        Err(read_fault.clone())
    );
    assert_eq!(
        read_long(
            &mut bus,
            address,
            FunctionCode::SupervisorData,
            AccessKind::Data
        ),
        Err(read_fault)
    );
    assert_eq!(
        write_word(
            &mut bus,
            address,
            1,
            FunctionCode::SupervisorData,
            AccessKind::Data
        ),
        Err(write_fault.clone())
    );
    assert_eq!(
        write_long(
            &mut bus,
            address,
            1,
            FunctionCode::SupervisorData,
            AccessKind::Data
        ),
        Err(write_fault)
    );
    assert!(bus.transfers.is_empty());
}

#[test]
fn failed_second_long_write_retains_first_word_and_reports_second_logical_address() {
    let mut bus = TestBus {
        fail_at: Some(1),
        ..TestBus::default()
    };
    bus.seed(0xffff_fffe, &[0xaa, 0xbb, 0xcc, 0xdd]);
    assert_eq!(
        write_long(
            &mut bus,
            0xffff_fffe,
            0x1234_5678,
            FunctionCode::UserData,
            AccessKind::Data
        ),
        Err(BusFault::Transfer {
            access: access(
                0,
                0,
                FunctionCode::UserData,
                AccessKind::Data,
                ByteLanes::Both
            ),
            direction: Direction::Write,
            source: 1,
        })
    );
    assert_eq!(
        [
            bus.peek(0xffff_fffe),
            bus.peek(0xffff_ffff),
            bus.peek(0),
            bus.peek(1)
        ],
        [0x12, 0x34, 0xcc, 0xdd]
    );
    assert_eq!(bus.transfers.len(), 2);
    assert_eq!(bus.transfers[0].access.logical_address, 0xffff_fffe);
    assert_eq!(bus.transfers[1].access.logical_address, 0);
}

#[test]
fn failed_long_reads_report_the_transfer_that_failed() {
    for failed in 0..2 {
        let mut bus = TestBus {
            fail_at: Some(failed),
            ..TestBus::default()
        };
        let address = 0xabff_fffe_u32.wrapping_add(u32::try_from(failed).unwrap() * 2);
        assert_eq!(
            read_long(
                &mut bus,
                0xabff_fffe,
                FunctionCode::UserProgram,
                AccessKind::Extension
            ),
            Err(BusFault::Transfer {
                access: access(
                    address,
                    address & 0x00ff_fffe,
                    FunctionCode::UserProgram,
                    AccessKind::Extension,
                    ByteLanes::Both
                ),
                direction: Direction::Read,
                source: failed,
            })
        );
        assert_eq!(bus.transfers.len(), failed + 1);
    }
}

#[test]
fn byte_faults_retain_odd_logical_address_and_write_direction() {
    let metadata = access(
        0xf100_0001,
        0,
        FunctionCode::SupervisorData,
        AccessKind::Data,
        ByteLanes::Lower,
    );
    let mut bus = TestBus {
        fail_at: Some(0),
        ..TestBus::default()
    };
    assert_eq!(
        read_byte(
            &mut bus,
            0xf100_0001,
            FunctionCode::SupervisorData,
            AccessKind::Data
        ),
        Err(BusFault::Transfer {
            access: metadata,
            direction: Direction::Read,
            source: 0,
        })
    );
    bus.fail_at = Some(1);
    assert_eq!(
        write_byte(
            &mut bus,
            0xf100_0001,
            0xaa,
            FunctionCode::SupervisorData,
            AccessKind::Data
        ),
        Err(BusFault::Transfer {
            access: metadata,
            direction: Direction::Write,
            source: 1,
        })
    );
    assert_eq!(bus.peek(1), 0);
    assert_eq!(bus.transfers[1].value, 0x00aa);
}

#[test]
fn function_code_values_match_external_encoding() {
    assert_eq!(FunctionCode::UserData as u8, 1);
    assert_eq!(FunctionCode::UserProgram as u8, 2);
    assert_eq!(FunctionCode::SupervisorData as u8, 5);
    assert_eq!(FunctionCode::SupervisorProgram as u8, 6);
}
