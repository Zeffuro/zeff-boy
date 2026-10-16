use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, Region, VdpMemory, VdpRegisterError, VdpRegisterPipeline, VdpTimingError,
    VdpVerticalTiming, VdpWrite, VdpWriteFifo,
};

pub type Observation = (
    Result<MasterTicks, VdpTimingError>,
    Result<bool, VdpTimingError>,
);
const ACTIVE: [u64; 16] = [
    50, 210, 370, 690, 850, 1010, 1330, 1490, 1650, 1970, 2130, 2290, 2590, 2610, 2890, 3170,
];
const REFRESH: [u64; 5] = [530, 1170, 1810, 2450, 3310];

fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}

fn publish(registers: &mut VdpRegisterPipeline, index: u8, value: u8) {
    registers
        .advance_to(tick(registers.now().get() + 1))
        .unwrap();
    let write = registers.queue_register(index, value).unwrap();
    registers.advance_to(write.publish_at).unwrap();
}

fn known(mode: u8) -> VdpRegisterPipeline {
    let mut registers = VdpRegisterPipeline::at_h32_edge(MasterTicks::ZERO);
    publish(&mut registers, 1, mode);
    publish(&mut registers, 12, 0);
    registers
}

pub fn observe(
    clock: &VdpVerticalTiming,
    at: MasterTicks,
    region: Region,
    registers: &VdpRegisterPipeline,
) -> Observation {
    (
        clock.next_memory_slot(at, region, registers),
        clock.is_memory_slot(at, region, registers),
    )
}

fn oracle(offset: u64, enabled: bool) -> Observation {
    let field = offset / 896040 * 896040;
    let phase = offset % 896040;
    let (start, end, active) = match phase {
        _ if !enabled => (0, u64::MAX, false),
        0..759240 => (0, 759240, true),
        769500..885780 => (769500, 885780, false),
        _ => {
            let error = VdpTimingError::UnqualifiedMemoryInterval(tick(offset));
            return (Err(error), Err(error));
        }
    };
    let eligible = |point: u64| {
        let dot = point % 3420;
        if active {
            ACTIVE.contains(&dot)
        } else {
            dot % 20 == 10 && !REFRESH.contains(&dot)
        }
    };
    assert!(phase >= start);
    let next = (offset + 1..=offset + 3420)
        .find(|&point| eligible(point))
        .unwrap();
    let next = if enabled && next >= field + end {
        Err(VdpTimingError::UnqualifiedMemoryInterval(tick(next)))
    } else {
        Ok(tick(next))
    };
    (next, Ok(eligible(offset)))
}

fn translated(observation: Observation, epoch: u64) -> Observation {
    let translate_error = |error| match error {
        VdpTimingError::UnqualifiedMemoryInterval(at) => {
            VdpTimingError::UnqualifiedMemoryInterval(tick(epoch + at.get()))
        }
        _ => error,
    };
    (
        observation
            .0
            .map(|at| tick(epoch + at.get()))
            .map_err(translate_error),
        observation.1.map_err(translate_error),
    )
}

pub fn field_slots_and_counts() {
    field_slots_and_counts_with(observe);
}

pub fn field_slots_and_counts_with(
    mut query: impl FnMut(&VdpVerticalTiming, MasterTicks, Region, &VdpRegisterPipeline) -> Observation,
) {
    let registers = known(0x44);
    let clock = VdpVerticalTiming::at_counters_zero(tick(3));
    let saved = clock;
    let mut counts = [0; 3];
    for cycle in 0..262 {
        for slot in 0..171 {
            let point = cycle * 3420 + slot * 20 + 10;
            let expected = oracle(point, true);
            match expected.1 {
                Ok(true) if cycle < 222 => counts[0] += 1,
                Ok(true) => counts[1] += 1,
                Err(_) => counts[2] += 1,
                _ => {}
            }
            for offset in [point - 1, point, point + 1] {
                assert_eq!(
                    query(&clock, tick(3 + offset), Region::NtscUs, &registers),
                    translated(oracle(offset, true), 3)
                );
                assert_eq!(clock, saved);
            }
        }
    }
    assert_eq!(counts, [3552, 5644, 1026]);
}

pub fn interval_endpoints() {
    interval_endpoints_with(observe);
}

pub fn every_field_phase() {
    every_field_phase_with(observe);
}

pub fn every_field_phase_with(
    mut query: impl FnMut(&VdpVerticalTiming, MasterTicks, Region, &VdpRegisterPipeline) -> Observation,
) {
    let registers = known(0x44);
    let clock = VdpVerticalTiming::at_counters_zero(tick(0));
    for offset in 0..896040 {
        assert_eq!(
            query(&clock, tick(offset), Region::NtscUs, &registers),
            oracle(offset, true)
        );
    }
}

pub fn interval_endpoints_with(
    mut query: impl FnMut(&VdpVerticalTiming, MasterTicks, Region, &VdpRegisterPipeline) -> Observation,
) {
    for epoch in [0, 47] {
        let clock = VdpVerticalTiming::at_counters_zero(tick(epoch));
        for region in [Region::NtscUs, Region::NtscJapan] {
            for mode in [0x04, 0x44] {
                let registers = known(mode);
                let saved = registers;
                for field in [0, 896040] {
                    for point in [
                        0, 49, 50, 51, 2660, 758989, 758990, 758991, 759239, 759240, 759241,
                        765319, 765320, 769499, 769500, 769509, 769510, 885769, 885770, 885771,
                        885779, 885780, 891859, 891860, 896039,
                    ] {
                        let offset = field + point;
                        assert_eq!(
                            query(&clock, tick(epoch + offset), region, &registers),
                            translated(oracle(offset, mode & 0x40 != 0), epoch)
                        );
                        assert_eq!(registers, saved);
                    }
                }
            }
        }
    }
}

pub fn partitioned_intervals() {
    partitioned_intervals_with(observe);
}

pub fn partitioned_intervals_with(
    mut query: impl FnMut(&VdpVerticalTiming, MasterTicks, Region, &VdpRegisterPipeline) -> Observation,
) {
    let registers = known(0x44);
    let clock = VdpVerticalTiming::at_counters_zero(tick(47));
    for (start, end, count) in [(0, 758990, 3552), (769500, 885770, 5644)] {
        let mut enumerate = |parts: &[u64]| {
            let mut values = [0; 5644];
            let mut count = 0;
            let mut after = start;
            for &part in parts {
                loop {
                    let next = query(&clock, tick(47 + after), Region::NtscUs, &registers)
                        .0
                        .unwrap()
                        .get()
                        - 47;
                    if next > part {
                        break;
                    }
                    values[count] = next;
                    count += 1;
                    after = next;
                    if after == end {
                        break;
                    }
                }
                after = part;
            }
            (values, count)
        };
        let whole = enumerate(&[end]);
        let split = enumerate(&[
            start + 49,
            start + 50,
            start + 2660,
            start + 3420,
            end - 1,
            end,
        ]);
        assert_eq!(whole, split);
        assert_eq!(whole.1, count);
    }
}

pub fn mode_and_tail_guards() {
    mode_and_tail_guards_with(observe);
}

pub fn mode_and_tail_guards_with(
    mut query: impl FnMut(&VdpVerticalTiming, MasterTicks, Region, &VdpRegisterPipeline) -> Observation,
) {
    let clock = VdpVerticalTiming::at_counters_zero(tick(47));
    let registers = known(0x44);
    let error = VdpTimingError::BeforeEpoch {
        epoch: tick(47),
        requested: tick(46),
    };
    assert_eq!(
        query(&clock, tick(46), Region::Pal, &registers),
        (Err(error), Err(error))
    );
    assert_eq!(
        query(&clock, tick(47), Region::Pal, &registers),
        (Ok(tick(97)), Ok(false))
    );
    let unknown = VdpRegisterPipeline::at_h32_edge(tick(0));
    let error = VdpTimingError::UnknownRegister(1);
    assert_eq!(
        query(&clock, tick(47), Region::NtscUs, &unknown),
        (Err(error), Err(error))
    );
    for (index, value) in [(1, 0x40), (12, 0x01), (12, 0x80)] {
        let mut invalid = registers;
        invalid.advance_to(tick(invalid.now().get() + 1)).unwrap();
        let saved = invalid;
        assert_eq!(
            invalid.queue_register(index, value),
            Err(VdpRegisterError::UnsupportedModeChange { index, value })
        );
        assert_eq!(invalid, saved);
    }
    for (index, value) in [(1, 0x4c), (1, 0xc4), (12, 0x02), (12, 0x04)] {
        let mut invalid = registers;
        publish(&mut invalid, index, value);
        let error = VdpTimingError::UnsupportedMode { index, value };
        assert_eq!(
            query(&clock, tick(47), Region::NtscUs, &invalid),
            (Err(error), Err(error))
        );
    }
    for index in [1, 12] {
        let mut pending = registers;
        pending.advance_to(tick(pending.now().get() + 1)).unwrap();
        pending
            .queue_register(index, if index == 1 { 0x04 } else { 0x10 })
            .unwrap();
        let saved = pending;
        let error = VdpTimingError::PendingModeChange;
        assert_eq!(
            query(&clock, tick(47), Region::NtscUs, &pending),
            (Err(error), Err(error))
        );
        assert_eq!(pending, saved);
    }
    let mut pending = registers;
    pending.advance_to(tick(pending.now().get() + 1)).unwrap();
    pending.queue_register(15, 2).unwrap();
    let saved = pending;
    assert_eq!(
        query(&clock, tick(47), Region::NtscUs, &pending),
        translated(oracle(0, true), 47)
    );
    assert_eq!(pending, saved);
    for (distance, mode, qualifies) in [(50, 0x44, true), (49, 0x44, false), (10, 0x04, true)] {
        let clock = VdpVerticalTiming::at_counters_zero(tick(u64::MAX - distance));
        let registers = known(mode);
        let at = tick(u64::MAX);
        assert_eq!(
            query(&clock, at, Region::NtscUs, &registers),
            (Err(VdpTimingError::ClockOverflow), Ok(qualifies))
        );
        let first = query(
            &clock,
            tick(u64::MAX - distance),
            Region::NtscUs,
            &registers,
        )
        .0;
        assert_eq!(
            first,
            if distance == 49 {
                Err(VdpTimingError::ClockOverflow)
            } else {
                Ok(at)
            }
        );
    }
}

pub fn fifo_service_and_uncertainty() {
    fifo_service_and_uncertainty_with(observe);
}

pub fn fifo_service_and_uncertainty_with(
    mut query: impl FnMut(&VdpVerticalTiming, MasterTicks, Region, &VdpRegisterPipeline) -> Observation,
) {
    let registers = known(0x44);
    let clock = VdpVerticalTiming::at_counters_zero(tick(0));
    let mut fifo = VdpWriteFifo::default();
    let mut memory = VdpMemory::default();
    let command = ControlCommand {
        address: 0,
        code: 1,
    };
    memory.write_word(command, 0xaabb).unwrap();
    fifo.push(VdpWrite {
        command,
        data: 0x1234,
    })
    .unwrap();
    assert_eq!(
        query(&clock, tick(758709), Region::NtscUs, &registers).0,
        Ok(tick(758710))
    );
    assert_eq!(fifo.service_write_slot(&mut memory), Ok(None));
    assert_eq!(&memory.vram()[..2], &[0xaa, 0x34]);
    let saved_fifo = fifo.clone();
    let saved_memory = memory.clone();
    for at in [759240, 765320, 769499, 885780, 891860, 896039] {
        assert!(matches!(
            query(&clock, tick(at), Region::NtscUs, &registers),
            (
                Err(VdpTimingError::UnqualifiedMemoryInterval(_)),
                Err(VdpTimingError::UnqualifiedMemoryInterval(_))
            )
        ));
        assert_eq!(fifo, saved_fifo);
        assert_eq!(memory, saved_memory);
    }
    assert_eq!(
        query(&clock, tick(758710), Region::NtscUs, &registers).0,
        Ok(tick(758990))
    );
    assert!(fifo.service_write_slot(&mut memory).unwrap().is_some());
    assert_eq!(&memory.vram()[..2], &[0x12, 0x34]);
    assert!(fifo.is_empty());
}
