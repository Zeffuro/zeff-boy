use super::*;

fn test_access(address: u32, lanes: ByteLanes) -> BusAccess {
    use zeff_m68k::bus::{AccessKind, FunctionCode};
    BusAccess {
        logical_address: address,
        bus_address: address,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes,
    }
}

fn test_record(
    machine: &crate::Emulator,
    callback: Callback,
    access: Option<BusAccess>,
) -> BusRecord {
    let context = machine.diagnostic_snapshot().context;
    BusRecord {
        sequence: 0,
        boundary: 0,
        callback,
        access,
        write_value: Some(0x1234),
        requested_start: 0,
        idle_clocks: 0,
        wait_clocks: 0,
        entry: context,
        service: Some(context.bus),
        vdp_control: None,
        returned: None,
        outcome: None,
    }
}

fn test_call(
    machine: &crate::Emulator,
    callback: Callback,
    access: Option<BusAccess>,
    outcome: Outcome,
) {
    let sequence = crate::diagnostic::begin_call(test_record(machine, callback, access)).unwrap();
    crate::diagnostic::end_call(sequence, machine.diagnostic_snapshot().context, 0, outcome);
}

pub(crate) fn assert_contract(machine: &crate::Emulator) {
    use crate::diagnostic::{MISSED, PhantomData, STATE, instruction_entry, observe};

    let watch = WriteWatch {
        ranges: [
            Some(WriteRange {
                first: 0x20,
                last: 0x21,
            }),
            Some(WriteRange {
                first: 0x21,
                last: 0x22,
            }),
            None,
            None,
        ],
    };
    let instruction = InstructionWatch {
        pc: machine.registers().pc,
        data_register: None,
    };
    let generation = STATE.with(|s| s.borrow().generation);
    for ranges in [
        [None; 4],
        [Some(WriteRange { first: 2, last: 1 }), None, None, None],
        [
            Some(WriteRange {
                first: 0,
                last: 0x100_0000,
            }),
            None,
            None,
            None,
        ],
    ] {
        assert!(matches!(
            Capture::begin_watching_writes(WriteWatch { ranges }, None),
            Err(CaptureError::InvalidWriteRange)
        ));
    }
    assert!(matches!(
        Capture::begin_watching_writes(
            watch,
            Some(InstructionWatch {
                data_register: Some((8, 0)),
                ..instruction
            })
        ),
        Err(CaptureError::InvalidDataRegister)
    ));
    assert_eq!(STATE.with(|s| s.borrow().generation), generation);
    let plain = Capture::begin().unwrap();
    observe(|r| assert!(r.writes.is_none()));
    assert!(plain.finish(machine).unwrap().write_history.is_none());
    let instruction_only = Capture::begin_watching(instruction).unwrap();
    observe(|r| assert!(r.writes.is_none()));
    assert!(
        instruction_only
            .finish(machine)
            .unwrap()
            .write_history
            .is_none()
    );

    let capture = Capture::begin_watching_writes(watch, Some(instruction)).unwrap();
    assert!(matches!(Capture::begin(), Err(CaptureError::AlreadyActive)));
    assert!(matches!(
        crate::diagnostic::field::Capture::begin(),
        Err(crate::diagnostic::field::CaptureError::OtherCaptureActive)
    ));
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(|| drop(Capture::begin().unwrap()))
        .join()
        .unwrap();
    let stale = Capture {
        generation: capture.generation + 1,
        finished: false,
        _owner_thread: PhantomData,
    };
    assert_eq!(stale.has_instruction_match(), Err(CaptureError::Stale));
    drop(stale);
    STATE.with(|s| {
        let _borrow = s.borrow_mut();
        assert_eq!(capture.has_instruction_match(), Err(CaptureError::Borrowed));
    });
    instruction_entry(machine.diagnostic_snapshot());
    assert_eq!(capture.has_instruction_match(), Ok(true));
    for (address, lanes) in [
        (0x20, ByteLanes::Upper),
        (0x20, ByteLanes::Lower),
        (0x20, ByteLanes::Both),
        (0x22, ByteLanes::Upper),
    ] {
        test_call(
            machine,
            Callback::Write,
            Some(test_access(address, lanes)),
            Outcome::Complete,
        );
    }
    for (address, lanes) in [(0x1e, ByteLanes::Both), (0x22, ByteLanes::Lower)] {
        test_call(
            machine,
            Callback::Write,
            Some(test_access(address, lanes)),
            Outcome::Complete,
        );
    }
    let mut unrelated = test_access(0x10020, ByteLanes::Both);
    unrelated.logical_address = 0x20;
    test_call(machine, Callback::Write, Some(unrelated), Outcome::Complete);
    for (callback, outcome) in [
        (Callback::Read, Outcome::Complete),
        (Callback::Idle, Outcome::Complete),
        (Callback::Write, Outcome::Unsupported),
        (
            Callback::Write,
            Outcome::UnsupportedAccess {
                address: 0x20,
                direction: zeff_m68k::bus::Direction::Write,
            },
        ),
    ] {
        test_call(
            machine,
            callback,
            Some(test_access(0x20, ByteLanes::Both)),
            outcome,
        );
    }
    let mut next_entry = machine.diagnostic_snapshot();
    next_entry.registers.pc += 2;
    instruction_entry(next_entry);
    test_call(
        machine,
        Callback::Write,
        Some(test_access(0x20, ByteLanes::Both)),
        Outcome::Complete,
    );
    let report = capture.finish(machine).unwrap();
    assert!(report.complete);
    assert!(report.instruction_match.unwrap().complete);
    assert_ne!(
        report.entry.unwrap().snapshot.registers.pc,
        next_entry.registers.pc
    );
    let history = report.write_history.unwrap();
    assert!(history.complete && !history.overflow);
    assert_eq!(
        (history.schema, history.total_matched, history.dropped),
        (1, 5, 0)
    );
    assert_eq!(history.observations.len(), 5);
    assert_eq!(
        history.observations.last().unwrap().entry.snapshot,
        next_entry
    );
    assert!(
        history
            .observations
            .windows(2)
            .all(|r| r[0].record.sequence < r[1].record.sequence)
    );
    assert_eq!(
        history.observations[0].record.write_value,
        history.observations[2].record.write_value
    );

    for range in [
        WriteRange { first: 0, last: 0 },
        WriteRange {
            first: 0xff_ffff,
            last: 0xff_ffff,
        },
    ] {
        let capture = Capture::begin_watching_writes(
            WriteWatch {
                ranges: [Some(range), None, None, None],
            },
            None,
        )
        .unwrap();
        instruction_entry(machine.diagnostic_snapshot());
        let (base, lanes) = if range.first == 0 {
            (0, ByteLanes::Upper)
        } else {
            (0xff_fffe, ByteLanes::Lower)
        };
        test_call(
            machine,
            Callback::Write,
            Some(test_access(base, lanes)),
            Outcome::Complete,
        );
        assert_eq!(
            capture
                .finish(machine)
                .unwrap()
                .write_history
                .unwrap()
                .total_matched,
            1
        );
    }

    let capture = Capture::begin_watching_writes(watch, None).unwrap();
    instruction_entry(machine.diagnostic_snapshot());
    for _ in 0..WRITE_HISTORY_CAPACITY + 3 {
        test_call(
            machine,
            Callback::Write,
            Some(test_access(0x20, ByteLanes::Both)),
            Outcome::Complete,
        );
    }
    let report = capture.finish(machine).unwrap();
    assert!(!report.complete);
    let history = report.write_history.unwrap();
    assert!(!history.complete && history.overflow);
    assert_eq!((history.total_matched, history.dropped), (259, 3));
    assert_eq!(history.observations.len(), WRITE_HISTORY_CAPACITY);
    assert_eq!(history.observations.last().unwrap().record.sequence, 255);

    for missing in 0..6 {
        let capture = Capture::begin_watching_writes(watch, None).unwrap();
        if missing != 0 {
            instruction_entry(machine.diagnostic_snapshot());
        }
        let mut record = test_record(
            machine,
            Callback::Write,
            Some(test_access(0x20, ByteLanes::Both)),
        );
        if missing == 1 {
            record.service = None;
        }
        if missing == 2 {
            record.write_value = None;
        }
        if missing == 3 {
            record.access = None;
        }
        let sequence = crate::diagnostic::begin_call(record).unwrap();
        if missing == 4 {
            observe(|r| r.writes.as_mut().unwrap().current_entry = None);
        }
        crate::diagnostic::end_call(
            sequence + u64::from(missing == 5),
            machine.diagnostic_snapshot().context,
            0,
            Outcome::Complete,
        );
        let report = capture.finish(machine).unwrap();
        assert!(!report.complete && !report.write_history.unwrap().complete);
    }

    let capture = Capture::begin_watching_writes(watch, None).unwrap();
    instruction_entry(machine.diagnostic_snapshot());
    STATE.with(|s| {
        let _borrow = s.borrow_mut();
        instruction_entry(machine.diagnostic_snapshot());
    });
    assert!(MISSED.with(std::cell::Cell::get));
    test_call(
        machine,
        Callback::Write,
        Some(test_access(0x20, ByteLanes::Both)),
        Outcome::Complete,
    );
    let report = capture.finish(machine).unwrap();
    assert!(!report.complete);
    let history = report.write_history.unwrap();
    assert!(!history.complete && history.observations.is_empty());

    let service_bus = crate::hardware::bus::Bus::new(
        crate::cartridge::Cartridge::new(machine.rom_bytes().to_vec()).unwrap(),
        crate::Region::NtscUs,
    );
    for invalid in 0..4 {
        let capture = Capture::begin_watching_writes(watch, None).unwrap();
        instruction_entry(machine.diagnostic_snapshot());
        let access = test_access(0x20, ByteLanes::Both);
        let mut record = test_record(machine, Callback::Write, Some(access));
        record.service = None;
        let sequence = crate::diagnostic::begin_call(record).unwrap();
        match invalid {
            0 => crate::diagnostic::service_entry(&service_bus, access, Callback::Write),
            1 => crate::diagnostic::service_entry(&service_bus, access, Callback::Read),
            2 => crate::diagnostic::service_entry(
                &service_bus,
                test_access(0x22, ByteLanes::Both),
                Callback::Write,
            ),
            _ => instruction_entry(machine.diagnostic_snapshot()),
        }
        crate::diagnostic::service_entry(&service_bus, access, Callback::Write);
        crate::diagnostic::end_call(
            sequence,
            machine.diagnostic_snapshot().context,
            0,
            Outcome::Complete,
        );
        let report = capture.finish(machine).unwrap();
        assert!(!report.complete);
        assert!(report.history.iter().flatten().any(|r| r.service.is_some()
            && r.returned.is_some()
            && r.outcome == Some(Outcome::Complete)));
        let history = report.write_history.unwrap();
        assert!(!history.complete && history.observations.is_empty());
    }

    let mut missing_return = WriteRecorder::new(watch);
    missing_return.current_entry = Some(Entry {
        boundary: 1,
        snapshot: machine.diagnostic_snapshot(),
    });
    let mut record = test_record(
        machine,
        Callback::Write,
        Some(test_access(0x20, ByteLanes::Both)),
    );
    record.boundary = 1;
    record.outcome = Some(Outcome::Complete);
    missing_return.completed(record, true);
    assert!(!missing_return.history.complete && missing_return.history.observations.is_empty());
    assert_eq!(
        (
            missing_return.history.total_matched,
            missing_return.history.dropped
        ),
        (1, 1)
    );

    let capture = Capture::begin_watching_writes(watch, None).unwrap();
    instruction_entry(machine.diagnostic_snapshot());
    test_call(
        machine,
        Callback::Write,
        Some(test_access(0x20, ByteLanes::Both)),
        Outcome::UnsupportedAccess {
            address: 0x20,
            direction: zeff_m68k::bus::Direction::Write,
        },
    );
    instruction_entry(machine.diagnostic_snapshot());
    test_call(
        machine,
        Callback::Write,
        Some(test_access(0x20, ByteLanes::Both)),
        Outcome::Complete,
    );
    for _ in 0..crate::diagnostic::HISTORY_CAPACITY + 1 {
        test_call(
            machine,
            Callback::Read,
            Some(test_access(0x100, ByteLanes::Both)),
            Outcome::Read(0),
        );
    }
    let report = capture.finish(machine).unwrap();
    assert!(!report.complete);
    let history = report.write_history.unwrap();
    assert!(history.complete && !history.overflow);
    assert_eq!(history.observations.len(), 1);

    let capture = Capture::begin_watching_writes(watch, None).unwrap();
    instruction_entry(machine.diagnostic_snapshot());
    crate::diagnostic::begin_call(test_record(
        machine,
        Callback::Write,
        Some(test_access(0x20, ByteLanes::Both)),
    ))
    .unwrap();
    let report = capture.finish(machine).unwrap();
    assert!(!report.complete);
    let history = report.write_history.unwrap();
    assert!(!history.complete && history.observations.is_empty());

    for exhausted in 0..3 {
        let capture = Capture::begin_watching_writes(watch, None).unwrap();
        instruction_entry(machine.diagnostic_snapshot());
        observe(|r| match exhausted {
            0 => r.writes.as_mut().unwrap().history.total_matched = u64::MAX,
            1 => {
                let w = r.writes.as_mut().unwrap();
                w.history.total_matched = WRITE_HISTORY_CAPACITY as u64;
                w.history.dropped = u64::MAX;
            }
            _ => r.boundary = u64::MAX,
        });
        if exhausted == 2 {
            instruction_entry(machine.diagnostic_snapshot());
        }
        test_call(
            machine,
            Callback::Write,
            Some(test_access(0x20, ByteLanes::Both)),
            Outcome::Complete,
        );
        let report = capture.finish(machine).unwrap();
        assert!(!report.complete && !report.write_history.unwrap().complete);
    }
    drop(Capture::begin_watching_writes(watch, None).unwrap());
    let field = crate::diagnostic::field::Capture::begin().unwrap();
    assert!(matches!(
        Capture::begin_watching_writes(watch, None),
        Err(CaptureError::AlreadyActive)
    ));
    drop(field);
    let fresh = Capture::begin().unwrap();
    assert!(fresh.finish(machine).unwrap().write_history.is_none());
}
