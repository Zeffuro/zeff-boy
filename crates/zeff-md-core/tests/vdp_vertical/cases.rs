use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    Region, VdpH32Timing, VdpRegisterPipeline, VdpTimingError, VdpVBlankEdge, VdpVerticalTiming,
};

pub type Observation = (
    Result<u16, VdpTimingError>,
    Result<bool, VdpTimingError>,
    Result<VdpVBlankEdge, VdpTimingError>,
);

fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}

fn publish(registers: &mut VdpRegisterPipeline, index: u8, value: u8) {
    registers
        .advance_to(tick(registers.now().get() + 1))
        .unwrap();
    let write = registers.queue_register(index, value).unwrap();
    assert_eq!(registers.advance_to(write.publish_at), Ok(Some(write)));
}

fn known(r1: u8, r12: u8) -> VdpRegisterPipeline {
    let mut registers = VdpRegisterPipeline::at_h32_edge(tick(0));
    publish(&mut registers, 1, r1);
    publish(&mut registers, 12, r12);
    registers
}

pub fn observe(
    clock: &VdpVerticalTiming,
    at: MasterTicks,
    region: Region,
    registers: &VdpRegisterPipeline,
) -> Observation {
    (
        clock.v_counter_at(at, region, registers),
        clock.vblank_flag_at(at, region, registers),
        clock.next_vblank_edge(at, region, registers),
    )
}

fn counter(index: usize) -> u16 {
    (0..=0xea).chain(0x1e5..=0x1ff).nth(index).unwrap()
}

fn expected(epoch: u64, elapsed: u64, value: u16) -> Observation {
    let events = [
        (765320, true),
        (891860, false),
        (1661360, true),
        (1787900, false),
        (2557400, true),
    ];
    let &(offset, active) = events.iter().find(|(offset, _)| *offset > elapsed).unwrap();
    let flag = matches!(elapsed, 765320..=891859 | 1661360..=1787899);
    (
        Ok(value),
        Ok(flag),
        Ok(VdpVBlankEdge {
            at: tick(epoch + offset),
            active,
        }),
    )
}

pub fn counter_sequence_with(
    mut query: impl FnMut(&VdpVerticalTiming, MasterTicks, Region, &VdpRegisterPipeline) -> Observation,
) {
    let registers = known(0x44, 0);
    let saved_registers = registers;
    for epoch in [40, 47, 10000] {
        let clock = VdpVerticalTiming::at_counters_zero(tick(epoch));
        let saved = clock;
        for region in [Region::NtscUs, Region::NtscJapan] {
            for ordinal in 0..524 {
                let edge = 2660 + ordinal * 3420;
                for delta in [-1, 0, 1, 759, 760, 3419] {
                    let elapsed = (edge as i64 + delta) as u64;
                    let index = ((ordinal + u64::from(delta >= 0)) % 262) as usize;
                    assert_eq!(
                        query(&clock, tick(epoch + elapsed), region, &registers),
                        expected(epoch, elapsed, counter(index))
                    );
                    assert_eq!(clock, saved);
                    assert_eq!(registers, saved_registers);
                }
            }
        }
    }
}

pub fn counter_sequence() {
    counter_sequence_with(observe);
}

pub fn literal_edges_and_copies() {
    let epoch = 137;
    let clock = VdpVerticalTiming::at_counters_zero(tick(epoch));
    let copy = clock;
    let horizontal = VdpH32Timing::at_counter_zero(tick(epoch));
    let registers = known(0x44, 0);
    for (elapsed, value, blank) in [
        (0, 0, false),
        (2659, 0, false),
        (2660, 1, false),
        (3419, 1, false),
        (3420, 1, false),
        (765319, 0xdf, false),
        (765320, 0xe0, true),
        (802939, 0xea, true),
        (802940, 0x1e5, true),
        (891859, 0x1fe, true),
        (891860, 0x1ff, false),
        (895279, 0x1ff, false),
        (895280, 0, false),
        (896040, 0, false),
    ] {
        let at = tick(epoch + elapsed);
        assert_eq!(
            observe(&clock, at, Region::NtscUs, &registers),
            expected(epoch, elapsed, value)
        );
        assert_eq!(
            copy.vblank_flag_at(at, Region::NtscUs, &registers),
            Ok(blank)
        );
    }
    for elapsed in [2660, 765320, 802940, 891860, 895280] {
        assert_eq!(
            horizontal.h_counter_at(tick(epoch + elapsed - 1)),
            Ok(0x109)
        );
        assert_eq!(horizontal.h_counter_at(tick(epoch + elapsed)), Ok(0x10a));
        assert_eq!(horizontal.hblank_flag_at(tick(epoch + elapsed)), Ok(false));
    }
    let mut after = tick(epoch);
    for (offset, active) in [
        (765320, true),
        (891860, false),
        (1661360, true),
        (1787900, false),
    ] {
        let edge = VdpVBlankEdge {
            at: tick(epoch + offset),
            active,
        };
        assert_eq!(
            clock.next_vblank_edge(after, Region::NtscUs, &registers),
            Ok(edge)
        );
        assert_eq!(
            copy.next_vblank_edge(after, Region::NtscUs, &registers),
            Ok(edge)
        );
        for (delta, flag) in [(-1, !active), (0, active), (1, active)] {
            assert_eq!(
                clock.vblank_flag_at(
                    tick((edge.at.get() as i64 + delta) as u64),
                    Region::NtscUs,
                    &registers
                ),
                Ok(flag)
            );
        }
        after = edge.at;
    }
    assert_eq!(
        clock.v_counter_at(tick(epoch), Region::NtscUs, &registers),
        Ok(0)
    );
}

pub fn critical_lines_with(
    mut query: impl FnMut(&VdpVerticalTiming, MasterTicks, Region, &VdpRegisterPipeline) -> Observation,
) {
    let epoch = 1000;
    let clock = VdpVerticalTiming::at_counters_zero(tick(epoch));
    let registers = known(0x44, 0);
    for region in [Region::NtscUs, Region::NtscJapan] {
        for (line, before, after) in [
            (0, 0, 1),
            (223, 0xdf, 0xe0),
            (234, 0xea, 0x1e5),
            (260, 0x1fe, 0x1ff),
            (261, 0x1ff, 0),
        ] {
            for phase in 0..3420 {
                let elapsed = line * 3420 + phase;
                let value = if phase < 2660 { before } else { after };
                assert_eq!(
                    query(&clock, tick(epoch + elapsed), region, &registers),
                    expected(epoch, elapsed, value)
                );
            }
        }
    }
}

pub fn critical_lines() {
    critical_lines_with(observe);
}

pub fn epoch_and_overflow_guards_with(
    mut query: impl FnMut(&VdpVerticalTiming, MasterTicks, Region, &VdpRegisterPipeline) -> Observation,
) {
    let registers = known(0x44, 0);
    let clock = VdpVerticalTiming::at_counters_zero(tick(137));
    for at in [0, 136] {
        let error = VdpTimingError::BeforeEpoch {
            epoch: tick(137),
            requested: tick(at),
        };
        assert_eq!(
            query(&clock, tick(at), Region::NtscUs, &registers),
            (Err(error), Err(error), Err(error))
        );
    }
    for (distance, active) in [
        (765320, true),
        (891860, false),
        (1661360, true),
        (1787900, false),
    ] {
        let epoch = u64::MAX - distance;
        let clock = VdpVerticalTiming::at_counters_zero(tick(epoch));
        for elapsed in [distance - 1, distance] {
            let ordinal = ((u128::from(elapsed) + 760) / 3420 % 262) as usize;
            let next = if elapsed == distance {
                Err(VdpTimingError::ClockOverflow)
            } else {
                Ok(VdpVBlankEdge {
                    at: tick(u64::MAX),
                    active,
                })
            };
            assert_eq!(
                query(&clock, tick(epoch + elapsed), Region::NtscUs, &registers),
                (
                    Ok(counter(ordinal)),
                    Ok(if elapsed == distance { active } else { !active }),
                    next
                )
            );
        }
    }
    for epoch in [0, 17, u64::MAX - 1, u64::MAX] {
        let clock = VdpVerticalTiming::at_counters_zero(tick(epoch));
        let elapsed = u64::MAX - epoch;
        let ordinal = ((u128::from(elapsed) + 760) / 3420 % 262) as usize;
        let value = counter(ordinal);
        assert_eq!(
            query(&clock, tick(u64::MAX), Region::NtscUs, &registers),
            (
                Ok(value),
                Ok((0xe0..=0x1fe).contains(&value)),
                Err(VdpTimingError::ClockOverflow)
            )
        );
    }
}

pub fn epoch_and_overflow_guards() {
    epoch_and_overflow_guards_with(observe);
}

pub fn selector_partition_with(
    mut query: impl FnMut(&VdpVerticalTiming, MasterTicks, Region, &VdpRegisterPipeline) -> Observation,
) {
    let clock = VdpVerticalTiming::at_counters_zero(tick(1000));
    let at = tick(1000 + 765320);
    let valid = expected(1000, 765320, 0xe0);
    for (index, mask, wanted) in [(1, 0x4c, 0x44), (12, 0x06, 0)] {
        let mut accepted = 0;
        let mut rejected = 0;
        for value in 0..=255 {
            if (index == 1 && value & 4 == 0) || (index == 12 && value & 0x81 != 0) {
                continue;
            }
            let registers = if index == 1 {
                known(value, 0)
            } else {
                known(0x44, value)
            };
            let error = VdpTimingError::UnsupportedMode { index, value };
            let result = if value & mask == wanted {
                accepted += 1;
                valid
            } else if index == 12 && value & mask == 6 {
                accepted += 1;
                (
                    Ok(0xe0),
                    Ok(true),
                    Ok(VdpVBlankEdge {
                        at: tick(1000 + 895280),
                        active: false,
                    }),
                )
            } else {
                rejected += 1;
                (Err(error), Err(error), Err(error))
            };
            for region in [Region::NtscUs, Region::NtscJapan] {
                assert_eq!(query(&clock, at, region, &registers), result);
            }
        }
        assert_eq!(
            (accepted, rejected),
            if index == 1 { (32, 96) } else { (32, 32) }
        );
    }
    let registers = known(0x44, 0);
    assert_eq!(
        query(&clock, at, Region::Pal, &registers),
        (
            Ok(0xe0),
            Ok(true),
            Ok(VdpVBlankEdge {
                at: tick(1000 + 1066280),
                active: false,
            })
        )
    );
    let mut registers = VdpRegisterPipeline::at_h32_edge(tick(0));
    for index in [1, 12] {
        let error = VdpTimingError::UnknownRegister(index);
        assert_eq!(
            query(&clock, at, Region::NtscUs, &registers),
            (Err(error), Err(error), Err(error))
        );
        publish(&mut registers, index, if index == 1 { 0x44 } else { 0 });
    }
}

pub fn selector_partition() {
    selector_partition_with(observe);
}

pub fn pending_publication_with(
    mut query: impl FnMut(&VdpVerticalTiming, MasterTicks, Region, &VdpRegisterPipeline) -> Observation,
) {
    let clock = VdpVerticalTiming::at_counters_zero(tick(1000));
    let at = tick(1000 + 765320);
    for (index, value) in [(1, 0x4c), (12, 2), (7, 0x31)] {
        let mut registers = known(0x44, 0);
        registers.advance_to(tick(41)).unwrap();
        let write = registers.queue_register(index, value).unwrap();
        let saved = registers;
        let result = query(&clock, at, Region::NtscUs, &registers);
        let error = VdpTimingError::PendingModeChange;
        assert_eq!(
            result,
            if index == 7 {
                expected(1000, 765320, 0xe0)
            } else {
                (Err(error), Err(error), Err(error))
            }
        );
        assert_eq!(registers, saved);
        registers.advance_to(write.publish_at).unwrap();
        let error = VdpTimingError::UnsupportedMode { index, value };
        assert_eq!(
            query(&clock, at, Region::NtscUs, &registers),
            if index == 7 {
                expected(1000, 765320, 0xe0)
            } else {
                (Err(error), Err(error), Err(error))
            }
        );
        if index != 7 {
            publish(&mut registers, index, if index == 1 { 0x44 } else { 0 });
            assert_eq!(
                query(&clock, at, Region::NtscUs, &registers),
                expected(1000, 765320, 0xe0)
            );
        }
    }
}

pub fn pending_publication() {
    pending_publication_with(observe);
}
