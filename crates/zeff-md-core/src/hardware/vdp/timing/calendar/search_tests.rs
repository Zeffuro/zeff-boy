use super::*;
use crate::hardware::vdp::mode::HorizontalMode;

fn linear(epoch: u64, after: u64, active: bool) -> Result<MasterTicks, VdpTimingError> {
    let elapsed = after
        .checked_sub(epoch)
        .ok_or(VdpTimingError::BeforeEpoch {
            epoch: MasterTicks::new(epoch),
            requested: MasterTicks::new(after),
        })?;
    let phase = elapsed % H32_LINE_TICKS;
    let slots = HorizontalMode::h40_slots(active);
    let step = slots
        .iter()
        .copied()
        .find(|&offset| u64::from(offset) > phase)
        .map_or(H32_LINE_TICKS - phase + u64::from(slots[0]), |offset| {
            u64::from(offset) - phase
        });
    after
        .checked_add(step)
        .map(MasterTicks::new)
        .ok_or(VdpTimingError::ClockOverflow)
}

#[test]
fn every_h40_phase_preserves_strict_successor_wrap_epoch_and_overflow() {
    for active in [false, true] {
        let slots = HorizontalMode::h40_slots(active);
        assert!(!slots.is_empty());
        assert!(slots.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(slots.iter().all(|&tick| u64::from(tick) < H32_LINE_TICKS));
        for epoch in [0, 1, 3419, u64::MAX - 4 * H32_LINE_TICKS] {
            let timing = VdpVerticalTiming::at_counters_zero(MasterTicks::new(epoch));
            let before = timing;
            for elapsed in 0..3 * H32_LINE_TICKS {
                let after = epoch + elapsed;
                let result = timing.next_slot(MasterTicks::new(after), active, HorizontalMode::H40);
                assert_eq!(result, linear(epoch, after, active));
                assert!(result.unwrap().get() > after);
                assert_eq!(timing, before);
            }
            if epoch != 0 {
                assert_eq!(
                    timing.next_slot(MasterTicks::new(epoch - 1), active, HorizontalMode::H40),
                    linear(epoch, epoch - 1, active)
                );
            }
        }
        let epoch = u64::MAX - 3 * H32_LINE_TICKS;
        let timing = VdpVerticalTiming::at_counters_zero(MasterTicks::new(epoch));
        for remaining in 0..2 * H32_LINE_TICKS {
            let after = u64::MAX - remaining;
            assert_eq!(
                timing.next_slot(MasterTicks::new(after), active, HorizontalMode::H40),
                linear(epoch, after, active)
            );
        }
    }
}
