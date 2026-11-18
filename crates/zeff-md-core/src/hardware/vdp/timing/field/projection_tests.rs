use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter, write_optional};
use crate::hardware::vdp::{VdpInterrupts, VdpRegisterWrite};

fn raw_registers(
    epoch: u64,
    now: u64,
    r1: Option<u8>,
    r12: Option<u8>,
    pending: Option<(u8, u8, u64)>,
) -> VdpRegisterPipeline {
    let mut writer = StateWriter::new();
    writer.write_u64(epoch);
    writer.write_u64(now);
    write_optional(&mut writer, pending, |writer, (index, value, at)| {
        writer.write_u8(index);
        writer.write_u8(value);
        writer.write_u64(at);
    });
    for index in 0..REGISTER_COUNT {
        write_optional(
            &mut writer,
            match index {
                MODE_REGISTER => r1,
                DISPLAY_SIZE_REGISTER => r12,
                _ => Some(0),
            },
            StateWriter::write_u8,
        );
    }
    let bytes = writer.into_bytes();
    VdpRegisterPipeline::read_state(&mut StateReader::new(&bytes)).unwrap()
}

fn compare(
    timing: VdpVerticalTiming,
    registers: VdpRegisterPipeline,
    region: Region,
    at: u64,
) -> Result<bool, VdpTimingError> {
    assert!(timing.blank.is_none());
    let mut writer = StateWriter::new();
    timing.write_state(&mut writer);
    writer.write_u64(timing.field.updated_at.get());
    writer.write_u8(0);
    writer.write_bool(false);
    writer.write_bool(false);
    VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, false)
        .write_blank_state(&mut writer)
        .unwrap();
    let bytes = writer.into_bytes();
    let mut reader = StateReader::new(&bytes);
    let interrupts = VdpInterrupts::read_state(&mut reader, region).unwrap();
    assert!(reader.is_exhausted());
    let before = registers;
    let actual = interrupts
        .sound_interrupt(&registers, region)
        .asserted_at(at);
    assert_eq!(
        actual,
        oracle::asserted_at(timing, registers, region, at),
        "region={region:?} at={at} timing={timing:?} registers={registers:?}"
    );
    let full = timing
        .sound_calendar_at(MasterTicks::new(at), region, &registers)
        .map(|(field, registers)| {
            (
                (
                    field.start,
                    field.id,
                    field.updated_at,
                    field.mode.interlaced(),
                    field.odd,
                    field.active_mode.interlaced(),
                    field.active_odd,
                ),
                registers,
            )
        });
    let original = timing
        .oracle_sound_calendar_at(MasterTicks::new(at), region, &registers)
        .map(|(field, registers)| {
            (
                (
                    field.start,
                    field.id,
                    field.updated_at,
                    field.interlaced,
                    field.odd,
                    field.active_interlaced,
                    field.active_odd,
                ),
                registers,
            )
        });
    assert_eq!(full, original);
    assert_eq!(registers, before);
    actual
}

fn values() -> impl Iterator<Item = Option<u8>> {
    std::iter::once(None).chain((0..=255).map(Some))
}

#[test]
fn published_raw_bytes_match_full_oracle_at_shortcut_and_event_queries() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for epoch in [0, 137] {
            let timing = VdpVerticalTiming::at_counters_zero(MasterTicks::new(epoch));
            for r1 in values() {
                for r12 in values() {
                    let registers = raw_registers(epoch, epoch, r1, r12, None);
                    for at in [
                        epoch,
                        epoch + 1,
                        epoch + 2 * region.frame_lines() * H32_LINE_TICKS,
                    ] {
                        let _ = compare(timing, registers, region, at);
                    }
                }
            }
        }
    }
}

#[test]
fn unrelated_pending_writes_keep_calendar_branch_and_exact_error_priority() {
    let timing = VdpVerticalTiming::at_counters_zero(MasterTicks::ZERO);
    let no_pending = raw_registers(0, 1, None, Some(0x80), None);
    assert_eq!(
        compare(timing, no_pending, Region::NtscUs, 1),
        Err(VdpTimingError::UnknownRegister(MODE_REGISTER))
    );
    for index in 0..REGISTER_COUNT {
        let registers = raw_registers(
            0,
            1,
            None,
            Some(0x80),
            Some((index, 0xff, H32_INTERNAL_CLOCK_TICKS)),
        );
        assert_eq!(
            compare(timing, registers, Region::NtscUs, 1),
            Err(VdpTimingError::UnsupportedMode {
                index: DISPLAY_SIZE_REGISTER,
                value: 0x80,
            })
        );
        assert_eq!(
            compare(timing, registers, Region::NtscUs, 0),
            Err(VdpTimingError::UnknownRegister(MODE_REGISTER))
        );
    }
}

#[test]
fn every_pending_register_and_raw_mode_value_match_at_publication_and_field_edges() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let timing = VdpVerticalTiming::at_counters_zero(MasterTicks::ZERO);
        let height = V28_VBLANK_SET_LINE;
        let latch = VdpVerticalTiming::blank_offset(
            height,
            crate::hardware::vdp::mode::HorizontalMode::H32.vertical_clock(),
        );
        let toggle = height * H32_LINE_TICKS + H32_VINT_COUNTER * H32_DOT_TICKS;
        let end = region.frame_lines() * H32_LINE_TICKS;
        for publication in [H32_INTERNAL_CLOCK_TICKS, latch, toggle, end] {
            let register_epoch = publication % H32_INTERNAL_CLOCK_TICKS;
            for index in 0..REGISTER_COUNT {
                let pending_values: Vec<u8> =
                    if matches!(index, MODE_REGISTER | DISPLAY_SIZE_REGISTER) {
                        (0..=255).collect()
                    } else {
                        vec![0, MODE5_ENABLE, 0x80, 0xff]
                    };
                for value in pending_values {
                    for (r1, r12) in [
                        (Some(MODE5_ENABLE), Some(0)),
                        (
                            Some(MODE5_ENABLE | V30_ENABLE),
                            Some(H40_CLOCK_MASK | INTERLACE_MODE_MASK),
                        ),
                        (None, Some(0)),
                        (Some(MODE5_ENABLE), None),
                    ] {
                        if index == DISPLAY_SIZE_REGISTER
                            && value & (H40_CLOCK_MASK | INTERLACE_MODE_MASK) == 1
                            && r1 == Some(MODE5_ENABLE)
                            && r12 == Some(0)
                        {
                            continue;
                        }
                        let registers = raw_registers(
                            register_epoch,
                            publication - 1,
                            r1,
                            r12,
                            Some((index, value, publication)),
                        );
                        for at in [
                            publication - 1,
                            publication,
                            publication + 1,
                            publication + end,
                        ] {
                            let _ = compare(timing, registers, region, at);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn calendar_history_interlace_rollover_and_clock_limits_match_full_oracle() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let period = region.frame_lines() * H32_LINE_TICKS;
        for epoch in [0, 137, u64::MAX - 3 * period] {
            for (interlaced, odd, active_interlaced, active_odd) in [
                (false, false, false, false),
                (false, false, true, true),
                (true, false, false, false),
                (true, false, true, true),
                (true, true, true, false),
            ] {
                for (id, offset) in [(0, 0), (1, period), (2, period), (u64::MAX, period)] {
                    let start = epoch + offset;
                    let timing = VdpVerticalTiming {
                        blank: None,
                        history: None,
                        horizontal: VdpH32Timing::at_counter_zero(MasterTicks::new(epoch)),
                        field: FieldCalendar {
                            start: MasterTicks::new(start),
                            id,
                            updated_at: MasterTicks::new(start + 1),
                            mode: if interlaced {
                                InterlaceMode::Double
                            } else {
                                InterlaceMode::Progressive
                            },
                            odd,
                            active_mode: if active_interlaced {
                                InterlaceMode::Double
                            } else {
                                InterlaceMode::Progressive
                            },
                            active_odd,
                        },
                    };
                    for r1 in [
                        None,
                        Some(0),
                        Some(MODE5_ENABLE),
                        Some(MODE5_ENABLE | V30_ENABLE),
                    ] {
                        for r12 in [
                            None,
                            Some(0),
                            Some(0x80),
                            Some(INTERLACE_MODE_MASK),
                            Some(H40_CLOCK_MASK),
                        ] {
                            let registers = raw_registers(epoch, epoch, r1, r12, None);
                            let mut queries =
                                vec![epoch, start, start + 1, start + period, u64::MAX];
                            if epoch > 0 {
                                queries.push(epoch - 1);
                            }
                            if start > epoch {
                                queries.push(start - 1);
                            }
                            queries.retain(|&at| at < start || at - start <= 3 * period);
                            for at in queries {
                                let _ = compare(timing, registers, region, at);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn capture_owns_prepublication_values_and_preserves_pulse_assert_and_deassert() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let base = V28_VBLANK_SET_LINE * H32_LINE_TICKS;
        let publication = base + H32_INTERNAL_CLOCK_TICKS;
        let mut registers = raw_registers(
            0,
            publication - 1,
            Some(MODE5_ENABLE),
            Some(0),
            Some((DISPLAY_SIZE_REGISTER, H40_CLOCK_MASK, publication)),
        );
        let interrupts = VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, false);
        let snapshot = interrupts.sound_interrupt(&registers, region);
        assert_eq!(
            registers.advance_to(MasterTicks::new(publication)),
            Ok(Some(VdpRegisterWrite {
                index: DISPLAY_SIZE_REGISTER,
                value: H40_CLOCK_MASK,
                publish_at: MasterTicks::new(publication),
            }))
        );
        let h40_start = base + u64::from(H40_DOT_TICKS);
        let h32_start = base + H32_VINT_COUNTER * H32_DOT_TICKS;
        let late_snapshot = interrupts.sound_interrupt(&registers, region);
        assert_eq!(snapshot.asserted_at(h40_start), Ok(false));
        assert_eq!(late_snapshot.asserted_at(h40_start), Ok(true));
        for (at, expected) in [
            (publication - 1, true),
            (publication, true),
            (h32_start - 1, false),
            (h32_start, true),
            (h40_start + H32_LINE_TICKS - 1, true),
            (h40_start + H32_LINE_TICKS, false),
        ] {
            assert_eq!(snapshot.asserted_at(at), Ok(expected));
        }
        assert_eq!(snapshot.asserted_at(0), Ok(false));
    }
}

#[test]
fn compact_publication_preserves_register_clock_reversal_and_clears_every_pending_write() {
    for index in 0..REGISTER_COUNT {
        let mut full = raw_registers(
            0,
            1,
            None,
            Some(0x80),
            Some((index, 0xff, H32_INTERNAL_CLOCK_TICKS)),
        );
        let mut compact = SoundRegisters::capture(&full);
        let before = compact;
        assert_eq!(
            compact.advance_calendar_to(MasterTicks::ZERO),
            full.advance_to(MasterTicks::ZERO).map(|_| ())
        );
        assert_eq!(compact, before);
        assert_eq!(
            compact.advance_calendar_to(MasterTicks::ZERO),
            Err(crate::hardware::vdp::VdpRegisterError::TimeReversal {
                current: MasterTicks::new(1),
                requested: MasterTicks::ZERO,
            })
        );
        for at in [
            H32_INTERNAL_CLOCK_TICKS - 1,
            H32_INTERNAL_CLOCK_TICKS,
            H32_INTERNAL_CLOCK_TICKS + 1,
        ] {
            assert_eq!(
                compact.advance_calendar_to(MasterTicks::new(at)),
                full.advance_to(MasterTicks::new(at)).map(|_| ())
            );
            assert_eq!(compact.mode_byte(), full.mode_byte());
            assert_eq!(compact.size_byte(), full.size_byte());
            assert_eq!(compact.pending_publication(), full.pending_publication());
        }
        assert_eq!(compact.pending_publication(), None);
    }
}
