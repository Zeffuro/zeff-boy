use super::*;

#[test]
fn current_position_survives_unrepresentable_future_field_edges() {
    let epoch = MasterTicks::new(u64::MAX - 195);
    let now = MasterTicks::new(u64::MAX);
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for h40 in [false, true] {
            for interlaced in [false, true] {
                let registers = registers(region, h40, false, interlaced);
                let mut timing = VdpVerticalTiming::at_counters_zero(epoch);
                timing.advance_calendar(now, region, &registers).unwrap();
                let position = timing.field_position_at(now, region, &registers).unwrap();
                assert_eq!((position.id, position.start, position.line), (0, epoch, 0));
                assert_eq!(position.lines, region.frame_lines());
                assert!(!position.interlaced);
                assert_eq!(position.end(), Err(VdpTimingError::ClockOverflow));
                assert_eq!(timing.v_counter_at(now, region, &registers), Ok(0));
                assert_eq!(timing.vblank_flag_at(now, region, &registers), Ok(false));
                assert!(timing.hv_counter_at(now, region, &registers).is_ok());
                timing.validate_calendar(now, region).unwrap();
                assert_eq!(
                    timing.next_interrupt_edge(now, region, &registers),
                    Err(VdpTimingError::ClockOverflow)
                );
                assert_eq!(
                    timing.next_vblank_edge(now, region, &registers),
                    Err(VdpTimingError::ClockOverflow)
                );
            }
        }
    }
}

#[test]
fn publication_at_clock_tail_does_not_require_a_future_latch() {
    let region = Region::NtscUs;
    let epoch = MasterTicks::new(u64::MAX - 195);
    let mut registers = registers(region, false, false, false);
    registers.advance_to(epoch).unwrap();
    let write = registers
        .queue_control_register(DISPLAY_SIZE_REGISTER, INTERLACE_MODE_MASK)
        .unwrap();
    assert!(write.publish_at > epoch);
    let (calendar, projected) = FieldCalendar::new(epoch)
        .project(MasterTicks::new(u64::MAX), region, &registers)
        .unwrap();
    assert_eq!(calendar.start, epoch);
    assert!(!calendar.interlaced);
    assert_eq!(
        projected.registers()[usize::from(DISPLAY_SIZE_REGISTER)],
        Some(INTERLACE_MODE_MASK)
    );
    assert!(projected.pending().is_none());
    assert!(registers.pending().is_some());
}

#[test]
fn progressed_sound_snapshot_keeps_past_pulses_and_pending_future_mode() {
    for region in [Region::NtscUs, Region::Pal] {
        for h40 in [false, true] {
            let mut registers = registers(region, h40, false, false);
            let mut interrupts =
                VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, false);
            while interrupts
                .advance_in_mode(registers.now(), region, &registers, false)
                .unwrap()
                .is_some()
            {}
            let period = region.frame_lines() * H32_LINE_TICKS;
            let now = 4 * period + 100;
            advance(&mut interrupts, now, region, &mut registers);
            registers
                .queue_control_register(
                    DISPLAY_SIZE_REGISTER,
                    (if h40 { H40_CLOCK_MASK } else { 0 }) | INTERLACE_MODE_MASK,
                )
                .unwrap();
            let snapshot = interrupts.sound_interrupt(registers, region);
            let mode = VdpVerticalTiming::horizontal_mode(&registers).unwrap();
            let pulse = 224 * H32_LINE_TICKS + mode.vertical_interrupt();
            let enabled_length = if region == Region::Pal { 313 } else { 263 } * H32_LINE_TICKS;
            for start in [0, period, 4 * period, 4 * period + enabled_length] {
                assert_eq!(snapshot.asserted_at(start + pulse - 1), Ok(false));
                assert_eq!(snapshot.asserted_at(start + pulse), Ok(true));
                assert_eq!(
                    snapshot.asserted_at(start + pulse + H32_LINE_TICKS),
                    Ok(false)
                );
            }
            assert!(registers.pending().is_some());
            assert_eq!(interrupts.now().get(), now);
        }
    }
}

#[test]
fn backward_sound_queries_after_interlace_exit_stay_in_known_field() {
    let region = Region::NtscUs;
    let mut registers = registers(region, false, false, true);
    let mut interrupts = VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, false);
    while interrupts
        .advance_in_mode(registers.now(), region, &registers, false)
        .unwrap()
        .is_some()
    {}
    advance(
        &mut interrupts,
        263 * H32_LINE_TICKS + 100,
        region,
        &mut registers,
    );
    registers
        .queue_control_register(DISPLAY_SIZE_REGISTER, 0)
        .unwrap();
    let start = (263 + 262) * H32_LINE_TICKS;
    advance(
        &mut interrupts,
        start + 225 * H32_LINE_TICKS,
        region,
        &mut registers,
    );
    let snapshot = interrupts.sound_interrupt(registers, region);
    let pulse = 224 * H32_LINE_TICKS + H32_DOT_TICKS;
    assert_eq!(snapshot.asserted_at(start + pulse), Ok(true));
    assert!(matches!(
        snapshot.asserted_at(pulse),
        Err(VdpTimingError::BeforeEpoch { .. })
    ));
}
