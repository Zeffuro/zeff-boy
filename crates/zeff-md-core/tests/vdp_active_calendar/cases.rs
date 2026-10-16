use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, VdpH32Timing, VdpMemory, VdpTimingError, VdpWrite, VdpWriteError, VdpWriteFifo,
};

const GRANTS: [u64; 16] = [
    50, 210, 370, 690, 850, 1010, 1330, 1490, 1650, 1970, 2130, 2290, 2590, 2610, 2890, 3170,
];
const COUNTERS: [u16; 16] = [
    0x005, 0x015, 0x025, 0x045, 0x055, 0x065, 0x085, 0x095, 0x0a5, 0x0c5, 0x0d5, 0x0e5, 0x103,
    0x105, 0x121, 0x1e7,
];

fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}

fn next(clock: &VdpH32Timing, at: MasterTicks) -> Result<MasterTicks, VdpTimingError> {
    clock.next_active_write_slot(at)
}

fn qualifies(clock: &VdpH32Timing, at: MasterTicks) -> Result<bool, VdpTimingError> {
    clock.is_active_write_slot(at)
}

fn oracle_next(after: u64) -> u64 {
    let cycle = after / 3420;
    GRANTS
        .iter()
        .copied()
        .find(|offset| cycle * 3420 + offset > after)
        .map_or((cycle + 1) * 3420 + 50, |offset| cycle * 3420 + offset)
}

pub fn literal_grants_and_counter_coordinates() {
    literal_grants_and_counter_coordinates_with(next, qualifies);
}

pub fn literal_grants_and_counter_coordinates_with(
    next: impl Fn(&VdpH32Timing, MasterTicks) -> Result<MasterTicks, VdpTimingError>,
    qualifies: impl Fn(&VdpH32Timing, MasterTicks) -> Result<bool, VdpTimingError>,
) {
    for epoch in [0, 3, 10_007] {
        let clock = VdpH32Timing::at_counter_zero(tick(epoch));
        let saved = clock;
        let mut after = tick(epoch);
        for cycle in 0..3 {
            for (offset, counter) in GRANTS.into_iter().zip(COUNTERS) {
                let at = tick(epoch + cycle * 3420 + offset);
                assert_eq!(next(&clock, after), Ok(at));
                assert_eq!(qualifies(&clock, at), Ok(true));
                assert_eq!(clock.h_counter_at(at), Ok(counter));
                assert_eq!(clock, saved);
                after = at;
            }
        }
        assert_eq!(next(&clock, tick(epoch + 3170)), Ok(tick(epoch + 3470)));
        assert_eq!(next(&clock, after), Ok(tick(epoch + 10_310)));
    }
}

pub fn every_master_tick_matches_literal_calendar() {
    every_master_tick_matches_literal_calendar_with(next, qualifies);
}

pub fn every_master_tick_matches_literal_calendar_with(
    next: impl Fn(&VdpH32Timing, MasterTicks) -> Result<MasterTicks, VdpTimingError>,
    qualifies: impl Fn(&VdpH32Timing, MasterTicks) -> Result<bool, VdpTimingError>,
) {
    for epoch in [0, 3] {
        let clock = VdpH32Timing::at_counter_zero(tick(epoch));
        let saved = clock;
        for offset in 0..6840 {
            let at = tick(epoch + offset);
            assert_eq!(next(&clock, at), Ok(tick(epoch + oracle_next(offset))));
            assert_eq!(qualifies(&clock, at), Ok(GRANTS.contains(&(offset % 3420))));
            assert_eq!(clock, saved);
        }
        let mut whole = [0; 32];
        let mut after = tick(epoch);
        for (index, at) in whole.iter_mut().enumerate() {
            after = next(&clock, after).unwrap();
            *at = after.get();
            assert_eq!(*at, epoch + (index / 16) as u64 * 3420 + GRANTS[index % 16]);
        }
        let mut partitioned = [0; 32];
        let mut count = 0;
        for bounds in [0, 50, 51, 530, 2590, 2960, 3170, 3420, 3470, 6840].windows(2) {
            let mut after = tick(epoch + bounds[0]);
            loop {
                let at = next(&clock, after).unwrap();
                if at.get() > epoch + bounds[1] {
                    break;
                }
                assert_eq!(qualifies(&clock, at), Ok(true));
                partitioned[count] = at.get();
                count += 1;
                after = at;
            }
        }
        assert_eq!(count, 32);
        assert_eq!(partitioned, whole);
        assert_eq!(clock, saved);
    }
}

pub fn strict_edges_and_busy_slots() {
    strict_edges_and_busy_slots_with(next, qualifies);
}

pub fn strict_edges_and_busy_slots_with(
    next: impl Fn(&VdpH32Timing, MasterTicks) -> Result<MasterTicks, VdpTimingError>,
    qualifies: impl Fn(&VdpH32Timing, MasterTicks) -> Result<bool, VdpTimingError>,
) {
    let epoch = 3;
    let clock = VdpH32Timing::at_counter_zero(tick(epoch));
    for offset in GRANTS {
        for delta in [-1i64, 0, 1] {
            let requested = offset.checked_add_signed(delta).unwrap();
            assert_eq!(
                next(&clock, tick(epoch + requested)),
                Ok(tick(epoch + oracle_next(requested)))
            );
            assert_eq!(qualifies(&clock, tick(epoch + requested)), Ok(delta == 0));
        }
    }
    for offset in [
        0, 10, 30, 530, 1170, 1810, 2450, 2950, 2960, 2970, 3310, 3410, 3420,
    ] {
        assert_eq!(qualifies(&clock, tick(epoch + offset)), Ok(false));
        assert_eq!(
            next(&clock, tick(epoch + offset)),
            Ok(tick(epoch + oracle_next(offset)))
        );
    }
    for (after, expected) in [(2589, 2590), (2590, 2610), (2609, 2610), (2610, 2890)] {
        assert_eq!(
            next(&clock, tick(epoch + after)),
            Ok(tick(epoch + expected))
        );
    }
}

pub fn epoch_and_overflow_are_pure() {
    epoch_and_overflow_are_pure_with(next, qualifies);
}

pub fn epoch_and_overflow_are_pure_with(
    next: impl Fn(&VdpH32Timing, MasterTicks) -> Result<MasterTicks, VdpTimingError>,
    qualifies: impl Fn(&VdpH32Timing, MasterTicks) -> Result<bool, VdpTimingError>,
) {
    let clock = VdpH32Timing::at_counter_zero(tick(17));
    let saved = clock;
    for at in [0, 16] {
        let error = VdpTimingError::BeforeEpoch {
            epoch: tick(17),
            requested: tick(at),
        };
        assert_eq!(next(&clock, tick(at)), Err(error));
        assert_eq!(qualifies(&clock, tick(at)), Err(error));
        assert_eq!(clock, saved);
    }
    for (remaining, requests) in [
        (50, [0, 49, 50]),
        (49, [0, 48, 49]),
        (3170, [3169, 3170, 3170]),
        (3470, [3170, 3469, 3470]),
        (0, [0, 0, 0]),
    ] {
        let epoch = u64::MAX - remaining;
        let clock = VdpH32Timing::at_counter_zero(tick(epoch));
        let saved = clock;
        for offset in requests {
            let expected = epoch
                .checked_add(oracle_next(offset))
                .map(tick)
                .ok_or(VdpTimingError::ClockOverflow);
            assert_eq!(next(&clock, tick(epoch + offset)), expected);
            assert_eq!(
                qualifies(&clock, tick(epoch + offset)),
                Ok(GRANTS.contains(&(offset % 3420)))
            );
            assert_eq!(clock, saved);
        }
    }
}

pub fn already_eligible_fifo_preserves_byte_phase() {
    already_eligible_fifo_preserves_byte_phase_with(next, qualifies);
}

pub fn already_eligible_fifo_preserves_byte_phase_with(
    next: impl Fn(&VdpH32Timing, MasterTicks) -> Result<MasterTicks, VdpTimingError>,
    qualifies: impl Fn(&VdpH32Timing, MasterTicks) -> Result<bool, VdpTimingError>,
) {
    // Captured FIFO heads are already eligible; no CPU arrival or frame transition is modeled.
    let epoch = 3;
    let clock = VdpH32Timing::at_counter_zero(tick(epoch));
    let mut fifo = VdpWriteFifo::default();
    let mut memory = VdpMemory::default();
    let command = |address, code| ControlCommand { address, code };
    let entry = |address, code, data| VdpWrite {
        command: command(address, code),
        data,
    };
    memory.write_word(command(0, 1), 0xaabb).unwrap();
    let writes = [
        entry(0, 0x11, 0x1234),
        entry(2, 3, 0xfabc),
        entry(4, 5, 0xfd67),
        entry(3, 1, 0x5678),
    ];
    for write in writes {
        fifo.push(write).unwrap();
    }
    let full = fifo.clone();
    assert_eq!(fifo.push(entry(8, 1, 0)), Err(VdpWriteError::Full));
    assert_eq!(fifo, full);
    let mut after = 2889;
    for (offset, completed, count, word) in [
        (2890, None, 4, 0xaa34),
        (3170, Some(writes[0]), 3, 0x1234),
        (3470, Some(writes[1]), 2, 0x1234),
        (3630, Some(writes[2]), 1, 0x1234),
        (3790, None, 1, 0x1234),
        (4110, Some(writes[3]), 0, 0x1234),
    ] {
        let saved_fifo = fifo.clone();
        let saved_memory = memory.clone();
        for busy in [2891, 2950, 2960, 2970, 3310, 3410, 3420, 3530, 3950] {
            if busy > after && busy < offset {
                assert_eq!(qualifies(&clock, tick(epoch + busy)), Ok(false));
                assert_eq!(next(&clock, tick(epoch + busy)), Ok(tick(epoch + offset)));
                assert_eq!(fifo, saved_fifo);
                assert_eq!(memory, saved_memory);
            }
        }
        assert_eq!(next(&clock, tick(epoch + after)), Ok(tick(epoch + offset)));
        assert_eq!(qualifies(&clock, tick(epoch + offset)), Ok(true));
        assert_eq!(fifo, saved_fifo);
        assert_eq!(memory, saved_memory);
        assert_eq!(fifo.service_write_slot(&mut memory), Ok(completed));
        assert_eq!(fifo.len(), count);
        assert_eq!(memory.read_word(command(0, 0)), Ok(word));
        after = offset;
    }
    assert_eq!(memory.cram()[1], 0x0aac);
    assert_eq!(memory.vsram()[2], 0x0567);
    assert_eq!(memory.read_word(command(2, 0)), Ok(0x7856));
    assert_eq!(fifo.status_bits(), 0x0200);
}
