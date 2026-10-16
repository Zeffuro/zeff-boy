use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, VdpH32Timing, VdpMemory, VdpTimingError, VdpWrite, VdpWriteError, VdpWriteFifo,
};

pub fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}
pub fn command(address: u32, code: u8) -> ControlCommand {
    ControlCommand { address, code }
}
pub fn entry(address: u32, code: u8, data: u16) -> VdpWrite {
    VdpWrite {
        command: command(address, code),
        data,
    }
}

pub fn counter_units_jump_and_wrap() {
    for epoch in [0, 7, 10_000] {
        let clock = VdpH32Timing::at_counter_zero(tick(epoch));
        for line in 0..3 {
            for dot in 0..342 {
                let expected = if dot < 296 { dot } else { 466 + dot - 296 };
                for fraction in [0, 9] {
                    assert_eq!(
                        clock.h_counter_at(tick(epoch + line * 3420 + dot * 10 + fraction)),
                        Ok(expected as u16)
                    );
                }
            }
        }
        for (offset, counter) in [
            (0, 0),
            (9, 0),
            (10, 1),
            (2959, 0x127),
            (2960, 0x1d2),
            (2970, 0x1d3),
            (3419, 0x1ff),
            (3420, 0),
            (3430, 1),
        ] {
            assert_eq!(clock.h_counter_at(tick(epoch + offset)), Ok(counter));
        }
    }
}

pub fn nonactive_slots_skip_refresh() {
    let refresh = [530, 1170, 1810, 2450, 3310];
    let expected: Vec<_> = (0..3)
        .flat_map(|line| (0..171).map(move |slot| line * 3420 + 10 + slot * 20))
        .filter(|at| !refresh.contains(&(at % 3420)))
        .collect();
    assert_eq!(expected.len(), 498);
    for epoch in [0, 17] {
        let clock = VdpH32Timing::at_counter_zero(tick(epoch));
        let saved = clock;
        for after in 0..6840 {
            let next = expected.iter().copied().find(|at| *at > after).unwrap();
            assert_eq!(
                clock.next_nonactive_write_slot(tick(epoch + after)),
                Ok(tick(epoch + next))
            );
            assert_eq!(clock, saved);
        }
        for (after, next) in [
            (0, 10),
            (9, 10),
            (10, 30),
            (509, 510),
            (510, 550),
            (529, 550),
            (530, 550),
            (531, 550),
            (1169, 1190),
            (1809, 1830),
            (2449, 2470),
            (2950, 2970),
            (3290, 3330),
            (3309, 3330),
            (3410, 3430),
            (3420, 3430),
        ] {
            assert_eq!(
                clock.next_nonactive_write_slot(tick(epoch + after)),
                Ok(tick(epoch + next))
            );
        }
        let mut after = tick(epoch);
        for at in &expected {
            after = clock.next_nonactive_write_slot(after).unwrap();
            assert_eq!(after, tick(epoch + *at));
            assert!(clock.h_counter_at(after).unwrap() & 1 != 0);
        }
    }
}

pub fn calendar_drives_captured_fifo() {
    let clock = VdpH32Timing::at_counter_zero(tick(1000));
    let mut fifo = VdpWriteFifo::default();
    let mut memory = VdpMemory::default();
    memory.write_word(command(0, 1), 0xaabb).unwrap();
    let writes = [
        entry(0, 0x11, 0x1234),
        entry(2, 3, 0xfabc),
        entry(4, 5, 0xfd67),
        entry(3, 1, 0x5678),
    ];
    // These captured heads are already eligible; this is not a CPU arrival trace.
    for write in writes {
        fifo.push(write).unwrap();
    }
    let before = memory.clone();
    let first = clock.next_nonactive_write_slot(tick(1500)).unwrap();
    assert_eq!(first, tick(1510));
    assert_eq!(memory, before);
    assert_eq!(fifo.service_write_slot(&mut memory), Ok(None));
    assert_eq!(memory.read_word(command(0, 0)), Ok(0xaa34));
    assert_eq!(fifo.status_bits(), 0x0100);
    let saved_fifo = fifo.clone();
    assert_eq!(fifo.push(entry(8, 1, 0)), Err(VdpWriteError::Full));
    assert_eq!(fifo, saved_fifo);
    let second = clock.next_nonactive_write_slot(first).unwrap();
    assert_eq!(second, tick(1550));
    assert_eq!(fifo.service_write_slot(&mut memory), Ok(Some(writes[0])));
    assert_eq!(memory.read_word(command(0, 0)), Ok(0x1234));
    let mut copied_fifo = fifo.clone();
    let mut copied_memory = memory.clone();
    let copied_clock = clock;
    let mut after = second;
    for (at, completed) in [
        (1570, Some(writes[1])),
        (1590, Some(writes[2])),
        (1610, None),
        (1630, Some(writes[3])),
    ] {
        let next = clock.next_nonactive_write_slot(after).unwrap();
        assert_eq!(next, tick(at));
        assert_eq!(copied_clock.next_nonactive_write_slot(after), Ok(next));
        assert_eq!(fifo.service_write_slot(&mut memory), Ok(completed));
        assert_eq!(
            copied_fifo.service_write_slot(&mut copied_memory),
            Ok(completed)
        );
        assert_eq!(fifo, copied_fifo);
        assert_eq!(memory, copied_memory);
        after = next;
    }
    assert_eq!(memory.cram()[1], 0x0aac);
    assert_eq!(memory.vsram()[2], 0x0567);
    assert_eq!(memory.read_word(command(2, 0)), Ok(0x7856));
    assert_eq!(fifo.status_bits(), 0x0200);
}

pub fn epoch_and_overflow_guards() {
    let clock = VdpH32Timing::at_counter_zero(tick(17));
    let saved = clock;
    for at in [0, 16] {
        let error = VdpTimingError::BeforeEpoch {
            epoch: tick(17),
            requested: tick(at),
        };
        assert_eq!(clock.h_counter_at(tick(at)), Err(error));
        assert_eq!(clock.next_nonactive_write_slot(tick(at)), Err(error));
        assert_eq!(clock, saved);
    }
    let clock = VdpH32Timing::at_counter_zero(tick(u64::MAX - 50));
    for (after, next) in [(50, 40), (40, 20), (20, 0)] {
        assert_eq!(
            clock.next_nonactive_write_slot(tick(u64::MAX - after)),
            Ok(tick(u64::MAX - next))
        );
    }
    assert_eq!(clock.h_counter_at(tick(u64::MAX)), Ok(5));
    assert_eq!(
        clock.next_nonactive_write_slot(tick(u64::MAX)),
        Err(VdpTimingError::ClockOverflow)
    );
    let refresh_edge = VdpH32Timing::at_counter_zero(tick(u64::MAX - 540));
    assert_eq!(refresh_edge.h_counter_at(tick(u64::MAX - 10)), Ok(0x35));
    assert_eq!(
        refresh_edge.next_nonactive_write_slot(tick(u64::MAX - 11)),
        Err(VdpTimingError::ClockOverflow)
    );
    let last = VdpH32Timing::at_counter_zero(tick(u64::MAX));
    assert_eq!(last.h_counter_at(tick(u64::MAX)), Ok(0));
    assert_eq!(
        last.next_nonactive_write_slot(tick(u64::MAX)),
        Err(VdpTimingError::ClockOverflow)
    );
}
