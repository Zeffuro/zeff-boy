use super::*;

pub(super) fn run() -> u64 {
    assert_eq!(LineClock::H32.scroll_fetch_phase(), 3190);
    assert_eq!(LineClock::ExternalH40.scroll_fetch_phase(), 3197);
    assert_eq!(LineClock::InternalH40.scroll_fetch_phase(), 3152);
    assert_eq!(LineClock::InternalH40.ticks(), 3360);
    assert_eq!(LineClock::ExternalH40.ticks(), 3420);
    assert_eq!(LineClock::H32.ticks(), 3420);
    for (value, clock) in [
        (0, LineClock::H32),
        (2, LineClock::H32),
        (6, LineClock::H32),
        (8, LineClock::H32),
        (1, LineClock::InternalH40),
        (9, LineClock::InternalH40),
        (0x81, LineClock::ExternalH40),
        (0x83, LineClock::ExternalH40),
        (0x87, LineClock::ExternalH40),
    ] {
        assert_eq!(LineClock::from_register(value).unwrap(), clock);
    }
    for value in [0x80, 0x82, 3, 7, 4, 0x85] {
        assert_eq!(
            LineClock::from_register(value),
            Err(VdpTimingError::UnsupportedMode { index: 12, value })
        );
    }
    let internal = LineClock::InternalH40;
    assert_eq!(internal.mode().width(), 320);
    assert_eq!(LineClock::H32.mode().width(), 256);
    for (phase, counter) in [
        (0, 0),
        (7, 0),
        (8, 1),
        (2912, 0x16c),
        (2919, 0x16c),
        (2920, 0x1c9),
        (3344, 0x1fe),
        (3359, 0x1ff),
    ] {
        assert_eq!(internal.counter(phase).unwrap(), counter);
    }
    assert!(internal.counter(3360).is_err());
    assert!(internal.blank(3360).is_err());
    assert!(internal.next_slot(3360, false).is_err());
    for (phase, blank) in [
        (0, true),
        (91, true),
        (92, false),
        (2867, false),
        (2868, true),
        (3359, true),
    ] {
        assert_eq!(internal.blank(phase).unwrap(), blank);
    }
    assert_eq!(internal.vertical_clock(), 2640);
    assert_eq!(internal.vertical_interrupt(), 8);
    assert_eq!(LineClock::H32.vertical_clock(), 2660);
    assert_eq!(LineClock::H32.vertical_interrupt(), 10);
    let active = [
        16, 144, 272, 528, 656, 784, 1040, 1168, 1296, 1552, 1680, 1808, 2064, 2192, 2320, 2560,
        2576, 2960,
    ];
    let refresh = [400, 912, 1424, 1936, 2448, 3248];
    let idle: Vec<u64> = (0..3360)
        .step_by(16)
        .filter(|tick| !refresh.contains(tick))
        .collect();
    assert_eq!(idle.len(), 204);
    let mut digest = 0_u64;
    for phase in 0..3360 {
        for (enabled, expected) in [(true, &active[..]), (false, &idle[..])] {
            assert_eq!(
                internal.is_slot(phase, enabled).unwrap(),
                expected.contains(&phase)
            );
            assert_eq!(
                internal.next_slot(phase, enabled).unwrap(),
                expected.iter().copied().find(|&tick| tick > phase)
            );
        }
        let dot = phase / 8;
        let expected = if dot < 365 { dot } else { dot + 92 };
        let actual = internal.counter(phase).unwrap();
        assert_eq!(u64::from(actual), expected);
        digest = digest.rotate_left(3) ^ u64::from(actual) ^ phase;
    }
    assert!(internal.is_slot(0, false).unwrap());
    assert!(!internal.is_slot(2962, true).unwrap());
    let legacy = VdpH32Timing::at_counter_zero(MasterTicks::ZERO);
    for phase in 0..3420 {
        for clock in [LineClock::H32, LineClock::ExternalH40] {
            assert_eq!(clock.counter(phase).unwrap(), clock.mode().counter(phase));
            assert_eq!(clock.blank(phase).unwrap(), clock.mode().blank(phase));
            for active in [false, true] {
                let expected = if clock == LineClock::H32 {
                    let counter = legacy.h_counter_at(MasterTicks::new(phase)).unwrap();
                    if active {
                        legacy
                            .is_active_write_slot(MasterTicks::new(phase))
                            .unwrap()
                    } else {
                        phase % 20 == 10 && !H32_NONACTIVE_REFRESH.contains(&counter)
                    }
                } else {
                    HorizontalMode::h40_slots(active).contains(&(phase as u16))
                };
                assert_eq!(clock.is_slot(phase, active).unwrap(), expected);
                let next = (phase + 1..3420).find(|&at| clock.is_slot(at, active).unwrap());
                assert_eq!(clock.next_slot(phase, active).unwrap(), next);
            }
        }
    }
    digest
}

#[test]
fn internal_h40_literal_grid_and_unchanged_legacy_clocks() {
    assert_ne!(run(), 0);
}
