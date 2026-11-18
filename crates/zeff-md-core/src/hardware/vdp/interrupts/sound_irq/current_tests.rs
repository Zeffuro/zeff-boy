use super::*;
use crate::hardware::vdp::InterlaceMode;
use crate::hardware::vdp::state::{StateReader, StateWriter, write_optional};
use crate::hardware::vdp::timing::SoundRegisters;
use crate::hardware::vdp::timing::clock::LineClock;
use crate::hardware::vdp::timing::cursor::LineCursor;
use crate::hardware::vdp::timing::history::TimingHistory;

impl SoundInterrupt {
    fn original_asserted_at(&self, now: u64) -> Result<bool, VdpTimingError> {
        let at = MasterTicks::new(now);
        if self.timing.uses_history(&self.registers) {
            return self.anchored_asserted_at(at);
        }
        let (calendar, registers) =
            self.timing
                .sound_calendar_at(at, self.region, &self.registers)?;
        let mode = self.timing.sound_mode_at(at, self.region, &registers)?;
        let start = registers.display_height(self.region) as u64 * H32_LINE_TICKS
            + mode.vertical_interrupt();
        let phase = now - calendar.start.get();
        Ok((start..start + H32_LINE_TICKS).contains(&phase))
    }
}

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
    VdpRegisterPipeline::read_state(&mut StateReader::new(&writer.into_bytes())).unwrap()
}

fn calendar(epoch: u64, start: u64, id: u64, updated: u64) -> VdpVerticalTiming {
    let mut timing = VdpVerticalTiming::at_counters_zero(MasterTicks::new(epoch));
    timing.field.start = MasterTicks::new(start);
    timing.field.id = id;
    timing.field.updated_at = MasterTicks::new(updated);
    timing
}

fn snapshot(
    timing: VdpVerticalTiming,
    registers: &VdpRegisterPipeline,
    region: Region,
) -> SoundInterrupt {
    SoundInterrupt {
        blank: None,
        timing,
        registers: SoundRegisters::capture(registers),
        region,
    }
}

fn compare(view: &SoundInterrupt, now: u64) -> Result<bool, VdpTimingError> {
    let before = *view;
    let actual = view.asserted_at(now);
    assert_eq!(
        actual,
        view.original_asserted_at(now),
        "at={now} view={view:?}"
    );
    assert_eq!(*view, before);
    actual
}

fn roundtrip(timing: VdpVerticalTiming) -> VdpVerticalTiming {
    let mut writer = StateWriter::new();
    timing.write_state(&mut writer);
    let bytes = writer.into_bytes();
    let restored = VdpVerticalTiming::read_state(&mut StateReader::new(&bytes)).unwrap();
    assert_eq!(restored, timing);
    let mut writer = StateWriter::new();
    restored.write_state(&mut writer);
    assert_eq!(writer.into_bytes(), bytes);
    restored
}

#[test]
fn current_query_all_optional_mode_bytes_preserve_results_and_first_errors() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for epoch in [0, 137] {
            let timing = calendar(epoch, epoch, 0, epoch);
            for r1 in std::iter::once(None).chain((0..=255).map(Some)) {
                for r12 in std::iter::once(None).chain((0..=255).map(Some)) {
                    let registers = raw_registers(epoch, epoch, r1, r12, None);
                    let view = snapshot(timing, &registers, region);
                    for at in [epoch, epoch + 1] {
                        let _ = compare(&view, at);
                    }
                    if epoch != 0 {
                        assert_eq!(
                            compare(&view, epoch - 1),
                            Err(VdpTimingError::BeforeEpoch {
                                epoch: MasterTicks::new(epoch),
                                requested: MasterTicks::new(epoch - 1),
                            })
                        );
                    }
                }
            }
        }
    }
    let timing = calendar(137, 137, 0, 136);
    let registers = raw_registers(137, 137, None, Some(0x80), None);
    assert_eq!(
        compare(&snapshot(timing, &registers, Region::NtscUs), 136),
        Err(VdpTimingError::BeforeEpoch {
            epoch: MasterTicks::new(137),
            requested: MasterTicks::new(136),
        })
    );
}

#[test]
fn current_query_pending_publications_keep_same_tick_image_and_error_order() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for publication in [
            H32_INTERNAL_CLOCK_TICKS,
            V28_VBLANK_SET_LINE * H32_LINE_TICKS,
            region.frame_lines() * H32_LINE_TICKS,
        ] {
            for index in 0..REGISTER_COUNT {
                for value in [0, 1, MODE5_ENABLE, H40_CLOCK_MASK, 0x80, 0xff] {
                    for (r1, r12) in [
                        (Some(MODE5_ENABLE), Some(0)),
                        (Some(MODE5_ENABLE | V30_ENABLE), Some(H40_CLOCK_MASK)),
                        (None, Some(0x80)),
                        (Some(MODE5_ENABLE), None),
                    ] {
                        let registers = raw_registers(
                            publication % H32_INTERNAL_CLOCK_TICKS,
                            publication - 1,
                            r1,
                            r12,
                            Some((index, value, publication)),
                        );
                        for updated in [publication - 1, publication, publication + 1] {
                            let view = snapshot(calendar(0, 0, 0, updated), &registers, region);
                            for at in [publication - 1, publication, publication + 1] {
                                let _ = compare(&view, at);
                            }
                        }
                    }
                }
            }
        }
    }
    let publication = H32_INTERNAL_CLOCK_TICKS;
    let registers = raw_registers(
        0,
        publication - 1,
        None,
        Some(0x80),
        Some((0, 0xff, publication)),
    );
    let view = snapshot(calendar(0, 0, 0, publication), &registers, Region::NtscUs);
    assert_eq!(
        compare(&view, publication),
        Err(VdpTimingError::UnknownRegister(MODE_REGISTER))
    );
    assert_eq!(
        compare(&view, publication + 1),
        Err(VdpTimingError::UnsupportedMode {
            index: DISPLAY_SIZE_REGISTER,
            value: 0x80,
        })
    );
}

#[test]
fn current_query_pulse_vblank_hirq_and_field_edges_match_after_raw19_restore() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for epoch in [0, 137] {
            for horizontal in [0, H40_CLOCK_MASK] {
                for height in [0, V30_ENABLE] {
                    if height != 0 && region != Region::Pal {
                        continue;
                    }
                    let registers = raw_registers(
                        epoch,
                        epoch,
                        Some(MODE5_ENABLE | height),
                        Some(horizontal),
                        None,
                    );
                    let clock = if horizontal == 0 {
                        LineClock::H32
                    } else {
                        LineClock::ExternalH40
                    };
                    let lines = if height == 0 {
                        V28_VBLANK_SET_LINE
                    } else {
                        V30_VBLANK_SET_LINE
                    };
                    let onset = epoch + lines * H32_LINE_TICKS + clock.vertical_interrupt();
                    let latch =
                        epoch + lines * H32_LINE_TICKS - H32_LINE_TICKS + clock.vertical_clock();
                    let end = epoch + region.frame_lines() * H32_LINE_TICKS;
                    for boundary in [
                        epoch + clock.vertical_clock(),
                        latch,
                        onset,
                        onset + H32_LINE_TICKS,
                        end - H32_LINE_TICKS,
                        end,
                    ] {
                        for updated in [boundary - 1, boundary, boundary + 1] {
                            let field_start = if updated >= end { end } else { epoch };
                            let timing = roundtrip(calendar(
                                epoch,
                                field_start,
                                u64::from(updated >= end),
                                updated,
                            ));
                            timing
                                .validate_calendar(MasterTicks::new(updated), region)
                                .unwrap();
                            let view = snapshot(timing, &registers, region);
                            for at in [boundary - 1, boundary, boundary + 1] {
                                let _ = compare(&view, at);
                            }
                        }
                    }
                    for (at, expected) in [
                        (onset - 1, false),
                        (onset, true),
                        (onset + H32_LINE_TICKS - 1, true),
                        (onset + H32_LINE_TICKS, false),
                    ] {
                        let view = snapshot(calendar(epoch, epoch, 0, at), &registers, region);
                        assert_eq!(compare(&view, at), Ok(expected));
                    }
                }
            }
        }
    }
}

#[test]
fn current_query_interlace_backward_normalization_and_overflow_remain_exact() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let period = region.frame_lines() * H32_LINE_TICKS;
        for epoch in [137, u64::MAX - 3 * period] {
            for (id, offset) in [(0, 0), (1, period), (2, period), (u64::MAX, period)] {
                for mode in [
                    InterlaceMode::Progressive,
                    InterlaceMode::Normal,
                    InterlaceMode::Double,
                ] {
                    for (odd, active, active_odd) in [
                        (false, InterlaceMode::Progressive, false),
                        (false, InterlaceMode::Normal, true),
                        (true, InterlaceMode::Double, false),
                    ] {
                        let start = epoch + offset;
                        let mut timing = calendar(epoch, start, id, start + 1);
                        timing.field.mode = mode;
                        timing.field.odd = odd;
                        timing.field.active_mode = active;
                        timing.field.active_odd = active_odd;
                        let timing = roundtrip(timing);
                        for r12 in [
                            None,
                            Some(0),
                            Some(2),
                            Some(6),
                            Some(0x80),
                            Some(H40_CLOCK_MASK),
                        ] {
                            let registers =
                                raw_registers(epoch, epoch, Some(MODE5_ENABLE), r12, None);
                            let view = snapshot(timing, &registers, region);
                            for at in [epoch - 1, epoch, start, start + 1, start + period] {
                                let _ = compare(&view, at);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn current_query_retained_history_and_pending_internal_dispatch_win_equality() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for clock in [
            LineClock::H32,
            LineClock::ExternalH40,
            LineClock::InternalH40,
        ] {
            for line in [0, 223, 224, 225, region.frame_lines() as u16 - 1] {
                let epoch = 137;
                let line_start = epoch + u64::from(line) * clock.ticks();
                let updated = line_start + clock.vertical_interrupt();
                for r12 in [0, 1, H40_CLOCK_MASK] {
                    let mut timing = calendar(epoch, epoch, 0, updated);
                    timing.history = Some(TimingHistory {
                        cursor: LineCursor {
                            start: MasterTicks::new(line_start),
                            line,
                            clock,
                        },
                        pulse_end: (line == 224)
                            .then_some(MasterTicks::new(updated + clock.ticks())),
                    });
                    let timing = roundtrip(timing);
                    timing
                        .validate_calendar(MasterTicks::new(updated), region)
                        .unwrap();
                    let registers =
                        raw_registers(epoch, updated, Some(MODE5_ENABLE), Some(r12), None);
                    let view = snapshot(timing, &registers, region);
                    assert!(view.timing.uses_history(&view.registers));
                    for at in [
                        updated - 1,
                        updated,
                        updated + 1,
                        line_start + clock.ticks(),
                        updated + clock.ticks(),
                    ] {
                        let _ = compare(&view, at);
                    }
                    assert_eq!(compare(&view, updated), Ok(line == 224));
                }
            }
        }
        let publication = H32_INTERNAL_CLOCK_TICKS;
        let registers = raw_registers(
            0,
            publication - 1,
            Some(MODE5_ENABLE),
            Some(0),
            Some((DISPLAY_SIZE_REGISTER, 1, publication)),
        );
        for updated in [publication - 1, publication, publication + 1] {
            let view = snapshot(calendar(0, 0, 0, updated), &registers, region);
            assert!(view.timing.uses_history(&view.registers));
            for at in [publication - 1, publication, publication + 1] {
                let _ = compare(&view, at);
            }
        }
        let mut timing = calendar(0, 0, 0, 0);
        timing.history = Some(TimingHistory {
            cursor: LineCursor {
                start: MasterTicks::ZERO,
                line: u16::MAX,
                clock: LineClock::H32,
            },
            pulse_end: None,
        });
        let registers = raw_registers(0, 0, None, Some(0x80), None);
        let view = snapshot(timing, &registers, region);
        assert_eq!(
            compare(&view, 0),
            Err(VdpTimingError::UnqualifiedDisplayHistory)
        );
    }
}
