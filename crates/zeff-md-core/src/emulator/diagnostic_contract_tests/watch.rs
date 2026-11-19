use super::*;
use crate::diagnostic::{Callback, Capture, CaptureError, InstructionWatch, Outcome};

fn machine(concurrent: bool) -> Emulator {
    let mut rom = vec![0; 0x1000];
    rom[..4].copy_from_slice(&0x00ff8000_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    let program = [
        0x7e01_u16, 0x7001, 0x4eb9, 0x0000, 0x0300, 0x7e00, 0x4eb9, 0x0000, 0x0300, 0x60fe,
    ];
    for (index, word) in program.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    for index in 0..80 {
        rom[0x300 + index * 2..0x302 + index * 2].copy_from_slice(&0x4e71_u16.to_be_bytes());
    }
    rom[0x3a0..0x3a2].copy_from_slice(&0x4e75_u16.to_be_bytes());
    Emulator::new(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: concurrent,
        },
    )
    .unwrap()
}

pub(super) fn ordinary_fields_preserve_watch_and_machine() {
    crate::diagnostic::assert_watch_lifetime_contract(&machine(false));
    for concurrent in [false, true] {
        failure_and_watch_share_one_capture(concurrent);
        for predicate in [None, Some((7, 0)), Some((7, 2))] {
            let mut actual = machine(concurrent);
            let mut control = actual.clone();
            let mut hash = Sha256::new();
            for _ in 0..2 {
                control.step_field().unwrap();
            }
            let capture = Capture::begin_watching(InstructionWatch {
                pc: 0x300,
                data_register: predicate,
            })
            .unwrap();
            assert_eq!(capture.has_instruction_match(), Ok(false));
            assert!(matches!(Capture::begin(), Err(CaptureError::AlreadyActive)));
            assert!(matches!(
                crate::diagnostic::field::Capture::begin(),
                Err(crate::diagnostic::field::CaptureError::OtherCaptureActive)
            ));
            for _ in 0..2 {
                actual.step_field().unwrap();
                assert_eq!(
                    capture.has_instruction_match(),
                    Ok(predicate != Some((7, 2)))
                );
            }
            let report = capture.finish(&actual).unwrap();
            assert!(report.complete);
            assert_eq!(report.schema, 2);
            assert_eq!(report.first_failed_sequence, None);
            assert_eq!(report.terminal, actual.diagnostic_snapshot());
            compare(&mut actual, &mut control, &mut hash);
            let Some(found) = report.instruction_match else {
                assert_eq!(predicate, Some((7, 2)));
                continue;
            };
            assert!(found.complete);
            assert_eq!(found.schema, 1);
            assert_eq!(found.entry.snapshot.registers.pc, 0x300);
            assert_eq!(found.entry.snapshot.queued_words, Some([0x4e71, 0x4e71]));
            assert_eq!(
                found.entry.snapshot.registers.d[7],
                if predicate.is_none() { 1 } else { 0 }
            );
            assert_eq!(found.entry.snapshot.registers.stack_pointer(), 0xff7ffc);
            assert!(
                found.entry.snapshot.context.bus.master_ticks
                    < report.terminal.context.bus.master_ticks
            );
            let entries: Vec<_> = found.preceding_entries.iter().flatten().collect();
            assert_eq!(entries.len(), if predicate.is_none() { 3 } else { 16 });
            assert!(
                entries
                    .windows(2)
                    .all(|pair| pair[0].boundary + 1 == pair[1].boundary)
            );
            let caller = entries.last().unwrap();
            assert_eq!(caller.boundary + 1, found.entry.boundary);
            assert_eq!(
                caller.snapshot.registers.pc,
                if predicate.is_none() { 0x204 } else { 0x20c }
            );
            assert_eq!(caller.snapshot.queued_words, Some([0x4eb9, 0x0000]));
            assert_eq!(
                found.preceding_entries_overwritten + entries.len() as u64,
                found.entry.boundary - 1
            );
            let callbacks: Vec<_> = found.preceding_callbacks.iter().flatten().collect();
            if predicate.is_some() {
                assert_eq!(callbacks.len(), crate::diagnostic::HISTORY_CAPACITY);
                assert!(found.preceding_callbacks_overwritten > 0);
            }
            assert!(
                callbacks
                    .windows(2)
                    .all(|pair| pair[0].sequence + 1 == pair[1].sequence)
            );
            assert_eq!(
                callbacks.last().unwrap().sequence + 1,
                found.callbacks_before_entry
            );
            assert_eq!(
                found.preceding_callbacks_overwritten + callbacks.len() as u64,
                found.callbacks_before_entry
            );
            assert!(report.total_callbacks > found.callbacks_before_entry + 64);
            assert!(
                callbacks.iter().all(
                    |record| record.boundary < found.entry.boundary && record.outcome.is_some()
                )
            );
            let return_pc = if predicate.is_none() { 0x20a } else { 0x212 };
            for (address, value) in [(0xff7ffc, 0), (0xff7ffe, return_pc)] {
                assert!(callbacks.iter().any(|record| {
                    record.boundary == caller.boundary
                        && record.callback == Callback::Write
                        && record
                            .access
                            .is_some_and(|access| access.bus_address == address)
                        && record.write_value == Some(value)
                        && record.outcome == Some(Outcome::Complete)
                }));
            }
        }
        let field = crate::diagnostic::field::Capture::begin().unwrap();
        assert!(matches!(
            Capture::begin_watching(InstructionWatch {
                pc: 0x300,
                data_register: None
            }),
            Err(CaptureError::AlreadyActive)
        ));
        drop(field);
    }
}

fn failure_and_watch_share_one_capture(concurrent: bool) {
    let mut actual = fixture(concurrent, true, false, 0, false);
    let mut control = actual.clone();
    let expected = control.step_field();
    let capture = Capture::begin_watching(InstructionWatch {
        pc: 0x3ffe22,
        data_register: Some((0, 0xffc00200)),
    })
    .unwrap();
    assert_eq!(actual.step_field(), expected);
    assert!(expected.is_err());
    assert_eq!(capture.has_instruction_match(), Ok(true));
    let report = capture.finish(&actual).unwrap();
    assert_fault_report(&report, &actual, false, 0, false);
    let found = report.instruction_match.as_ref().unwrap();
    assert!(found.complete);
    assert_eq!(Some(found.entry), report.entry);
    assert_eq!(
        report.first_failed_sequence,
        Some(found.callbacks_before_entry)
    );
    assert!(
        found
            .preceding_callbacks
            .iter()
            .flatten()
            .all(
                |record| record.sequence < found.callbacks_before_entry && record.outcome.is_some()
            )
    );
    compare(&mut actual, &mut control, &mut Sha256::new());
}
