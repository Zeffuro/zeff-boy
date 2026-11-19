use super::*;

#[test]
fn normal_interlace_mode_change_seeds_new_pair_and_validates_delivery_tag() {
    let mut video = RowDelivery::default();
    for (id, mode, odd, color) in [
        (0, InterlaceMode::Double, false, 0x000e),
        (1, InterlaceMode::Normal, true, 0x0e00),
    ] {
        for row in 0..224 {
            video.publish_field_row(
                row,
                RasterField {
                    id,
                    height: 224,
                    mode,
                    odd,
                },
                Ok(line(320, color)),
            );
        }
    }
    assert_eq!(video.valid_fields, 2);
    assert!(
        video
            .framebuffer()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [0, 0, 255, 255])
    );
    let bytes = encoded(&video);
    let tag = 15 + FRAMEBUFFER_LEN + 1 + 8 + 2;
    assert_eq!(bytes[tag], 2);
    let mut restored = RowDelivery::default();
    restored
        .read_state(&mut StateReader::new(&bytes), Region::NtscUs)
        .unwrap();
    assert_eq!(restored, video);
    for value in [3, 255] {
        let mut malformed = bytes.clone();
        malformed[tag] = value;
        let mut target = video.clone();
        assert!(
            target
                .read_state(&mut StateReader::new(&malformed), Region::NtscUs)
                .is_err()
        );
        assert_eq!(target, video);
    }
    let mut position = crate::hardware::vdp::FieldPosition {
        id: 1,
        start: MasterTicks::ZERO,
        lines: 262,
        line: 224,
        line_start: MasterTicks::new(224 * 3420),
        line_ticks: 3420,
        projected_end: Some(MasterTicks::new(262 * 3420)),
        mode: InterlaceMode::Normal,
        odd: false,
        active_mode: InterlaceMode::Double,
        active_odd: true,
    };
    assert!(video.validate_calendar(position).is_err());
    position.active_mode = InterlaceMode::Normal;
    video.validate_calendar(position).unwrap();
}
