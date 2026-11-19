use super::*;

fn field(start: u64, id: u64, at: u64) -> FieldCalendar {
    let mut field = FieldCalendar::new(MasterTicks::new(start));
    field.id = id;
    field.updated_at = MasterTicks::new(at);
    field
}

pub(super) fn run() -> u64 {
    let mut digest = 0_u64;
    for (region, tick, id, line, phase, r12, boundary, stable_end) in [
        (
            Region::NtscJapan,
            32268250,
            36,
            3,
            550,
            0,
            32271120,
            33138000,
        ),
        (Region::NtscUs, 15233526, 17, 0, 846, 0, 15236100, 16113060),
        (Region::NtscUs, 3665984, 4, 23, 3164, 0x81, 3666240, 4465920),
        (
            Region::NtscUs,
            52592232,
            58,
            181,
            2892,
            0,
            52592760,
            52861560,
        ),
        (
            Region::NtscUs,
            587735148,
            655,
            242,
            1308,
            0x81,
            587737260,
            587801100,
        ),
        (
            Region::NtscUs,
            143442817,
            160,
            22,
            1177,
            0x81,
            143445060,
            144248100,
        ),
        (Region::NtscUs, 4088, 0, 1, 668, 0, 6840, 880440),
        (Region::NtscUs, 2524914, 2, 214, 954, 0, 2527380, 2685300),
    ] {
        let start = tick - line * 3420 - phase;
        let mut calendar = field(start, id, tick);
        let old_clock = LineClock::from_register(r12).unwrap();
        let mut cursor =
            LineCursor::from_legacy(calendar, MasterTicks::new(tick), region, old_clock).unwrap();
        assert_eq!(u64::from(cursor.line), line);
        assert_eq!(cursor.phase(MasterTicks::new(tick)).unwrap(), phase);
        assert_eq!(cursor.next_boundary().unwrap().get(), boundary);
        let old = cursor;
        let completed = cursor
            .finish_line(&mut calendar, region, LineClock::InternalH40)
            .unwrap();
        assert_eq!(completed.at.get(), boundary);
        assert_eq!(completed.field, id);
        assert_eq!(completed.field_start.get(), start);
        assert_eq!(completed.line_start, old.start);
        assert_eq!(completed.line, old.line);
        assert_eq!(completed.clock, old_clock);
        assert!(!completed.wrapped);
        assert_eq!(cursor.phase(calendar.updated_at).unwrap(), 0);
        assert_eq!(
            cursor
                .stable_field_end(calendar, calendar.updated_at, region)
                .unwrap()
                .get(),
            stable_end
        );
        digest = digest.rotate_left(7) ^ boundary ^ stable_end;
    }
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut calendar = field(37, 655, 37);
        let mut cursor =
            LineCursor::from_legacy(calendar, calendar.updated_at, region, LineClock::H32).unwrap();
        let mut expected_start = 37;
        let mut expected_field_start = 37;
        for row in 0..2 * region.frame_lines() + 3 {
            let clock = if row < 64 {
                LineClock::InternalH40
            } else {
                LineClock::ExternalH40
            };
            let period = if row == 0 || row > 64 { 3420 } else { 3360 };
            assert_eq!(cursor.clock.ticks(), period);
            for phase in [0, 1, 7, 8, 91, 92, 2639, 2640, 2867, 2868, 3359] {
                let at = MasterTicks::new(expected_start + phase);
                let mut query = calendar;
                query.updated_at = at;
                cursor.validate(query, at, region).unwrap();
                assert_eq!(cursor.phase(at).unwrap(), phase);
                let end =
                    expected_start + (region.frame_lines() - row % region.frame_lines()) * period;
                assert_eq!(
                    cursor.stable_field_end(query, at, region).unwrap().get(),
                    end
                );
            }
            expected_start += period;
            let completed = cursor.finish_line(&mut calendar, region, clock).unwrap();
            assert_eq!(completed.at.get(), expected_start);
            assert_eq!(completed.field, 655 + row / region.frame_lines());
            assert_eq!(u64::from(completed.line), row % region.frame_lines());
            assert_eq!(completed.field_start.get(), expected_field_start);
            if (row + 1).is_multiple_of(region.frame_lines()) {
                expected_field_start = expected_start;
                assert!(completed.wrapped);
            } else {
                assert!(!completed.wrapped);
            }
            assert_eq!(calendar.start.get(), expected_field_start);
            assert_eq!(calendar.id, 655 + (row + 1) / region.frame_lines());
            assert_eq!(u64::from(cursor.line), (row + 1) % region.frame_lines());
            assert_eq!(cursor.start.get(), expected_start);
            digest = digest.rotate_left(3) ^ calendar.id ^ expected_start;
        }
        assert!(!(expected_field_start - 37).is_multiple_of(3420));
    }
    let region = Region::NtscUs;
    let mut calendar = field(0, 0, 0);
    let cursor =
        LineCursor::from_legacy(calendar, MasterTicks::ZERO, region, LineClock::H32).unwrap();
    assert!(
        LineCursor::from_legacy(calendar, MasterTicks::ZERO, region, LineClock::InternalH40)
            .is_err()
    );
    for malformed in [
        LineCursor {
            start: MasterTicks::new(1),
            ..cursor
        },
        LineCursor { line: 1, ..cursor },
        LineCursor {
            line: 262,
            ..cursor
        },
    ] {
        assert!(
            malformed
                .validate(calendar, malformed.start, region)
                .is_err()
        );
    }
    assert!(cursor.phase(MasterTicks::new(3420)).is_err());
    for start in [3360, 3361, 3419, 3420] {
        let calendar = field(0, 0, start);
        let cursor = LineCursor {
            start: MasterTicks::new(start),
            line: 1,
            clock: LineClock::InternalH40,
        };
        assert_eq!(
            cursor
                .validate(calendar, calendar.updated_at, region)
                .is_ok(),
            start == 3360 || start == 3420
        );
    }
    calendar.active_mode = InterlaceMode::Normal;
    assert!(
        LineCursor::from_legacy(calendar, calendar.updated_at, region, LineClock::H32).is_err()
    );
    for (start, id, line) in [
        (u64::MAX - 100, 0, 0),
        (u64::MAX - 3420, 0, 0),
        (0, u64::MAX, 261),
    ] {
        let tick = start + u64::from(line) * 3420;
        let mut calendar = field(start, id, tick);
        let mut cursor = LineCursor {
            start: MasterTicks::new(tick),
            line,
            clock: LineClock::H32,
        };
        let before = (cursor, calendar);
        assert_eq!(
            cursor.finish_line(&mut calendar, region, LineClock::InternalH40),
            Err(VdpTimingError::ClockOverflow)
        );
        assert_eq!((cursor, calendar), before);
    }
    digest
}

#[test]
fn anchored_progressive_lines_preserve_prefix_and_boundary_identity() {
    assert_ne!(run(), 0);
}
