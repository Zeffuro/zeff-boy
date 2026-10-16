use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};

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
    w.write_bool(pending.is_some());
    if let Some((index, value)) = pending {
        w.write_u8(index);
        w.write_u8(value);
        w.write_u64(now + H32_INTERNAL_CLOCK_TICKS - (now - epoch) % H32_INTERNAL_CLOCK_TICKS);
    }
    for index in 0..REGISTER_COUNT {
        let value = match index {
            MODE_REGISTER => mode,
            DISPLAY_SIZE_REGISTER => width,
            HINT_RELOAD_REGISTER => reload,
            _ => Some(0),
        };
        w.write_bool(value.is_some());
        if let Some(value) = value {
            w.write_u8(value);
        }
    }
    VdpRegisterPipeline::read_state(&mut StateReader::new(&w.into_bytes())).unwrap()
}

fn reference_pair(
    interrupts: &mut VdpInterrupts,
    at: MasterTicks,
    region: Region,
    registers: &mut VdpRegisterPipeline,
    require_display: bool,
) -> Result<Option<VdpEvent>, VdpScheduleError> {
    if interrupts.now != registers.now() {
        return Err(VdpScheduleError::ClockMismatch {
            interrupt: interrupts.now,
            registers: registers.now(),
        });
    }
    let (epoch, published) = registers.published_state();
    if !interrupts.timing.shares_internal_phase(epoch) {
        return Err(VdpScheduleError::PhaseMismatch);
    }
    if at < interrupts.now {
        return Err(VdpInterruptError::TimeReversal {
            current: interrupts.now,
            requested: at,
        }
        .into());
    }
    let interrupt = match interrupts.timing.next_counter_edge(
        interrupts.now,
        region,
        &published,
        require_display,
    ) {
        Ok(edge) => Some(edge),
        Err(VdpTimingError::ClockOverflow) => None,
        Err(error) => return Err(VdpInterruptError::from(error).into()),
    };
    let write = registers.pending().filter(|write| write.publish_at <= at);
    if let (Some(edge), Some(write)) = (interrupt, write)
        && edge.at == write.publish_at
        && require_display
    {
        return Err(VdpScheduleError::CoincidentPublication(edge.at));
    }
    let event = match (interrupt.filter(|edge| edge.at <= at), write) {
        (Some(edge), Some(write)) if write.publish_at < edge.at => Some(VdpEvent::Register(write)),
        (Some(edge), _) => Some(VdpEvent::Interrupt(edge)),
        (None, Some(write)) => Some(VdpEvent::Register(write)),
        (None, None) => None,
    };
    let horizon = event.map_or(at, VdpEvent::at);
    let mut candidate = *interrupts;
    let mut candidate_registers = *registers;
    // Keep the independent edge query in the reference transaction.
    candidate.advance_in_mode(horizon, region, &published, require_display)?;
    candidate_registers.advance_to(horizon)?;
    candidate
        .timing
        .counter_in_mode(
            horizon,
            region,
            &candidate_registers.published_state().1,
            require_display,
        )
        .map_err(VdpInterruptError::from)?;
    *interrupts = candidate;
    *registers = candidate_registers;
    Ok(event)
}

fn compare(
    interrupts: VdpInterrupts,
    registers: VdpRegisterPipeline,
    at: u64,
    region: Region,
    require_display: bool,
) -> Result<Option<VdpEvent>, VdpScheduleError> {
    let (mut actual, mut actual_registers) = (interrupts, registers);
    let (mut expected, mut expected_registers) = (interrupts, registers);
    let a = actual.advance_pair(
        MasterTicks::new(at),
        region,
        &mut actual_registers,
        require_display,
    );
    let b = reference_pair(
        &mut expected,
        MasterTicks::new(at),
        region,
        &mut expected_registers,
        require_display,
    );
    assert_eq!(a, b);
    assert_eq!((actual, actual_registers), (expected, expected_registers));
    if a.is_err() {
        assert_eq!((actual, actual_registers), (interrupts, registers));
    }
    a
}

#[test]
fn prepared_edge_matches_independent_query_for_raw_modes_and_errors() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for require_display in [false, true] {
            for raw in 0..=255 {
                for (mode, width) in [(Some(raw), Some(0)), (Some(0x44), Some(raw))] {
                    let regs = registers(0, 0, mode, width, Some(3), None);
                    let irq = VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, false);
                    for at in [0, 1, 2659, 2660, 3000, 768000] {
                        let _ = compare(irq, regs, at, region, require_display);
                    }
                }
            }
            for (mode, width, reload) in [
                (None, Some(0), Some(0)),
                (Some(0x44), None, Some(0)),
                (Some(0x44), Some(0), None),
            ] {
                let regs = registers(0, 0, mode, width, reload, None);
                let irq = VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, false);
                assert!(compare(irq, regs, 1, region, require_display).is_err());
            }
        }
    }
}

#[test]
fn prepared_edge_retains_publication_order_and_atomic_failures() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for width in [
            0,
            H40_CLOCK_MASK,
            INTERLACE_MODE_MASK,
            H40_CLOCK_MASK | INTERLACE_MODE_MASK,
        ] {
            for mode in [0x04, 0x44, 0x0c, 0x4c] {
                for require_display in [false, true] {
                    for now in [0, 1, 2641, 2659, 2660, 3420 * 224 - 1] {
                        for pending in [
                            None,
                            Some((HINT_RELOAD_REGISTER, 0)),
                            Some((MODE_REGISTER, mode ^ DISPLAY_ENABLE)),
                            Some((DISPLAY_SIZE_REGISTER, width ^ INTERLACE_MODE_MASK)),
                            Some((HINT_ENABLE_REGISTER, HINT_ENABLE_MASK)),
                        ] {
                            let regs = registers(0, now, Some(mode), Some(width), Some(0), pending);
                            let mut irq =
                                VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, true, true);
                            irq.now = MasterTicks::new(now);
                            for at in [now, now + 1, now + 20, now + 3420] {
                                let _ = compare(irq, regs, at, region, require_display);
                            }
                        }
                    }
                }
            }
        }
    }
    let regs = registers(
        0,
        2641,
        Some(0x44),
        Some(0),
        Some(0),
        Some((HINT_RELOAD_REGISTER, 7)),
    );
    let mut irq = VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, false);
    irq.now = MasterTicks::new(2641);
    assert_eq!(
        compare(irq, regs, 2660, Region::NtscUs, true),
        Err(VdpScheduleError::CoincidentPublication(MasterTicks::new(
            2660
        )))
    );
}

#[test]
fn prepared_edge_retains_phase_time_epoch_and_overflow_contracts() {
    let regs = registers(0, 100, Some(0x44), Some(0), Some(0), None);
    let mut irq = VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, false);
    assert!(matches!(
        compare(irq, regs, 101, Region::NtscUs, false),
        Err(VdpScheduleError::ClockMismatch { .. })
    ));
    irq.now = MasterTicks::new(100);
    assert!(matches!(
        compare(irq, regs, 99, Region::NtscUs, false),
        Err(VdpScheduleError::Interrupt(
            VdpInterruptError::TimeReversal { .. }
        ))
    ));
    irq.timing = VdpVerticalTiming::at_counters_zero(MasterTicks::new(1));
    assert_eq!(
        compare(irq, regs, 101, Region::NtscUs, false),
        Err(VdpScheduleError::PhaseMismatch)
    );
    irq.timing = VdpVerticalTiming::at_counters_zero(MasterTicks::new(120));
    assert!(compare(irq, regs, 101, Region::NtscUs, false).is_err());
    for epoch in [u64::MAX - 20, u64::MAX - 3420 * 263] {
        let regs = registers(epoch, epoch, Some(0x44), Some(0), Some(0), None);
        let irq = VdpInterrupts::at_counters_zero(MasterTicks::new(epoch), 0, false, false);
        let _ = compare(irq, regs, u64::MAX, Region::NtscUs, false);
    }
}

#[test]
fn prepared_edge_matches_multiple_interlaced_fields_and_acknowledge_ties() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for width in [
            0,
            H40_CLOCK_MASK,
            INTERLACE_MODE_MASK,
            H40_CLOCK_MASK | INTERLACE_MODE_MASK,
        ] {
            let mut regs = registers(0, 0, Some(0x64), Some(width), Some(2), None);
            let mut irq = VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 2, false, false);
            let end = 4 * region.frame_lines() * H32_LINE_TICKS;
            loop {
                let mut expected_irq = irq;
                let mut expected_regs = regs;
                let expected = reference_pair(
                    &mut expected_irq,
                    MasterTicks::new(end),
                    region,
                    &mut expected_regs,
                    false,
                );
                let actual = irq.advance_pair(MasterTicks::new(end), region, &mut regs, false);
                assert_eq!(actual, expected);
                assert_eq!((irq, regs), (expected_irq, expected_regs));
                if actual.unwrap().is_none() {
                    break;
                }
            }
            assert!(irq.field_count() >= 3);
            let mut regs = registers(
                0,
                2641,
                Some(0x64),
                Some(0),
                Some(0),
                Some((HINT_RELOAD_REGISTER, 1)),
            );
            let mut irq = VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, false);
            irq.now = MasterTicks::new(2641);
            let original = (irq, regs);
            assert_eq!(
                irq.acknowledge_interval(MasterTicks::new(2660), region, &mut regs, false),
                Err(VdpScheduleError::CoincidentAcknowledge(MasterTicks::new(
                    2660
                )))
            );
            assert_eq!((irq, regs), original);
        }
    }
}
