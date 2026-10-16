use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, VdpH32Timing, VdpHBlankEdge, VdpMemory, VdpTimingError, VdpWrite, VdpWriteFifo,
};

pub fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}

pub fn full_lines_with(
    mut flag: impl FnMut(&VdpH32Timing, MasterTicks) -> Result<bool, VdpTimingError>,
    mut next: impl FnMut(&VdpH32Timing, MasterTicks) -> Result<VdpHBlankEdge, VdpTimingError>,
) {
    let events = [
        (100, false),
        (2940, true),
        (3520, false),
        (6360, true),
        (6940, false),
        (9780, true),
        (10360, false),
    ];
    for epoch in [0, 7, 10000] {
        let clock = VdpH32Timing::at_counter_zero(tick(epoch));
        let saved = clock;
        let mut blank_ticks = 0;
        for elapsed in 0..10260 {
            let phase = elapsed % 3420;
            let expected = matches!(phase, 0..=99 | 2940..=3419);
            assert_eq!(flag(&clock, tick(epoch + elapsed)), Ok(expected));
            blank_ticks += usize::from(expected);
            let &(offset, active) = events.iter().find(|(offset, _)| *offset > elapsed).unwrap();
            assert_eq!(
                next(&clock, tick(epoch + elapsed)),
                Ok(VdpHBlankEdge {
                    at: tick(epoch + offset),
                    active
                })
            );
            assert_eq!(clock, saved);
        }
        assert_eq!(blank_ticks, 1740);
    }
}

pub fn full_lines() {
    full_lines_with(
        |clock, at| clock.hblank_flag_at(at),
        |clock, at| clock.next_hblank_edge(at),
    );
}

pub fn literal_edges_and_copies() {
    let epoch = 137;
    let clock = VdpH32Timing::at_counter_zero(tick(epoch));
    let copy = clock;
    let mut after = tick(epoch);
    for (offset, active) in [
        (100, false),
        (2940, true),
        (3520, false),
        (6360, true),
        (6940, false),
        (9780, true),
    ] {
        let expected = VdpHBlankEdge {
            at: tick(epoch + offset),
            active,
        };
        let edge = clock.next_hblank_edge(after).unwrap();
        assert_eq!(edge, expected);
        assert_eq!(copy.next_hblank_edge(after), Ok(edge));
        assert_eq!(clock.hblank_flag_at(tick(edge.at.get() - 1)), Ok(!active));
        assert_eq!(clock.hblank_flag_at(edge.at), Ok(active));
        assert_eq!(clock.hblank_flag_at(tick(edge.at.get() + 1)), Ok(active));
        assert_eq!(copy.hblank_flag_at(edge.at), Ok(active));
        after = edge.at;
    }
    for (offset, counter, flag) in [
        (99, 9, true),
        (100, 10, false),
        (2939, 0x125, false),
        (2940, 0x126, true),
        (2959, 0x127, true),
        (2960, 0x1d2, true),
        (3419, 0x1ff, true),
        (3420, 0, true),
    ] {
        assert_eq!(clock.h_counter_at(tick(epoch + offset)), Ok(counter));
        assert_eq!(clock.hblank_flag_at(tick(epoch + offset)), Ok(flag));
    }
    assert_eq!(clock.hblank_flag_at(tick(epoch)), Ok(true));
}

pub fn epoch_and_overflow_guards_with(
    mut flag: impl FnMut(&VdpH32Timing, MasterTicks) -> Result<bool, VdpTimingError>,
    mut next: impl FnMut(&VdpH32Timing, MasterTicks) -> Result<VdpHBlankEdge, VdpTimingError>,
) {
    let clock = VdpH32Timing::at_counter_zero(tick(17));
    let saved = clock;
    for at in [0, 16] {
        let error = VdpTimingError::BeforeEpoch {
            epoch: tick(17),
            requested: tick(at),
        };
        assert_eq!(flag(&clock, tick(at)), Err(error));
        assert_eq!(next(&clock, tick(at)), Err(error));
        assert_eq!(clock, saved);
    }
    for (distance, active) in [
        (100, false),
        (2940, true),
        (3520, false),
        (6360, true),
        (6940, false),
    ] {
        let clock = VdpH32Timing::at_counter_zero(tick(u64::MAX - distance));
        let saved = clock;
        assert_eq!(flag(&clock, tick(u64::MAX - 1)), Ok(!active));
        assert_eq!(
            next(&clock, tick(u64::MAX - 1)),
            Ok(VdpHBlankEdge {
                at: tick(u64::MAX),
                active
            })
        );
        assert_eq!(flag(&clock, tick(u64::MAX)), Ok(active));
        assert_eq!(
            next(&clock, tick(u64::MAX)),
            Err(VdpTimingError::ClockOverflow)
        );
        assert_eq!(clock, saved);
    }
    let last = VdpH32Timing::at_counter_zero(tick(u64::MAX));
    assert_eq!(flag(&last, tick(u64::MAX)), Ok(true));
    assert_eq!(
        next(&last, tick(u64::MAX)),
        Err(VdpTimingError::ClockOverflow)
    );
    let partial = VdpH32Timing::at_counter_zero(tick(u64::MAX - 99));
    assert_eq!(flag(&partial, tick(u64::MAX)), Ok(true));
    assert_eq!(
        next(&partial, tick(u64::MAX)),
        Err(VdpTimingError::ClockOverflow)
    );
}

pub fn epoch_and_overflow_guards() {
    epoch_and_overflow_guards_with(
        |clock, at| clock.hblank_flag_at(at),
        |clock, at| clock.next_hblank_edge(at),
    );
}

pub fn flag_edges_do_not_service_fifo() {
    let epoch = 1000;
    let clock = VdpH32Timing::at_counter_zero(tick(epoch));
    let mut memory = VdpMemory::default();
    let mut fifo = VdpWriteFifo::default();
    for index in 0..4 {
        fifo.push(VdpWrite {
            command: ControlCommand {
                address: index * 2,
                code: 1,
            },
            data: 0x1234 + index as u16,
        })
        .unwrap();
    }
    let slot = clock.next_nonactive_write_slot(tick(epoch + 2929)).unwrap();
    assert_eq!(slot, tick(epoch + 2930));
    assert_eq!(clock.hblank_flag_at(slot), Ok(false));
    assert_eq!(fifo.service_write_slot(&mut memory), Ok(None));
    let saved_fifo = fifo.clone();
    let saved_memory = memory.clone();
    let edge = clock.next_hblank_edge(slot).unwrap();
    assert_eq!(
        edge,
        VdpHBlankEdge {
            at: tick(epoch + 2940),
            active: true
        }
    );
    assert_eq!(clock.hblank_flag_at(edge.at), Ok(true));
    assert_eq!(fifo, saved_fifo);
    assert_eq!(memory, saved_memory);
    assert_eq!(fifo.status_bits(), 0x0100);
    let next_slot = clock.next_nonactive_write_slot(edge.at).unwrap();
    assert_eq!(next_slot, tick(epoch + 2950));
    let mut copied_fifo = fifo.clone();
    let mut copied_memory = memory.clone();
    assert!(fifo.service_write_slot(&mut memory).unwrap().is_some());
    assert!(
        copied_fifo
            .service_write_slot(&mut copied_memory)
            .unwrap()
            .is_some()
    );
    assert_eq!(fifo, copied_fifo);
    assert_eq!(memory, copied_memory);
    assert_eq!(
        memory.read_word(ControlCommand {
            address: 0,
            code: 0
        }),
        Ok(0x1234)
    );
    assert_eq!(fifo.status_bits(), 0);
    assert_eq!(clock.hblank_flag_at(next_slot), Ok(true));
}
