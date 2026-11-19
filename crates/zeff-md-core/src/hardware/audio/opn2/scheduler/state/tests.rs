use super::*;

fn bytes(scheduler: Scheduler) -> Vec<u8> {
    let mut writer = Writer::new();
    scheduler.write_state(&mut writer);
    writer.into_bytes()
}

#[test]
fn pending_and_invalid_channel_commands_round_trip_exactly() {
    for channel in [0, 5, 255] {
        for step in [0, CLOCK_PREFIX - 1, CLOCK_PREFIX, CLOCK_END - 1] {
            let scheduler = Scheduler {
                step,
                command: Command {
                    channel: 255,
                    mask: 15,
                },
                pending: Some(Command { channel, mask: 10 }),
                frequency: (0x7ff, 7),
                frequency_valid: false,
                output: [8160, -32764, -8192, -3, 0, 32],
                ready: 37,
                accumulator: [-256, 255, 1, -1, 0, 42],
                carriers: [15, 0, 1, 2, 3, 7],
            };
            let saved = bytes(scheduler);
            let mut reader = Reader::new(&saved);
            assert_eq!(Scheduler::read_state(&mut reader).unwrap(), scheduler);
            assert!(reader.is_exhausted());
        }
    }
}

#[test]
fn malformed_scheduler_cursor_commands_frequency_and_output_reject() {
    let saved = bytes(Scheduler {
        pending: Some(Command {
            channel: 2,
            mask: 15,
        }),
        ..Scheduler::default()
    });
    for (offset, replacement) in [
        (0, CLOCK_END.to_le_bytes().to_vec()),
        (4, vec![6]),
        (5, vec![16]),
        (6, vec![2]),
        (7, vec![254]),
        (8, vec![16]),
        (9, 0x800_u16.to_le_bytes().to_vec()),
        (11, vec![8]),
        (12, vec![2]),
        (13, 32765_u32.to_le_bytes().to_vec()),
        (17, (-32765_i32).to_le_bytes().to_vec()),
        (37, vec![64]),
        (13, 31_u32.to_le_bytes().to_vec()),
        (17, (-8193_i32).to_le_bytes().to_vec()),
        (38, 256_u32.to_le_bytes().to_vec()),
        (42, (-257_i32).to_le_bytes().to_vec()),
        (62, vec![16]),
    ] {
        let mut bad = saved.clone();
        bad[offset..offset + replacement.len()].copy_from_slice(&replacement);
        assert!(
            Scheduler::read_state(&mut Reader::new(&bad)).is_err(),
            "offset {offset}"
        );
    }
    for length in 0..saved.len() {
        assert!(Scheduler::read_state(&mut Reader::new(&saved[..length])).is_err());
    }
}
