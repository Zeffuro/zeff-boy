use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter, write_optional};
use crate::hardware::vdp::{VdpEvent, VdpInterruptError, VdpTimingError};

fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}

fn registers(
    epoch: u64,
    now: u64,
    mode: Option<u8>,
    width: Option<u8>,
    reload: Option<u8>,
    pending: Option<(u8, u8)>,
) -> VdpRegisterPipeline {
    let mut w = StateWriter::new();
    w.write_u64(epoch);
    w.write_u64(now);
    write_optional(&mut w, pending, |w, (index, value)| {
        w.write_u8(index);
        w.write_u8(value);
        w.write_u64(now + H32_INTERNAL_CLOCK_TICKS - (now - epoch) % H32_INTERNAL_CLOCK_TICKS);
    });
    for index in 0..REGISTER_COUNT {
        let value = match index {
            MODE_REGISTER => mode,
            DISPLAY_SIZE_REGISTER => width,
            HINT_RELOAD_REGISTER => reload,
            _ => Some(0),
        };
        write_optional(&mut w, value, StateWriter::write_u8);
    }
    VdpRegisterPipeline::read_state(&mut StateReader::new(&w.into_bytes())).unwrap()
}

fn interrupts(
    epoch: u64,
    start: u64,
    id: u64,
    updated: u64,
    now: u64,
    counter: u8,
) -> VdpInterrupts {
    let mut w = StateWriter::new();
    for value in [epoch, start, id, updated] {
        w.write_u64(value);
    }
    for _ in 0..4 {
        w.write_bool(false);
    }
    w.write_bool(false);
    w.write_u64(now);
    w.write_u8(counter);
    w.write_bool(false);
    w.write_bool(false);
    VdpInterrupts::at_counters_zero(tick(epoch), counter, false, false)
        .write_blank_state(&mut w)
        .unwrap();
    let bytes = w.into_bytes();
    let mut reader = StateReader::new(&bytes);
    let interrupts = VdpInterrupts::read_state(&mut reader, Region::NtscUs).unwrap();
    assert!(reader.is_exhausted());
    interrupts
}

fn state(epoch: u64, now: u64, mode: u8, width: u8, counter: u8) -> State {
    let mut state = State::bootstrap_mode5();
    state.registers = registers(epoch, now, Some(mode), Some(width), Some(3), None);
    state.interrupts = interrupts(epoch, epoch, 0, epoch, now, counter);
    state
}

fn original_boundary(
    interrupts: &VdpInterrupts,
    registers: &VdpRegisterPipeline,
    region: Region,
) -> Result<Option<MasterTicks>, VdpScheduleError> {
    match interrupts
        .field_position(region, &registers.published_state().1)
        .and_then(|position| position.end())
    {
        Ok(end) => Ok(Some(end)),
        Err(VdpTimingError::ClockOverflow) => Ok(None),
        Err(error) => Err(VdpInterruptError::from(error).into()),
    }
}

#[derive(Default, Debug)]
struct Trace {
    pairs: usize,
    interrupts: usize,
    publications: usize,
    skips: usize,
}

fn original_advance(
    state: &mut State,
    at: MasterTicks,
    region: Region,
    trace: &mut Trace,
) -> Result<(), VdpScheduleError> {
    let mut interrupts = state.interrupts;
    let mut registers = state.registers;
    let mut memory_mode_known = state.memory_mode_known;
    let mut hv_latch = state.hv_latch;
    let mut status_known = state.status_known
        && registers.registers()[usize::from(MODE_REGISTER)]
            .is_some_and(|mode| mode & DISPLAY_ENABLE == 0);
    loop {
        if let Some(next) = interrupts.skip_cycles(at, region, &registers)? {
            trace.skips += 1;
            registers.advance_to(next)?;
        }
        let horizon = original_boundary(&interrupts, &registers, region)?
            .map_or(at, |boundary| boundary.min(at));
        loop {
            let previous_mode = registers.registers()[usize::from(MODE_REGISTER)];
            let previous_latch = registers.registers()[usize::from(HINT_ENABLE_REGISTER)]
                .is_some_and(|value| value & HV_COUNTER_LATCH_MASK != 0);
            trace.pairs += 1;
            let event = interrupts.advance_pair(horizon, region, &mut registers, false)?;
            match event {
                Some(VdpEvent::Interrupt(_)) => trace.interrupts += 1,
                Some(VdpEvent::Register(_)) => trace.publications += 1,
                None => {}
            }
            memory_mode_known &= previous_mode.map(|mode| mode & MEMORY_MODE_MASK)
                == registers.registers()[usize::from(MODE_REGISTER)]
                    .map(|mode| mode & MEMORY_MODE_MASK);
            let latched = registers.registers()[usize::from(HINT_ENABLE_REGISTER)]
                .is_some_and(|value| value & HV_COUNTER_LATCH_MASK != 0);
            if !latched {
                hv_latch = None;
            } else if !previous_latch {
                hv_latch = Some(interrupts.hv_counter(region, &registers)?);
            }
            status_known &= registers.registers()[usize::from(MODE_REGISTER)]
                .is_some_and(|mode| mode & DISPLAY_ENABLE == 0);
            if event.is_none() {
                break;
            }
        }
        if horizon == at {
            state.interrupts = interrupts;
            state.registers = registers;
            state.memory_mode_known = memory_mode_known;
            state.hv_latch = hv_latch;
            state.status_known = status_known;
            return Ok(());
        }
    }
}

fn compare(
    actual: &mut State,
    expected: &mut State,
    at: u64,
    region: Region,
) -> (Result<(), VdpScheduleError>, Trace) {
    assert_eq!(actual, expected);
    let before = actual.clone();
    assert_eq!(
        actual
            .interrupts
            .cycle_boundary(region, &actual.registers)
            .map(|pair| pair.0),
        original_boundary(&expected.interrupts, &expected.registers, region)
    );
    let mut trace = Trace::default();
    let result = actual.advance_to(tick(at), region);
    assert_eq!(
        result,
        original_advance(expected, tick(at), region, &mut trace)
    );
    assert_eq!(
        actual, expected,
        "at={at} region={region:?} trace={trace:?}"
    );
    if result.is_err() {
        assert_eq!(*actual, before, "outer rollback at {at}");
    }
    (result, trace)
}

fn one(mut actual: State, at: u64, region: Region) -> (Result<(), VdpScheduleError>, Trace) {
    let mut expected = actual.clone();
    compare(&mut actual, &mut expected, at, region)
}

pub fn original_walker_regions_modes_and_field_cuts() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let period = region.frame_lines() * H32_LINE_TICKS;
        for epoch in [0, 137] {
            for width in [
                0,
                H40_CLOCK_MASK,
                INTERLACE_MODE_MASK,
                H40_CLOCK_MASK | INTERLACE_MODE_MASK,
            ] {
                for mode in [
                    MODE5_ENABLE,
                    MODE5_ENABLE | DISPLAY_ENABLE,
                    MODE5_ENABLE | V30_ENABLE,
                ] {
                    if mode & V30_ENABLE != 0 && region != Region::Pal {
                        continue;
                    }
                    let mut actual = state(epoch, epoch, mode, width, 0);
                    let mut expected = actual.clone();
                    let height = actual.registers.display_height(region) as u64;
                    let clock = if width & H40_CLOCK_MASK == 0 {
                        2660
                    } else {
                        2640
                    };
                    let latch = (height - 1) * H32_LINE_TICKS + clock;
                    let toggle =
                        height * H32_LINE_TICKS + if width & H40_CLOCK_MASK == 0 { 10 } else { 8 };
                    let mut cuts = vec![
                        0,
                        1,
                        19,
                        clock - 1,
                        clock,
                        clock + 1,
                        3 * H32_LINE_TICKS,
                        latch - 1,
                        latch,
                        latch + 1,
                        toggle - 1,
                        toggle,
                        toggle + 1,
                        period - 1,
                        period,
                        period + 1,
                        2 * period + H32_LINE_TICKS,
                        4 * period,
                    ];
                    cuts.sort_unstable();
                    for cut in cuts {
                        compare(&mut actual, &mut expected, epoch + cut, region)
                            .0
                            .unwrap();
                    }
                    assert!(actual.interrupts.field_count() >= 3);
                    assert_eq!(actual.now(), tick(epoch + 4 * period));
                }
            }
        }
        let initial = state(0, 0, MODE5_ENABLE, 0, 2);
        let (result, trace) = one(initial.clone(), 3 * period + 7, region);
        result.unwrap();
        assert!(trace.skips > 0);
        let mut no_skip = initial;
        no_skip.interrupts = interrupts(0, 0, 0, 0, 0, 0);
        let (result, trace) = one(no_skip, 2 * period + 7, region);
        result.unwrap();
        assert!(trace.interrupts > 2 && trace.pairs > trace.interrupts);
    }
}

pub fn original_walker_publications_latches_and_late_rollback() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for width in [0, H40_CLOCK_MASK] {
            let clock = if width == 0 { 2660 } else { 2640 };
            for publication in [clock - 20, clock, clock + 20] {
                for (index, value) in [
                    (MODE_REGISTER, MODE5_ENABLE | DISPLAY_ENABLE),
                    (DISPLAY_SIZE_REGISTER, width | INTERLACE_MODE_MASK),
                    (HINT_RELOAD_REGISTER, 0),
                    (HINT_ENABLE_REGISTER, HV_COUNTER_LATCH_MASK),
                ] {
                    let now = publication - 19;
                    let mut actual = state(0, now, MODE5_ENABLE, width, 0);
                    actual.registers = registers(
                        0,
                        now,
                        Some(MODE5_ENABLE),
                        Some(width),
                        Some(3),
                        Some((index, value)),
                    );
                    let mut expected = actual.clone();
                    for cut in [
                        now,
                        publication - 1,
                        publication,
                        publication + 1,
                        clock + H32_LINE_TICKS,
                    ] {
                        compare(&mut actual, &mut expected, cut, region).0.unwrap();
                    }
                    assert!(actual.registers.pending().is_none());
                    if index == HINT_ENABLE_REGISTER {
                        assert!(actual.hv_latch.is_some());
                        let next = actual.now().get() + 1;
                        compare(&mut actual, &mut expected, next, region).0.unwrap();
                        let write = actual
                            .queue_register(HINT_ENABLE_REGISTER, 0, region)
                            .unwrap();
                        assert_eq!(
                            expected.queue_register(HINT_ENABLE_REGISTER, 0, region),
                            Ok(write)
                        );
                        compare(&mut actual, &mut expected, write.publish_at.get(), region)
                            .0
                            .unwrap();
                        assert_eq!(actual.hv_latch, None);
                    }
                }
            }
        }
        let now = V28_VBLANK_SET_LINE * H32_LINE_TICKS + 1;
        for (index, value) in [
            (MODE_REGISTER, MODE5_ENABLE | DISPLAY_ENABLE),
            (DISPLAY_SIZE_REGISTER, H40_CLOCK_MASK | INTERLACE_MODE_MASK),
            (HINT_RELOAD_REGISTER, 0),
            (HINT_ENABLE_REGISTER, HV_COUNTER_LATCH_MASK),
        ] {
            let mut later = state(0, now, MODE5_ENABLE, H40_CLOCK_MASK, 0);
            later.registers = registers(
                0,
                now,
                Some(MODE5_ENABLE),
                Some(H40_CLOCK_MASK),
                Some(3),
                Some((index, value)),
            );
            let at = later.registers.pending().unwrap().publish_at.get();
            let (result, trace) = one(later, at, region);
            result.unwrap();
            assert_eq!(trace.interrupts, 1);
            assert_eq!(trace.publications, 1);
            assert_eq!(trace.pairs, 3);
        }
        let mut bad = state(0, now, MODE5_ENABLE, H40_CLOCK_MASK, 0);
        bad.registers = registers(
            0,
            now,
            Some(MODE5_ENABLE),
            Some(H40_CLOCK_MASK),
            Some(3),
            Some((DISPLAY_SIZE_REGISTER, 3)),
        );
        let pending = bad.registers.pending().unwrap();
        let (result, trace) = one(bad, pending.publish_at.get(), region);
        assert_eq!(
            result,
            Err(VdpInterruptError::from(VdpTimingError::UnsupportedMode {
                index: DISPLAY_SIZE_REGISTER,
                value: 3
            })
            .into())
        );
        assert_eq!(trace.interrupts, 1);
        assert_eq!(trace.pairs, 2);
    }
}

pub fn original_boundary_overflows_and_error_precedence() {
    let region = Region::NtscUs;
    let epoch = u64::MAX - 100;
    let end_overflow = state(epoch, epoch, MODE5_ENABLE, 0, 0);
    let boundary = end_overflow
        .interrupts
        .cycle_boundary(region, &end_overflow.registers)
        .unwrap();
    assert_eq!(boundary.0, None);
    assert!(boundary.1.is_some());
    assert_eq!(
        boundary.1.unwrap().end(),
        Err(VdpTimingError::ClockOverflow)
    );
    one(end_overflow, u64::MAX, region).0.unwrap();

    let now = region.frame_lines() * H32_LINE_TICKS + 1;
    let mut projection_overflow = state(0, now, MODE5_ENABLE, 0, 0);
    projection_overflow.interrupts = interrupts(0, 0, u64::MAX, 0, now, 0);
    projection_overflow.registers =
        registers(0, now, Some(MODE5_ENABLE), Some(0), Some(3), Some((15, 2)));
    assert_eq!(
        projection_overflow
            .interrupts
            .cycle_boundary(region, &projection_overflow.registers),
        Ok((None, None))
    );
    assert_eq!(
        one(projection_overflow, now, region).0,
        Err(VdpInterruptError::from(VdpTimingError::ClockOverflow).into())
    );

    let mut rollover = state(0, 0, MODE5_ENABLE, 0, 0);
    rollover.interrupts = interrupts(0, 0, u64::MAX, 0, 0, 0);
    let (result, trace) = one(rollover, region.frame_lines() * H32_LINE_TICKS, region);
    assert_eq!(
        result,
        Err(VdpInterruptError::from(VdpTimingError::ClockOverflow).into())
    );
    assert!(trace.interrupts > 1);

    for (mode, width) in [
        (None, Some(0)),
        (Some(0), Some(0)),
        (Some(MODE5_ENABLE), None),
        (Some(MODE5_ENABLE), Some(1)),
    ] {
        for fault in 0..4 {
            let mut bad = state(0, 100, MODE5_ENABLE, 0, 0);
            bad.interrupts = interrupts(0, 0, 0, 100, 100, 0);
            let register_now = if fault == 0 { 101 } else { 100 };
            let register_epoch = if fault == 1 { 1 } else { 0 };
            bad.registers = registers(
                register_epoch,
                register_now,
                mode,
                width,
                Some(3),
                Some((15, 2)),
            );
            if fault == 3 {
                bad.interrupts = interrupts(120, 120, 0, 100, 100, 0);
            }
            let at = if fault == 2 { 99 } else { 102 };
            let error = one(bad, at, region).0.unwrap_err();
            match fault {
                0 => assert!(matches!(error, VdpScheduleError::ClockMismatch { .. })),
                1 => assert_eq!(error, VdpScheduleError::PhaseMismatch),
                2 => assert!(matches!(
                    error,
                    VdpScheduleError::Interrupt(VdpInterruptError::TimeReversal { .. })
                )),
                _ => assert!(matches!(
                    error,
                    VdpScheduleError::Interrupt(VdpInterruptError::Timing(
                        VdpTimingError::BeforeEpoch { .. }
                    ))
                )),
            }
        }
    }
    let mut bad_skip = state(0, 0, 0, 0, 2);
    bad_skip.registers = registers(0, 1, Some(0), Some(0), None, None);
    assert_eq!(
        one(bad_skip, 1, region).0,
        Err(VdpInterruptError::from(VdpTimingError::UnsupportedMode {
            index: MODE_REGISTER,
            value: 0
        })
        .into())
    );
    let mut unknown_reload = state(0, 0, MODE5_ENABLE, 0, 2);
    unknown_reload.registers = registers(0, 0, Some(MODE5_ENABLE), Some(0), None, None);
    assert_eq!(
        one(unknown_reload, 1, region).0,
        Err(VdpInterruptError::from(VdpTimingError::UnknownRegister(HINT_RELOAD_REGISTER)).into())
    );
}

#[test]
fn state_original_walker_regions_modes_and_field_cuts() {
    original_walker_regions_modes_and_field_cuts();
}

#[test]
fn state_original_walker_publications_latches_and_late_rollback() {
    original_walker_publications_latches_and_late_rollback();
}

#[test]
fn state_original_boundary_overflows_and_error_precedence() {
    original_boundary_overflows_and_error_precedence();
}
