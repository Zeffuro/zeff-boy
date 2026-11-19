use super::*;

#[path = "blank_mode/memory.rs"]
mod memory;

fn write_at(vdp: &mut Vdp, region: Region, publication: u64, index: u8, value: u8) {
    let begin = (publication - 29) / 7 * 7;
    assert!(begin >= vdp.now().get());
    vdp.advance_to(tick(begin), region).unwrap();
    vdp.begin_timed_control(tick(begin), tick(begin + 28), region)
        .unwrap();
    vdp.advance_to(tick(begin + 28), region).unwrap();
    let mut beyond = vdp.clone();
    assert!(beyond.advance_to(tick(begin + 29), region).is_err());
    assert_eq!(beyond, *vdp);
    let sound = vdp.interrupts().sound_interrupt(vdp.registers(), region);
    assert_eq!(
        sound.asserted_at(begin + 29),
        Err(crate::hardware::vdp::VdpTimingError::UnqualifiedDisplayHistory)
    );
    vdp.write_control(0x8000 | u16::from(index) << 8 | u16::from(value), region)
        .unwrap();
    assert!(vdp.has_timed_control());
    vdp.finish_timed_control(tick(begin + 28), region).unwrap();
    assert!(!vdp.has_timed_control());
    assert_eq!(
        vdp.registers().pending().unwrap().publish_at,
        tick(publication)
    );
    vdp.advance_to(tick(publication), region).unwrap();
}

fn restored(vdp: &Vdp, region: Region) -> Vdp {
    let mut writer = StateWriter::new();
    vdp.write_state_in_region(&mut writer, region).unwrap();
    let bytes = writer.into_bytes();
    let mut other = Vdp::bootstrap_mode5();
    let mut reader = StateReader::new(&bytes);
    other.read_state(&mut reader, region).unwrap();
    assert!(reader.is_exhausted());
    assert_eq!(&other, vdp);
    other
}

fn advance_pair(vdp: &mut Vdp, other: &mut Vdp, region: Region, at: u64) {
    vdp.advance_to(tick(at), region).unwrap();
    other.advance_to(tick(at), region).unwrap();
    assert_eq!(vdp, other);
}

pub(super) fn assert_staged_blank_history() {
    for region in [Region::NtscUs, Region::Pal] {
        for publication in [2640, 2660, 3180, 3200] {
            let mut vdp = Vdp::bootstrap_mode5();
            write_at(&mut vdp, region, publication, 1, 0);
            let mut other = restored(&vdp, region);
            for at in [
                publication + 1,
                publication + 9,
                publication + 10,
                3420,
                3430,
            ] {
                if at < vdp.now().get() {
                    continue;
                }
                advance_pair(&mut vdp, &mut other, region, at);
            }
            let expected = if publication == 2640 {
                1
            } else if publication <= 3180 {
                2
            } else {
                1
            };
            assert_eq!(vdp.read_hv(region).unwrap() >> 8, expected);
            assert!(!vdp.field_presentation_supported());
            let before = vdp.clone();
            assert!(vdp.read_cpu_status(region).is_err());
            assert_eq!(vdp, before);
        }
        let mut zero = Vdp::bootstrap_mode5();
        write_at(&mut zero, region, 100, 1, 0);
        write_at(&mut zero, region, 2800, 1, MODE5_ENABLE);
        zero.advance_to(tick(3430), region).unwrap();
        assert_eq!(zero.read_hv(region).unwrap() >> 8, 0);
        assert!(!zero.field_presentation_supported());
        status_and_sound(region);
        reload_and_recovery(region);
        preactivation_read(region);
        legacy_sample_and_pulse(region);
        history_admission(region);
        memory::checks(region);
    }
}

fn history_admission(region: Region) {
    let mut old = Vdp::bootstrap_mode5();
    old.write_control(0x8c81, region).unwrap();
    old.advance_to(tick(20), region).unwrap();
    old.write_control(0x8c00, region).unwrap();
    old.advance_to(tick(40), region).unwrap();
    assert!(old.state.interrupts.blank_history().is_none());
    let before = old.clone();
    assert!(old.write_control(0x8100, region).is_err());
    assert_eq!(old, before);
    let mut vdp = Vdp::bootstrap_mode5();
    write_at(&mut vdp, region, 100, 1, 0);
    let mut bad = vdp.clone();
    let mut history = bad.state.interrupts.blank_history().unwrap();
    history.core.prepared.v ^= 1;
    bad.state.interrupts.set_blank_history(Some(history));
    let mut writer = StateWriter::new();
    assert!(bad.write_state_in_region(&mut writer, region).is_err());
    assert_eq!(writer.position(), 0);
    let mut writer = StateWriter::new();
    vdp.write_state_in_region(&mut writer, region).unwrap();
    let bytes = writer.into_bytes();
    for length in [bytes.len() - 1, bytes.len() / 2] {
        let before = vdp.clone();
        assert!(
            vdp.read_state(&mut StateReader::new(&bytes[..length]), region)
                .is_err()
        );
        assert_eq!(vdp, before);
    }
}

fn status_and_sound(region: Region) {
    let mut vdp = Vdp::bootstrap_mode5();
    write_at(&mut vdp, region, 100, 0, HINT_ENABLE_MASK);
    write_at(&mut vdp, region, 160, 1, VINT_ENABLE_MASK);
    let onset = 192 * H32_LINE_TICKS + 10;
    let sound = vdp.interrupts().sound_interrupt(vdp.registers(), region);
    assert!(!sound.asserted_at(onset - 1).unwrap());
    assert!(sound.asserted_at(onset).unwrap());
    assert!(sound.asserted_at(onset + 2579).unwrap());
    assert!(!sound.asserted_at(onset + 2580).unwrap());
    vdp.advance_to(tick(onset + 30), region).unwrap();
    assert!(vdp.interrupts().vertical_pending());
    let seed = vdp.clone();
    for phase in 0..10 {
        let mut probe = seed.clone();
        let begin = probe.now().get().div_ceil(7) * 7 + phase * 7;
        probe.advance_to(tick(begin), region).unwrap();
        probe
            .begin_timed_status(tick(begin), tick(begin + 28), region)
            .unwrap();
        probe.advance_to(tick(begin + 28), region).unwrap();
        assert_ne!(
            probe.finish_timed_status(tick(begin + 28), region).unwrap() & VINT_PENDING_STATUS,
            0
        );
        let first = (begin + 7).div_ceil(10) * 10;
        let clear = (first + 20).max((begin + 25).div_ceil(10) * 10) + 10;
        probe.advance_to(tick(clear - 1), region).unwrap();
        assert!(probe.interrupts().vertical_pending());
        probe.advance_to(tick(clear), region).unwrap();
        assert!(!probe.interrupts().vertical_pending());
    }
    assert_eq!(
        vdp.state
            .interrupts
            .complete_acknowledge(&vdp.state.registers)
            .unwrap(),
        VINT_LEVEL
    );
    assert!(vdp.interrupts().vertical_pending());
    let begin = vdp.now().get().div_ceil(7) * 7;
    vdp.advance_to(tick(begin), region).unwrap();
    vdp.begin_timed_status(tick(begin), tick(begin + 28), region)
        .unwrap();
    let mut other = restored(&vdp, region);
    for at in [begin + 6, begin + 7, begin + 18, begin + 25, begin + 28] {
        advance_pair(&mut vdp, &mut other, region, at);
        other = restored(&other, region);
    }
    let word = vdp.finish_timed_status(tick(begin + 28), region).unwrap();
    assert_eq!(
        word,
        other.finish_timed_status(tick(begin + 28), region).unwrap()
    );
    assert_ne!(word & VINT_PENDING_STATUS, 0);
    advance_pair(&mut vdp, &mut other, region, begin + 100);
    assert!(!vdp.interrupts().vertical_pending());
    assert!(
        vdp.interrupts().blank_history().unwrap().core.clear
            == other.interrupts().blank_history().unwrap().core.clear
    );
}

fn reload_and_recovery(region: Region) {
    for (m5, compare, reload, offset) in [
        (
            false,
            if region == Region::Pal { 242 } else { 218 },
            if region == Region::Pal { 442 } else { 469 },
            3200,
        ),
        (
            true,
            if region == Region::Pal { 258 } else { 234 },
            if region == Region::Pal { 458 } else { 485 },
            2660,
        ),
    ] {
        let mut vdp = Vdp::bootstrap_mode5();
        write_at(&mut vdp, region, 100, 1, 0);
        if m5 {
            write_at(&mut vdp, region, 200, 1, MODE5_ENABLE);
        }
        let at = compare * H32_LINE_TICKS + offset;
        vdp.advance_to(tick(at - 1), region).unwrap();
        assert_eq!(
            u64::from(vdp.interrupts().blank_history().unwrap().core.v),
            compare
        );
        let mut other = restored(&vdp, region);
        advance_pair(&mut vdp, &mut other, region, at);
        assert_eq!(vdp.interrupts().blank_history().unwrap().core.v, reload);
    }
    let mut vdp = Vdp::bootstrap_mode5();
    write_at(&mut vdp, region, 100, 1, 0);
    let boundary = region.frame_ticks();
    write_at(&mut vdp, region, boundary, 1, MODE5_ENABLE);
    assert_eq!(vdp.field_count(), 1);
    assert_eq!(vdp.interrupts().blank_history().unwrap().core.v, 0);
    assert!(vdp.field_presentation_supported());
    raster_port_crossings(&vdp, region);
    let mut other = restored(&vdp, region);
    advance_pair(
        &mut vdp,
        &mut other,
        region,
        boundary + region.frame_ticks(),
    );
    assert_eq!(vdp.field_count(), 2);
    assert!(vdp.field_presentation_supported());
}

fn raster_port_crossings(seed: &Vdp, region: Region) {
    for status in [false, true] {
        let mut vdp = seed.clone();
        let row_at = seed.now().get() + H32_LINE_TICKS;
        let begin = (row_at - 14) / 7 * 7;
        let end = begin + 28;
        vdp.advance_to(tick(begin), region).unwrap();
        if status {
            vdp.begin_timed_status(tick(begin), tick(end), region)
                .unwrap();
        } else {
            vdp.begin_timed_control(tick(begin), tick(end), region)
                .unwrap();
        }
        vdp.advance_to(tick(row_at), region).unwrap();
        let before = vdp.clone();
        let line = vdp.prepare_raster_with_horizontal(0, region, None).unwrap();
        assert_eq!(line.colors().len(), H32_SCREEN_WIDTH);
        assert_eq!(vdp, before);
        assert!(vdp.field_presentation_supported());
        assert_eq!(vdp.has_timed_status(), status);
        assert_eq!(vdp.has_timed_control(), !status);
        vdp.advance_to(tick(end), region).unwrap();
        if status {
            assert_ne!(
                vdp.finish_timed_status(tick(end), region).unwrap() & VBLANK_STATUS,
                0
            );
        } else {
            for word in [0x8c81, 0x8144] {
                let before = vdp.clone();
                assert!(vdp.write_control(word, region).is_err());
                assert_eq!(vdp, before);
            }
            vdp.write_control(0x8a00, region).unwrap();
            vdp.finish_timed_control(tick(end), region).unwrap();
        }
        assert!(!vdp.has_timed_status() && !vdp.has_timed_control());
    }
}

fn preactivation_read(region: Region) {
    let mut vdp = Vdp::bootstrap_mode5();
    write_at(&mut vdp, region, 100, 0, HINT_ENABLE_MASK);
    let begin = 3514;
    vdp.advance_to(tick(begin), region).unwrap();
    assert!(vdp.interrupts().horizontal_pending());
    vdp.begin_timed_control(tick(begin), tick(begin + 28), region)
        .unwrap();
    vdp.advance_to(tick(begin + 28), region).unwrap();
    vdp.write_control(0x8100, region).unwrap();
    vdp.finish_timed_control(tick(begin + 28), region).unwrap();
    let read = begin + 28;
    vdp.begin_timed_status(tick(read), tick(read + 28), region)
        .unwrap();
    let mut other = restored(&vdp, region);
    for at in [3549, 3550, 3560, 3567, 3570] {
        advance_pair(&mut vdp, &mut other, region, at);
    }
    assert_eq!(
        vdp.finish_timed_status(tick(3570), region).unwrap(),
        other.finish_timed_status(tick(3570), region).unwrap()
    );
    advance_pair(&mut vdp, &mut other, region, 3579);
    assert!(vdp.interrupts().horizontal_pending());
    advance_pair(&mut vdp, &mut other, region, 3580);
    assert!(!vdp.interrupts().horizontal_pending());
}

fn legacy_sample_and_pulse(region: Region) {
    let mut vdp = Vdp::bootstrap_mode5();
    vdp.advance_to(tick(2912), region).unwrap();
    vdp.begin_timed_status(tick(2912), tick(2940), region)
        .unwrap();
    vdp.advance_to(tick(2940), region).unwrap();
    assert_eq!(
        vdp.interrupts()
            .blank_history()
            .unwrap()
            .port
            .unwrap()
            .captured
            .unwrap()
            & HBLANK_STATUS,
        0
    );
    assert_ne!(
        vdp.finish_timed_status(tick(2940), region).unwrap() & HBLANK_STATUS,
        0
    );
    let mut tail = Vdp::bootstrap_mode5();
    let onset = 224 * H32_LINE_TICKS + 10;
    let publication = 224 * H32_LINE_TICKS + 3000;
    let legacy = tail.interrupts().sound_interrupt(tail.registers(), region);
    assert!(legacy.asserted_at(publication - 1).unwrap());
    assert!(publication > onset + 2580);
    let begin = (publication - 29) / 7 * 7;
    let end = begin + 28;
    tail.advance_to(tick(begin), region).unwrap();
    tail.begin_timed_control(tick(begin), tick(end), region)
        .unwrap();
    tail.advance_to(tick(end), region).unwrap();
    tail.write_control(0x8100, region).unwrap();
    tail.finish_timed_control(tick(end), region).unwrap();
    assert_eq!(
        tail.registers().pending().unwrap().publish_at,
        tick(publication)
    );
    assert!(!tail.interrupts().staged_blank());
    let pending = tail.interrupts().sound_interrupt(tail.registers(), region);
    assert!(pending.asserted_at(publication - 1).unwrap());
    assert!(!pending.asserted_at(publication).unwrap());
    tail.advance_to(tick(publication), region).unwrap();
    let staged = tail.interrupts().sound_interrupt(tail.registers(), region);
    assert!(!staged.asserted_at(publication).unwrap());
}
