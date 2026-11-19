use super::*;

pub(super) fn run() -> u64 {
    let mut digest = 0_u64;
    for (region, at, id, line, phase, publication, boundary, end, image) in [
        (
            Region::NtscJapan,
            32268250,
            36,
            3,
            550,
            32268260,
            32271120,
            33138000,
            [
                4, 52, 40, 48, 5, 110, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0,
            ],
        ),
        (
            Region::NtscUs,
            15233526,
            17,
            0,
            846,
            15233540,
            15236100,
            16113060,
            [
                20, 52, 48, 56, 7, 120, 0, 0, 0, 0, 0, 0, 0, 61, 0, 2, 1, 0, 0, 0, 0, 0, 0, 128,
            ],
        ),
        (
            Region::NtscUs,
            3665984,
            4,
            23,
            3164,
            3666000,
            3666240,
            4465920,
            [
                4, 116, 56, 52, 6, 122, 0, 0, 0, 0, 0, 0, 129, 0, 0, 2, 1, 0, 0, 0, 0, 239, 181, 2,
            ],
        ),
        (
            Region::NtscUs,
            52592232,
            58,
            181,
            2892,
            52592240,
            52592760,
            52861560,
            [
                4, 52, 40, 48, 5, 110, 0, 0, 0, 0, 0, 1, 0, 0, 0, 2, 3, 0, 0, 0, 0, 0, 0, 0,
            ],
        ),
        (
            Region::NtscUs,
            587735148,
            655,
            242,
            1308,
            587735160,
            587737260,
            587801100,
            [
                20, 36, 8, 60, 0, 10, 0, 0, 0, 0, 230, 0, 129, 32, 0, 2, 17, 0, 0, 0, 0, 255, 255,
                128,
            ],
        ),
        (
            Region::NtscUs,
            143442817,
            160,
            22,
            1177,
            143442820,
            143445060,
            144248100,
            [
                20, 36, 8, 60, 0, 10, 0, 0, 0, 0, 230, 0, 129, 32, 0, 2, 17, 0, 0, 0, 0, 255, 255,
                128,
            ],
        ),
        (
            Region::NtscUs,
            4088,
            0,
            1,
            668,
            4100,
            6840,
            880440,
            [
                0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
        ),
        (
            Region::NtscUs,
            2524914,
            2,
            214,
            954,
            2524920,
            2527380,
            2685300,
            [
                4, 52, 40, 48, 5, 110, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0,
            ],
        ),
    ] {
        let field_start = at - line * 3420 - phase;
        let mut timing = VdpVerticalTiming::at_counters_zero(ticks(0));
        timing.field = FieldCalendar::new(ticks(field_start));
        timing.field.id = id;
        timing.field.updated_at = ticks(at);
        let mut registers = VdpRegisterPipeline::clock_fixture(ticks(0), ticks(at), image);
        assert_eq!(
            registers
                .queue_clock_fixture(12, 1)
                .unwrap()
                .publish_at
                .get(),
            publication
        );
        let mut p = timing.history_projection(&registers, region, None).unwrap();
        assert_eq!(p.field_end().unwrap().get(), end);
        let sound = HistoryProjection::new(
            p.field,
            p.history,
            SoundRegisters::capture(&registers),
            region,
        )
        .unwrap();
        assert_eq!(sound.field_end().unwrap().get(), end);
        let old_clock = LineClock::from_register(image[12]).unwrap();
        p.advance_to(ticks(publication)).unwrap();
        assert_eq!(p.history.cursor.clock, old_clock);
        assert_eq!(p.registers.size_byte(), Some(1));
        assert_eq!(p.registers.mode_byte(), Some(image[1]));
        p.advance_to(ticks(boundary - 1)).unwrap();
        let event = p.step().unwrap();
        let completed = event.completed.unwrap();
        assert_eq!(completed.at.get(), boundary);
        assert_eq!(completed.field, id);
        assert_eq!(completed.field_start.get(), field_start);
        assert_eq!(u64::from(completed.line), line);
        assert_eq!(completed.line_start.get(), at - phase);
        assert_eq!(completed.clock, old_clock);
        assert!(!completed.wrapped);
        assert_eq!(
            event.slot,
            image[1] & DISPLAY_ENABLE == 0 || line + 1 >= 224
        );
        assert_eq!(p.history.cursor.clock, LineClock::InternalH40);
        assert_eq!(p.field.id, id);
        assert_eq!(p.field.start.get(), field_start);
        assert_eq!(p.field_end().unwrap().get(), end);
        p.advance_to(ticks(end - 1)).unwrap();
        let wrapped = p.step().unwrap().completed.unwrap();
        assert!(wrapped.wrapped);
        assert_eq!(wrapped.field, id);
        assert_eq!(p.field.id, id + 1);
        assert_eq!(p.field.start.get(), end);
        assert_eq!(p.history.cursor.line, 0);
        for byte in image {
            digest = digest.rotate_left(3) ^ u64::from(byte);
        }
        digest ^= end ^ boundary ^ publication;
    }
    digest
}

#[test]
fn all_eight_recorded_register_and_timing_conditions() {
    assert_ne!(run(), 0);
}
