use super::*;

fn ticks(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}

fn fixture(r12: u8, r1: u8, region: Region, now: u64) -> HistoryProjection<VdpRegisterPipeline> {
    let mut image = [0; 24];
    image[1] = r1;
    image[12] = r12;
    let registers = VdpRegisterPipeline::clock_fixture(MasterTicks::ZERO, ticks(now), image);
    let clock = LineClock::from_register(r12).unwrap();
    let mut field = FieldCalendar::new(MasterTicks::ZERO);
    field.updated_at = ticks(now);
    let history = if clock == LineClock::InternalH40 {
        TimingHistory {
            cursor: LineCursor {
                start: ticks(now - now % 3360),
                line: (now / 3360) as u16,
                clock,
            },
            pulse_end: None,
        }
    } else {
        TimingHistory::from_legacy(field, region, &registers).unwrap()
    };
    HistoryProjection::new(field, history, registers, region).unwrap()
}

fn queue(p: &mut HistoryProjection<VdpRegisterPipeline>, index: u8, value: u8) -> u64 {
    p.registers
        .queue_clock_fixture(index, value)
        .unwrap()
        .publish_at
        .get()
}

fn transitions() -> u64 {
    let mut digest = 0_u64;
    let stable = fixture(1, 4, Region::NtscUs, 0);
    assert_eq!(stable.field_end().unwrap().get(), 880320);
    for old in [0, 0x81] {
        let mut p = fixture(old, 4, Region::NtscUs, 100);
        assert_eq!(queue(&mut p, 12, 1), 120);
        let before = p;
        assert_eq!(p.field_end().unwrap().get(), 3420 + 261 * 3360);
        assert_eq!(p, before);
        p.advance_to(ticks(120)).unwrap();
        assert_eq!(p.registers.size_byte(), Some(1));
        assert_eq!(
            p.history.cursor.clock,
            LineClock::from_register(old).unwrap()
        );
        p.advance_to(ticks(3419)).unwrap();
        assert_eq!(p.next_slot().unwrap().get(), 3420);
        let event = p.step().unwrap();
        let row = event.completed.unwrap();
        assert_eq!(event.at.get(), 3420);
        assert_eq!(row.clock, LineClock::from_register(old).unwrap());
        assert_eq!((row.line, row.field, row.field_start.get()), (0, 0, 0));
        assert!(event.slot);
        assert_eq!(p.history.cursor.clock, LineClock::InternalH40);
        let position = p.position().unwrap();
        assert_eq!(position.phase, 0);
        assert_eq!(position.line, 1);
        assert_eq!(position.id, 0);
        assert_eq!(position.field_start.get(), 0);
        assert_eq!(position.lines, 262);
        assert_eq!(position.line_start.get(), 3420);
        assert_eq!(position.clock, LineClock::InternalH40);
        assert_eq!(p.next_slot().unwrap().get(), 3436);
        assert!(p.next_event().unwrap() > event.at);
        for returned in [0, 0x81] {
            let mut returned_p = p.project(ticks(3500)).unwrap();
            queue(&mut returned_p, 12, returned);
            returned_p.advance_to(ticks(6780)).unwrap();
            assert_eq!(returned_p.history.cursor.start.get(), 6780);
            assert_eq!(
                returned_p.history.cursor.clock,
                LineClock::from_register(returned).unwrap()
            );
            assert_eq!(returned_p.position().unwrap().phase, 0);
            assert_eq!(returned_p.field.start.get(), 0);
            assert_eq!(
                returned_p.history.cursor.next_boundary().unwrap().get(),
                10200
            );
            digest = digest.rotate_left(5) ^ returned_p.field_end().unwrap().get();
        }
        for replacement in [0, 0x81, 1, 9] {
            let mut cancelled = before.project(ticks(120)).unwrap();
            assert_eq!(queue(&mut cancelled, 12, replacement), 140);
            cancelled.advance_to(ticks(3420)).unwrap();
            assert_eq!(
                cancelled.history.cursor.clock,
                LineClock::from_register(replacement).unwrap()
            );
            digest ^= cancelled.next_slot().unwrap().get();
        }
    }
    for (queued_at, published, latch) in
        [(3381, 3400, 3420), (3401, 3420, 3420), (3421, 3440, 6840)]
    {
        let mut p = fixture(0, 4, Region::NtscUs, queued_at);
        assert_eq!(queue(&mut p, 12, 1), published);
        p.advance_to(ticks(latch - 1)).unwrap();
        let event = p.step().unwrap();
        assert_eq!(event.at.get(), latch);
        assert_eq!(event.publication, published == latch);
        assert!(event.completed.is_some());
        assert!(event.slot);
        assert_eq!(p.history.cursor.clock, LineClock::InternalH40);
        digest = digest.rotate_left(3) ^ event.at.get();
    }
    let mut p = fixture(1, 4, Region::NtscUs, 100);
    queue(&mut p, 12, 9);
    p.advance_to(ticks(3360)).unwrap();
    assert_eq!(p.history.cursor.clock, LineClock::InternalH40);
    assert_eq!(p.field_end().unwrap().get(), 262 * 3360);
    digest
}

fn pulses() -> u64 {
    let mut digest = 0_u64;
    for (region, height, r1) in [
        (Region::NtscUs, 224, 4),
        (Region::NtscJapan, 224, 4),
        (Region::Pal, 224, 4),
        (Region::Pal, 240, 12),
    ] {
        for (old, next, period, vint) in [
            (0, 1, 3420, 10),
            (0x81, 1, 3420, 8),
            (1, 0, 3360, 8),
            (1, 0x81, 3360, 8),
        ] {
            let onset = height * period + vint;
            let mut p = fixture(old, r1, region, onset - 1);
            let snapshot = p;
            assert!(!p.sound_now());
            let event = p.step().unwrap();
            assert_eq!(event.at.get(), onset);
            assert!(event.vertical);
            assert!(p.sound_now());
            assert_eq!(p.history.pulse_end.unwrap().get(), onset + period);
            p.advance_to(ticks(onset + 100)).unwrap();
            queue(&mut p, 12, next);
            p.advance_to(ticks(onset + period - 1)).unwrap();
            assert!(p.sound_now());
            assert_eq!(
                p.history.cursor.clock,
                LineClock::from_register(next).unwrap()
            );
            let ended = p.step().unwrap();
            assert_eq!(ended.at.get(), onset + period);
            assert!(ended.pulse_ended);
            assert!(!p.sound_now());
            let end = snapshot.project(ticks(onset + period)).unwrap();
            assert!(!end.sound_now());
            assert!(!snapshot.sound_now());
            let sound_registers = SoundRegisters::capture(&snapshot.registers);
            let sound =
                HistoryProjection::new(snapshot.field, snapshot.history, sound_registers, region)
                    .unwrap();
            assert!(sound.project(ticks(onset)).unwrap().sound_now());
            assert!(!sound.project(ticks(onset + period)).unwrap().sound_now());
            assert!(sound.project(ticks(onset - 2)).is_err());
            digest = digest.rotate_left(7) ^ ended.at.get();
        }
        let onset = height * 3420 + 10;
        let mut seeded = fixture(0, r1, region, onset + 100);
        assert_eq!(seeded.history.pulse_end.unwrap().get(), onset + 3420);
        queue(&mut seeded, 12, 1);
        assert!(seeded.project(ticks(onset + 3419)).unwrap().sound_now());
        assert!(!seeded.project(ticks(onset + 3420)).unwrap().sound_now());
    }
    let onset = 224 * 3420 + 10;
    let mut p = fixture(0, 4, Region::Pal, onset - 1);
    let mut image = [0; 24];
    image[1] = 4;
    p.registers = VdpRegisterPipeline::clock_fixture(ticks(10), ticks(onset - 1), image);
    assert_eq!(queue(&mut p, 1, 12), onset);
    let event = p.step().unwrap();
    assert!(event.vertical && event.publication);
    assert_eq!(p.registers.mode_byte(), Some(12));
    assert_eq!(p.history.pulse_end.unwrap().get(), onset + 3420);
    let mut p = fixture(1, 4, Region::Pal, 224 * 3360 + 1);
    queue(&mut p, 1, 12);
    let event = p.step().unwrap();
    assert!(event.vertical);
    assert!(!event.publication);
    digest
}

fn query_coverage() -> u64 {
    let mut digest = 0_u64;
    for (region, height, r1) in [
        (Region::NtscUs, 224, 4),
        (Region::NtscJapan, 224, 4),
        (Region::Pal, 224, 4),
        (Region::Pal, 240, 12),
    ] {
        let p = fixture(1, r1, region, 0);
        assert_eq!(p.field_end().unwrap().get(), region.frame_lines() * 3360);
        assert_eq!(
            p.next_hblank().unwrap(),
            VdpHBlankEdge {
                at: ticks(92),
                active: false
            }
        );
        for (phase, counter, hb) in [
            (0, 0, true),
            (7, 0, true),
            (8, 1, true),
            (91, 11, true),
            (92, 11, false),
            (2867, 358, false),
            (2868, 358, true),
            (2919, 0x16c, true),
            (2920, 0x1c9, true),
            (3359, 0x1ff, true),
        ] {
            let at = p.project(ticks(phase)).unwrap();
            assert_eq!(at.hcounter_now().unwrap(), counter);
            assert_eq!(at.hblank_now().unwrap(), hb);
            assert_eq!(at.hv_counter_now().unwrap() & 0xff, counter >> 1);
        }
        for active in [false, true] {
            let mut slots = fixture(1, r1 | if active { 0x40 } else { 0 }, region, 0);
            let mut count = usize::from(slots.slot_now().unwrap());
            let mut last = 0;
            loop {
                let at = slots.next_slot().unwrap();
                assert!(at > slots.field.updated_at);
                if at.get() >= 3360 {
                    break;
                }
                slots.advance_to(at).unwrap();
                assert!(slots.slot_now().unwrap());
                count += 1;
                last = at.get();
            }
            assert_eq!(count, if active { 18 } else { 204 });
            if active {
                assert_eq!(last, 2960);
            }
            digest ^= last;
        }
        let blank = (height - 1) * 3360 + 2640;
        let clear = (region.frame_lines() - 2) * 3360 + 2640;
        assert_eq!(
            p.next_vblank().unwrap(),
            VdpVBlankEdge {
                at: ticks(blank),
                active: true
            }
        );
        let before_blank = p.project(ticks(blank - 1)).unwrap();
        assert!(!before_blank.vblank_now().unwrap());
        assert_eq!(before_blank.next_interrupt().unwrap().at.get(), blank);
        let at_blank = p.project(ticks(blank)).unwrap();
        assert!(at_blank.vblank_now().unwrap());
        assert_eq!(at_blank.vcounter_now().unwrap(), height as u16);
        assert_eq!(
            at_blank.next_vblank().unwrap(),
            VdpVBlankEdge {
                at: ticks(clear),
                active: false
            }
        );
        assert!(!p.project(ticks(clear)).unwrap().vblank_now().unwrap());
        let mut mixed = fixture(0x81, r1, region, 100);
        queue(&mut mixed, 12, 1);
        let horizon = 3420 + (region.frame_lines() * 2 + 3) * 3360 + 111;
        let direct = mixed.project(ticks(horizon)).unwrap();
        let mut chunked = mixed;
        let mut tick = 100;
        while tick < horizon {
            tick = (tick + [1, 19, 71, 3359, 3421][(tick as usize) % 5]).min(horizon);
            chunked.advance_to(ticks(tick)).unwrap();
        }
        assert_eq!(chunked, direct);
        assert_eq!(direct.field.id, 2);
        assert_ne!(direct.field.start.get() % 3420, 0);
        assert!(direct.project(ticks(0)).is_err());
        digest = digest.rotate_left(11) ^ direct.field.start.get();
    }
    let mut p = fixture(0, 4, Region::NtscUs, 2641);
    assert_eq!(queue(&mut p, 12, 1), 2660);
    let event = p.step().unwrap();
    assert!(event.horizontal && event.publication);
    assert_eq!(p.history.cursor.clock, LineClock::H32);
    assert_eq!(p.next_interrupt().unwrap().at.get(), 3420 + 2640);
    let mut p = fixture(1, 4, Region::NtscUs, 100);
    queue(&mut p, 1, 0x44);
    assert_eq!(p.next_slot().unwrap().get(), 112);
    p.advance_to(ticks(119)).unwrap();
    assert_eq!(p.next_slot().unwrap().get(), 144);
    digest
}

fn rejections() {
    let p = fixture(0, 4, Region::NtscUs, 100);
    let mut reverse = p;
    assert!(reverse.advance_to(ticks(99)).is_err());
    assert_eq!(reverse, p);
    for (r12, r1, region) in [
        (3, 4, Region::NtscUs),
        (2, 4, Region::NtscUs),
        (0x80, 4, Region::NtscUs),
        (0, 12, Region::NtscUs),
        (0, 0x84, Region::Pal),
    ] {
        let mut image = [0; 24];
        image[1] = r1;
        image[12] = r12;
        let registers = VdpRegisterPipeline::clock_fixture(ticks(0), ticks(100), image);
        assert!(HistoryProjection::new(p.field, p.history, registers, region).is_err());
    }
    for (mode, active) in [
        (InterlaceMode::Normal, InterlaceMode::Progressive),
        (InterlaceMode::Progressive, InterlaceMode::Double),
    ] {
        let mut field = p.field;
        field.mode = mode;
        field.active_mode = active;
        assert!(HistoryProjection::new(field, p.history, p.registers, p.region).is_err());
    }
    for deadline in [100, 3521, u64::MAX] {
        let mut history = p.history;
        history.pulse_end = Some(ticks(deadline));
        assert!(HistoryProjection::new(p.field, history, p.registers, p.region).is_err());
    }
    let mut malformed = p.history;
    malformed.cursor.line = 1;
    assert!(HistoryProjection::new(p.field, malformed, p.registers, p.region).is_err());
    let mut invalid_pending = p;
    queue(&mut invalid_pending, 12, 2);
    let before = invalid_pending;
    assert!(invalid_pending.advance_to(ticks(4000)).is_err());
    assert_eq!(invalid_pending, before);
    let mut image = [0; 24];
    image[1] = 4;
    let start = u64::MAX - 3420;
    let field = FieldCalendar::new(ticks(start));
    let registers = VdpRegisterPipeline::clock_fixture(ticks(start), ticks(start), image);
    let history = TimingHistory {
        cursor: LineCursor {
            start: ticks(start),
            line: 0,
            clock: LineClock::H32,
        },
        pulse_end: None,
    };
    let mut overflow = HistoryProjection::new(field, history, registers, Region::NtscUs).unwrap();
    let before = overflow;
    assert_eq!(
        overflow.advance_to(ticks(u64::MAX)),
        Err(VdpTimingError::ClockOverflow)
    );
    assert_eq!(overflow, before);
    let mut wrap = fixture(0, 4, Region::NtscUs, 261 * 3420);
    wrap.field.id = u64::MAX;
    let before = wrap;
    assert_eq!(
        wrap.advance_to(ticks(262 * 3420)),
        Err(VdpTimingError::ClockOverflow)
    );
    assert_eq!(wrap, before);
}

#[test]
fn publication_latches_and_anchored_return_paths() {
    assert_ne!(transitions(), 0);
}

#[test]
fn finite_sound_pulses_keep_the_onset_period() {
    assert_ne!(pulses(), 0);
}

#[test]
fn forward_queries_share_pending_publication_and_line_boundaries() {
    assert_ne!(query_coverage(), 0);
}

#[test]
fn projection_rejections_preserve_all_input_state() {
    rejections();
}

mod original_cases;
