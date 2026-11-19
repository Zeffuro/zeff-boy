use super::*;
use crate::diagnostic::{Callback, Capture, InstructionWatch, Outcome, WriteRange, WriteWatch};

pub(super) fn sound_failure_capture() -> Capture {
    Capture::begin_watching_writes(
        WriteWatch {
            ranges: [
                Some(WriteRange {
                    first: 0xc0479c,
                    last: 0xc0479d,
                }),
                None,
                None,
                None,
            ],
        },
        None,
    )
    .unwrap()
}

fn machine(concurrent: bool) -> Emulator {
    let mut rom = vec![0; 0x1000];
    rom[..4].copy_from_slice(&0x00ff8000_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    let words = [
        0x33fc_u16, 0x1234, 0x00ff, 0x0020, 0x13fc, 0x0056, 0x00ff, 0x0021, 0x13fc, 0x0078, 0x00ff,
        0x0020, 0x33fc, 0x1234, 0x00ff, 0x0020, 0x33fc, 0x1234, 0x00ff, 0x0020, 0x1039, 0x00ff,
        0x0020, 0x60fe,
    ];
    for (i, word) in words.into_iter().enumerate() {
        rom[0x200 + 2 * i..0x202 + 2 * i].copy_from_slice(&word.to_be_bytes());
    }
    Emulator::new(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: concurrent,
        },
    )
    .unwrap()
}

pub(super) fn ordinary_fields_preserve_completed_writes() {
    crate::diagnostic::assert_write_watch_contract(&machine(false));
    let watch = WriteWatch {
        ranges: [
            Some(WriteRange {
                first: 0xff0020,
                last: 0xff0021,
            }),
            Some(WriteRange {
                first: 0xff0021,
                last: 0xff0022,
            }),
            None,
            None,
        ],
    };
    for concurrent in [false, true] {
        suppressed_rmw_is_a_completed_callback(concurrent);
        let mut actual = machine(concurrent);
        let mut control = actual.clone();
        for _ in 0..2 {
            control.step_field().unwrap();
        }
        let capture = Capture::begin_watching_writes(
            watch,
            Some(InstructionWatch {
                pc: 0x218,
                data_register: None,
            }),
        )
        .unwrap();
        for _ in 0..2 {
            actual.step_field().unwrap();
        }
        let report = capture.finish(&actual).unwrap();
        assert!(report.complete);
        assert!(report.instruction_match.unwrap().complete);
        let history = report.write_history.unwrap();
        assert_eq!(history.schema, 1);
        assert!(history.complete && !history.overflow);
        assert_eq!((history.total_matched, history.dropped), (5, 0));
        let writes = &history.observations;
        assert_eq!(writes.len(), 5);
        for (i, pc) in [0x200, 0x208, 0x210, 0x218, 0x220].into_iter().enumerate() {
            let observed = writes[i];
            assert_eq!(observed.entry.snapshot.registers.pc, pc);
            assert_eq!(observed.entry.boundary, observed.record.boundary);
            assert_eq!(observed.record.callback, Callback::Write);
            assert_eq!(observed.record.outcome, Some(Outcome::Complete));
            assert!(observed.record.service.is_some() && observed.record.returned.is_some());
        }
        assert_eq!(writes[3].record.write_value, writes[4].record.write_value);
        assert_eq!(writes[0].record.access.unwrap().lanes, ByteLanes::Both);
        assert_eq!(writes[1].record.access.unwrap().lanes, ByteLanes::Lower);
        assert_eq!(writes[2].record.access.unwrap().lanes, ByteLanes::Upper);
        compare(&mut actual, &mut control, &mut Sha256::new());
    }
}

fn suppressed_rmw_is_a_completed_callback(concurrent: bool) {
    let mut rom = vec![0; 0x1000];
    rom[..4].copy_from_slice(&0x00ff8000_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    let words = [
        0x13fc_u16, 0x0012, 0x00ff, 0x0030, 0x4af9, 0x00ff, 0x0030, 0x1239, 0x00ff, 0x0030, 0x13fc,
        0x0034, 0x00ff, 0x0030, 0x1439, 0x00ff, 0x0030, 0x60fe,
    ];
    for (i, word) in words.into_iter().enumerate() {
        rom[0x200 + 2 * i..0x202 + 2 * i].copy_from_slice(&word.to_be_bytes());
    }
    let mut actual = Emulator::new(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: concurrent,
        },
    )
    .unwrap();
    let mut control = actual.clone();
    for _ in 0..2 {
        control.step_field().unwrap();
    }
    let capture = Capture::begin_watching_writes(
        WriteWatch {
            ranges: [
                Some(WriteRange {
                    first: 0xff0030,
                    last: 0xff0030,
                }),
                None,
                None,
                None,
            ],
        },
        None,
    )
    .unwrap();
    for _ in 0..2 {
        actual.step_field().unwrap();
    }
    let report = capture.finish(&actual).unwrap();
    assert!(report.complete);
    let history = report.write_history.unwrap();
    assert!(history.complete && !history.overflow);
    assert_eq!((history.total_matched, history.dropped), (3, 0));
    let writes = &history.observations;
    assert_eq!(writes.len(), 3);
    for (i, pc) in [0x200, 0x208, 0x214].into_iter().enumerate() {
        assert_eq!(writes[i].entry.snapshot.registers.pc, pc);
        assert_eq!(writes[i].record.callback, Callback::Write);
        assert_eq!(writes[i].record.outcome, Some(Outcome::Complete));
        assert_eq!(writes[i].record.service.unwrap().rmw, i == 1);
        assert_eq!(writes[i].record.returned.unwrap().bus.rmw, i == 1);
    }
    assert_eq!(writes[1].entry.snapshot.queued_words.unwrap()[0], 0x4af9);
    assert_eq!(writes[1].record.access.unwrap().lanes, ByteLanes::Upper);
    assert_eq!(writes[1].record.write_value.unwrap() >> 8, 0x92);
    assert_eq!(actual.registers().d[1] & 255, 0x12);
    assert_eq!(actual.registers().d[2] & 255, 0x34);
    compare(&mut actual, &mut control, &mut Sha256::new());
}
