use super::*;
use crate::{Vdp, VdpRenderError};

fn seeded(width: usize, height: usize) -> RowDelivery {
    let mut video = RowDelivery::default();
    for (index, byte) in video.framebuffer.iter_mut().enumerate() {
        *byte = index.wrapping_mul(29).wrapping_add(17) as u8;
    }
    video.width = width;
    video.height = height;
    video.rows = height / 2;
    video.frame_count = 7;
    video.frame_ready = true;
    video
}

fn encoded(video: &RowDelivery) -> Vec<u8> {
    let mut writer = StateWriter::new();
    video.write_state(&mut writer).unwrap();
    writer.into_bytes()
}

fn raster(width: usize, height: usize, region: Region) -> Vdp {
    let mut vdp = Vdp::bootstrap_mode5();
    for (index, value) in [
        (12, if width == VDP_FRAME_WIDTH { 0x81 } else { 0 }),
        (
            1,
            if height == VDP_MAX_FRAME_HEIGHT {
                0x0c
            } else {
                4
            },
        ),
    ] {
        vdp.write_control(0x8000 | (index << 8) | value, region)
            .unwrap();
        vdp.advance_to(MasterTicks::new(vdp.now().get() + 20), region)
            .unwrap();
    }
    vdp
}

#[test]
fn all_geometries_preserve_partial_frame_and_unused_backing() {
    for (region, width, height) in [
        (Region::NtscUs, VDP_H32_FRAME_WIDTH, VDP_FRAME_HEIGHT),
        (Region::NtscUs, VDP_FRAME_WIDTH, VDP_FRAME_HEIGHT),
        (Region::Pal, VDP_H32_FRAME_WIDTH, VDP_MAX_FRAME_HEIGHT),
        (Region::Pal, VDP_FRAME_WIDTH, VDP_MAX_FRAME_HEIGHT),
    ] {
        let mut source = seeded(width, height);
        let bytes = encoded(&source);
        let mut restored = RowDelivery::default();
        let mut reader = StateReader::new(&bytes);
        restored.read_state(&mut reader, region).unwrap();
        assert!(reader.is_exhausted());
        assert_eq!(restored, source);
        let vdp = raster(width, height, region);
        for row in height / 2..height {
            let line = vdp.prepare_raster_line(row as u16, region).unwrap();
            source.publish_row(row, 8, height, Ok(line.clone()));
            restored.publish_row(row, 8, height, Ok(line));
        }
        assert_eq!(restored, source);
        assert_eq!(restored.frame_count(), 9);
        assert!(restored.frame_ready());
        assert_eq!(restored.rows, height);
        assert!(
            restored.framebuffer()[width * height / 2 * RGBA_CHANNELS..]
                .as_chunks::<RGBA_CHANNELS>()
                .0
                .iter()
                .all(|pixel| *pixel == [0, 0, 0, 255])
        );
        assert_eq!(encoded(&restored), encoded(&source));
    }
}

#[test]
fn geometry_boolean_and_late_framebuffer_errors_are_atomic() {
    let bytes = encoded(&seeded(VDP_FRAME_WIDTH, VDP_MAX_FRAME_HEIGHT));
    let mut target = seeded(VDP_H32_FRAME_WIDTH, VDP_FRAME_HEIGHT);
    target
        .queue(MasterTicks::new(9), MasterTicks::new(8))
        .unwrap();
    let before = target.clone();
    let mut malformed = Vec::new();
    for (offset, value) in [(0, 0), (2, 225), (4, 241)] {
        let mut bad = bytes.clone();
        bad[offset..offset + 2].copy_from_slice(&u16::to_le_bytes(value));
        malformed.push(bad);
    }
    let mut bad = bytes.clone();
    bad[14] = 2;
    malformed.push(bad);
    for end in [
        0,
        1,
        5,
        13,
        14,
        15,
        bytes.len() / 2,
        bytes.len() - 4,
        bytes.len() - 1,
    ] {
        malformed.push(bytes[..end].to_vec());
    }
    for bad in malformed {
        assert!(
            target
                .read_state(&mut StateReader::new(&bad), Region::Pal)
                .is_err()
        );
        assert_eq!(target, before);
    }
    assert!(
        target
            .read_state(&mut StateReader::new(&bytes), Region::NtscUs)
            .is_err()
    );
    assert_eq!(target, before);
}

#[test]
fn diagnostic_capture_and_raster_errors_reject_before_writing() {
    let original = seeded(VDP_H32_FRAME_WIDTH, VDP_FRAME_HEIGHT);
    let error = VdpLineError::Render(VdpRenderError::PendingLineInputs);
    for kind in 0..3 {
        let mut video = original.clone();
        match kind {
            0 => video
                .queue(MasterTicks::new(9), MasterTicks::new(8))
                .unwrap(),
            1 => video.complete(Err(error)),
            _ => video.raster_error = Some(error),
        }
        let before = video.clone();
        let mut writer = StateWriter::new();
        assert!(video.write_state(&mut writer).is_err());
        assert_eq!(writer.position(), 0);
        assert_eq!(video, before);
    }
}

#[test]
fn interlace_seeds_weaves_and_discards_skipped_or_changed_geometry() {
    fn line(width: usize, color: u16) -> VdpScanline {
        let mut vdp = raster(width, VDP_FRAME_HEIGHT, Region::NtscUs);
        vdp.write_control(0xc000, Region::NtscUs).unwrap();
        vdp.write_control(0, Region::NtscUs).unwrap();
        vdp.write_data(color, Region::NtscUs).unwrap();
        vdp.advance_to(MasterTicks::new(vdp.now().get() + 1000), Region::NtscUs)
            .unwrap();
        vdp.prepare_raster_line(0, Region::NtscUs).unwrap()
    }
    let mut video = RowDelivery::default();
    for (id, odd, width, color, expected_even, expected_odd) in [
        (0, false, 320, 0x000e, [255, 0, 0, 255], [255, 0, 0, 255]),
        (1, true, 320, 0x0e00, [255, 0, 0, 255], [0, 0, 255, 255]),
        (4, false, 320, 0x00e0, [0, 255, 0, 255], [0, 255, 0, 255]),
        (5, true, 256, 0x000e, [255, 0, 0, 255], [255, 0, 0, 255]),
    ] {
        let row = line(width, color);
        let field = RasterField {
            id,
            height: 224,
            interlaced: true,
            odd,
        };
        for y in 0..224 {
            video.publish_field_row(y, field, Ok(row.clone()));
        }
        assert_eq!(video.dimensions(), (width, 448));
        assert!(video.frame_ready());
        assert_eq!(video.frame_count(), id + 1);
        for y in 0..448 {
            let expected = if y % 2 == 0 {
                expected_even
            } else {
                expected_odd
            };
            assert!(
                video.framebuffer()[y * width * 4..(y + 1) * width * 4]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| *pixel == expected)
            );
        }
        let mut restored = RowDelivery::default();
        restored
            .read_state(&mut StateReader::new(&encoded(&video)), Region::NtscUs)
            .unwrap();
        assert!(restored == video);
        video.clear_frame_ready();
    }
    let checkpoint = video.checkpoint();
    let before = video.clone();
    video.reset();
    video.restore(checkpoint);
    assert!(video == before);
    let row = line(320, 0x0e00);
    video.publish_field_row(
        0,
        RasterField {
            id: 6,
            height: 224,
            interlaced: false,
            odd: false,
        },
        Ok(row),
    );
    assert_eq!(video.dimensions(), (320, 224));
    assert_eq!(video.valid_fields, 0);
    assert!(!video.frame_ready());
}
