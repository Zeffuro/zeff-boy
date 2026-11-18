use super::*;
use crate::hardware::vdp::{Vdp, VdpRegisterWrite};
use crate::save_state::read_section;

fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}

fn registers(now: u64, size: u8, pending: Option<VdpRegisterWrite>) -> VdpRegisterPipeline {
    let mut writer = StateWriter::new();
    writer.write_u64(0);
    writer.write_u64(now);
    write_optional(&mut writer, pending, |w, write| {
        w.write_u8(write.index);
        w.write_u8(write.value);
        w.write_u64(write.publish_at.get());
    });
    for index in 0..REGISTER_COUNT {
        writer.write_bool(true);
        writer.write_u8(match index {
            MODE_REGISTER => MODE5_ENABLE,
            DISPLAY_SIZE_REGISTER => size,
            _ => 0,
        });
    }
    VdpRegisterPipeline::read_state(&mut StateReader::new(&writer.into_bytes())).unwrap()
}

fn anchored(region: Region, line: u16, phase: u64) -> VdpVerticalTiming {
    let field_start = region.frame_lines() * H32_LINE_TICKS * 3 - 60;
    let start = field_start + u64::from(line) * clock::INTERNAL_LINE_TICKS;
    VdpVerticalTiming {
        blank: None,
        horizontal: VdpH32Timing::at_counter_zero(MasterTicks::ZERO),
        field: field::FieldCalendar {
            start: tick(field_start),
            id: 3,
            updated_at: tick(start + phase),
            ..field::FieldCalendar::new(MasterTicks::ZERO)
        },
        history: Some(history::TimingHistory {
            cursor: cursor::LineCursor {
                start: tick(start),
                line,
                clock: clock::LineClock::InternalH40,
            },
            pulse_end: None,
        }),
    }
}

fn timing_bytes(timing: VdpVerticalTiming) -> Vec<u8> {
    let mut writer = StateWriter::new();
    timing.write_state(&mut writer);
    writer.into_bytes()
}

#[test]
fn raw19_history_roundtrips_each_clock_and_finite_pulse() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for clock in [
            clock::LineClock::H32,
            clock::LineClock::ExternalH40,
            clock::LineClock::InternalH40,
        ] {
            let mut timing = anchored(region, 224, 18);
            let history = timing.history.as_mut().unwrap();
            history.cursor.clock = clock;
            history.pulse_end = Some(tick(
                history.cursor.start.get() + clock.vertical_interrupt() + clock.ticks(),
            ));
            timing
                .validate_calendar(timing.field.updated_at, region)
                .unwrap();
            let bytes = timing_bytes(timing);
            let mut reader = StateReader::new(&bytes);
            assert_eq!(VdpVerticalTiming::read_state(&mut reader).unwrap(), timing);
            assert!(reader.is_exhausted());
        }
    }
}

#[test]
fn history_validation_rejects_corrupt_cursor_identity_parity_and_deadlines() {
    let good = anchored(Region::NtscUs, 42, 100);
    let mut invalid = Vec::new();
    let mut shifted = good;
    shifted.field.start = tick(shifted.field.start.get() + 1);
    shifted.field.updated_at = tick(shifted.field.updated_at.get() + 1);
    shifted.history.as_mut().unwrap().cursor.start =
        tick(shifted.history.unwrap().cursor.start.get() + 1);
    invalid.push(shifted);
    for line in [0, 262, u16::MAX] {
        let mut value = good;
        value.history.as_mut().unwrap().cursor.line = line;
        invalid.push(value);
    }
    for start in [good.field.start.get() - 1, good.field.updated_at.get() + 1] {
        let mut value = good;
        value.history.as_mut().unwrap().cursor.start = tick(start);
        invalid.push(value);
    }
    for delta in [1, 60] {
        let mut value = good;
        value.field.start = tick(value.field.start.get() + delta);
        invalid.push(value);
    }
    for id in [0, 4, u64::MAX] {
        let mut value = good;
        value.field.id = id;
        invalid.push(value);
    }
    for mode in [InterlaceMode::Normal, InterlaceMode::Double] {
        let mut value = good;
        value.field.mode = mode;
        invalid.push(value);
        value = good;
        value.field.active_mode = mode;
        invalid.push(value);
    }
    let mut value = good;
    value.field.odd = true;
    invalid.push(value);
    value = good;
    value.field.active_odd = true;
    invalid.push(value);
    for end in [
        good.field.updated_at.get(),
        good.field.updated_at.get() + 1,
        good.field.updated_at.get() + 3421,
    ] {
        let mut value = good;
        value.history.as_mut().unwrap().pulse_end = Some(tick(end));
        invalid.push(value);
    }
    value = good;
    value.field.updated_at = tick(value.history.unwrap().cursor.start.get() + 3360);
    invalid.push(value);
    value = anchored(Region::NtscUs, 0, 0);
    value.horizontal.epoch = tick(u64::MAX - 100);
    value.field.start = tick(u64::MAX - 100);
    value.field.updated_at = value.field.start;
    value.field.id = 0;
    value.history.as_mut().unwrap().cursor.start = value.field.start;
    invalid.push(value);
    for value in invalid {
        assert!(
            value
                .validate_calendar(value.field.updated_at, Region::NtscUs)
                .is_err(),
            "{value:?}"
        );
    }
    assert!(
        good.validate_calendar(tick(good.field.updated_at.get() + 1), Region::NtscUs)
            .is_err()
    );
}

#[test]
fn pulse_validation_keeps_previous_clock_tail_and_rejects_shifted_onset() {
    for region in [Region::NtscUs, Region::Pal] {
        for line in [225, 241] {
            for offset in [8, 10] {
                let mut timing = anchored(region, line, 1);
                timing.history.as_mut().unwrap().pulse_end =
                    Some(tick(timing.history.unwrap().cursor.start.get() + offset));
                assert_eq!(
                    timing
                        .validate_calendar(timing.field.updated_at, region)
                        .is_ok(),
                    line == 225 || region == Region::Pal
                );
            }
        }
        let mut timing = anchored(region, 224, 18);
        let start = timing.history.unwrap().cursor.start.get();
        timing.history.as_mut().unwrap().pulse_end = Some(tick(start + 3369));
        assert!(
            timing
                .validate_calendar(timing.field.updated_at, region)
                .is_err()
        );
        timing.history.as_mut().unwrap().pulse_end = Some(tick(start + 3368));
        timing
            .validate_calendar(timing.field.updated_at, region)
            .unwrap();
    }
}

#[test]
fn history_register_validation_enforces_hc0_and_progressive_modes() {
    let timing = anchored(Region::NtscUs, 5, 10);
    for size in [0, 1, 0x81] {
        timing
            .validate_register_history(
                &registers(timing.field.updated_at.get(), size, None),
                Region::NtscUs,
            )
            .unwrap();
    }
    for size in [0x80, 3, 5, 7, 0x83, 0x87] {
        assert!(
            timing
                .validate_register_history(
                    &registers(timing.field.updated_at.get(), size, None),
                    Region::NtscUs
                )
                .is_err()
        );
    }
    let hc0 = anchored(Region::NtscUs, 5, 0);
    assert!(
        hc0.validate_register_history(
            &registers(hc0.field.updated_at.get(), 0x81, None),
            Region::NtscUs
        )
        .is_err()
    );
    hc0.validate_register_history(
        &registers(hc0.field.updated_at.get(), 1, None),
        Region::NtscUs,
    )
    .unwrap();
    let legacy = VdpVerticalTiming::at_counters_zero(MasterTicks::ZERO);
    assert!(
        legacy
            .validate_register_history(&registers(0, 1, None), Region::NtscUs)
            .is_err()
    );
    let queued = VdpRegisterWrite {
        index: DISPLAY_SIZE_REGISTER,
        value: 1,
        publish_at: tick(20),
    };
    legacy
        .validate_register_history(&registers(0, 0, Some(queued)), Region::NtscUs)
        .unwrap();
    let invalid_pending = VdpRegisterWrite {
        value: 0x83,
        publish_at: tick((timing.field.updated_at.get() / 20 + 1) * 20),
        ..queued
    };
    assert!(
        timing
            .validate_register_history(
                &registers(timing.field.updated_at.get(), 1, Some(invalid_pending)),
                Region::NtscUs
            )
            .is_err()
    );
}

fn vdp_bytes(vdp: &Vdp, region: Region) -> Vec<u8> {
    let mut writer = StateWriter::new();
    vdp.write_state_in_region(&mut writer, region).unwrap();
    writer.into_bytes()
}

fn history_offset(bytes: &[u8]) -> usize {
    let mut reader = StateReader::new(bytes);
    VdpRegisterPipeline::read_state(&mut reader).unwrap();
    reader.position() + 36
}

fn restored(vdp: &Vdp, region: Region) -> Vdp {
    let bytes = vdp_bytes(vdp, region);
    let mut reader = StateReader::new(&bytes);
    let mut restored = Vdp::bootstrap_mode5();
    restored.read_state(&mut reader, region).unwrap();
    assert!(reader.is_exhausted());
    assert_eq!(restored, *vdp);
    restored
}

fn continuation(vdp: &Vdp, region: Region) {
    let mut original = vdp.clone();
    let mut copy = restored(vdp, region);
    for offset in [1, 20, 3360, 7000, 90_000] {
        let at = tick(vdp.now().get() + offset);
        original.advance_to(at, region).unwrap();
        copy.advance_to(at, region).unwrap();
        assert_eq!(copy, original);
    }
}

#[test]
fn raw19_restores_before_publication_latch_pulse_and_after_return() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut vdp = Vdp::bootstrap_mode5();
        vdp.advance_to(tick(3164), region).unwrap();
        let write = vdp
            .queue_register(DISPLAY_SIZE_REGISTER, 1, region)
            .unwrap();
        continuation(&vdp, region);
        vdp.advance_to(write.publish_at, region).unwrap();
        continuation(&vdp, region);
        vdp.advance_to(tick(3420), region).unwrap();
        continuation(&vdp, region);
        vdp.advance_to(tick(3420 + 223 * 3360 + 18), region)
            .unwrap();
        continuation(&vdp, region);
        let write = vdp
            .queue_register(DISPLAY_SIZE_REGISTER, 0x81, region)
            .unwrap();
        vdp.advance_to(write.publish_at, region).unwrap();
        continuation(&vdp, region);
        vdp.advance_to(tick(vdp.now().get() + 3500), region)
            .unwrap();
        continuation(&vdp, region);
    }
}

#[test]
fn raw19_malformed_history_is_transactional_and_section_exhausted() {
    let region = Region::NtscUs;
    let mut source = Vdp::bootstrap_mode5();
    source.advance_to(tick(1), region).unwrap();
    let write = source
        .queue_register(DISPLAY_SIZE_REGISTER, 1, region)
        .unwrap();
    source
        .advance_to(tick(write.publish_at.get() + 3420), region)
        .unwrap();
    let bytes = vdp_bytes(&source, region);
    let offset = history_offset(&bytes);
    assert_eq!(bytes[offset], 1);
    let mut target = Vdp::bootstrap_mode5();
    let before = target.clone();
    for (relative, value) in [(0, 2), (9, 0xff), (10, 0xff), (11, 3), (12, 2)] {
        let mut bad = bytes.clone();
        bad[offset + relative] = value;
        assert!(
            target
                .read_state(&mut StateReader::new(&bad), region)
                .is_err()
        );
        assert_eq!(target, before);
    }
    for end in offset..offset + 13 {
        assert!(
            target
                .read_state(&mut StateReader::new(&bytes[..end]), region)
                .is_err()
        );
        assert_eq!(target, before);
    }
    let mut writer = StateWriter::new();
    writer.write_section(|writer| {
        writer.write_bytes(&bytes);
        writer.write_u8(0);
    });
    assert!(
        read_section(&mut StateReader::new(&writer.into_bytes()), |reader| {
            let mut candidate = before.clone();
            candidate.read_state(reader, region)
        })
        .is_err()
    );
}
