use super::*;
use crate::Region;
use crate::hardware::bus::{Bus, BusError};

fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}

fn fixture() -> Bus {
    let mut image = vec![0x4e; 0x200];
    image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    Bus::new(
        crate::cartridge::Cartridge::new(image).unwrap(),
        Region::default(),
    )
}

pub fn late_iack_preserves_video_with(
    mut advance: impl FnMut(&mut Bus, u64, bool) -> Result<(), BusError>,
) {
    let mut bus = fixture();
    bus.vdp.write_control(0x8144, Region::NtscUs).unwrap();
    bus.advance_machine(3419).unwrap();
    bus.vdp.write_control(0x4000, Region::NtscUs).unwrap();
    bus.vdp.write_control(0, Region::NtscUs).unwrap();
    bus.vdp.write_data(0xabcd, Region::NtscUs).unwrap();
    bus.video.queue(tick(3420), bus.now).unwrap();
    let mut ordinary = bus.clone();
    let pointer = bus.video.framebuffer().as_ptr();
    advance(&mut ordinary, 51, false).unwrap();
    advance(&mut bus, 51, true).unwrap();
    assert_eq!(bus, ordinary);
    assert_eq!(bus.now, tick(3470));
    assert_eq!(bus.vdp.now(), bus.now);
    assert_eq!(bus.video.framebuffer().as_ptr(), pointer);
    assert_eq!(bus.video.deadline(), None);
    assert!(bus.video.take().is_some());
    assert_eq!(&bus.vdp.memory().vram()[..2], &[0, 0xcd]);
    assert_eq!(bus.vdp.fifo().len(), 1);
    advance(&mut bus, 160, false).unwrap();
    assert_eq!(bus.now, tick(3630));
    assert_eq!(bus.vdp.now(), bus.now);
    assert_eq!(bus.video.take(), None);
    assert_eq!(&bus.vdp.memory().vram()[..2], &[0xab, 0xcd]);
    assert!(bus.vdp.fifo().is_empty());
}

pub fn capture_row_iack_tie_with(
    mut advance: impl FnMut(&mut Bus, u64, bool) -> Result<(), BusError>,
) {
    let mut bus = fixture();
    bus.video.queue(tick(3420), bus.now).unwrap();
    advance(&mut bus, 3420, true).unwrap();
    assert_eq!(bus.now, tick(3420));
    assert_eq!(bus.vdp.now(), bus.now);
    assert!(matches!(
        bus.video.take(),
        Some(Err(VdpLineError::Schedule(_)))
    ));
    assert_eq!(bus.video.deadline(), None);
    assert_eq!(bus.video.rows, 1);
    assert_eq!(&bus.video.framebuffer()[..4], &[0, 0, 0, 255]);
}

pub fn skipped_fields_and_width_with(
    mut advance: impl FnMut(&mut Bus, u64, bool) -> Result<(), BusError>,
) {
    let mut bus = fixture();
    bus.vdp.write_control(0xc000, Region::NtscUs).unwrap();
    bus.vdp.write_control(0, Region::NtscUs).unwrap();
    bus.vdp.write_data(0x000e, Region::NtscUs).unwrap();
    let period = 896040;
    let pointer = bus.video.framebuffer().as_ptr();
    advance(&mut bus, 5 * period + 6840, false).unwrap();
    assert_eq!(bus.video.frame_count(), 5);
    assert!(bus.video.frame_ready());
    assert_eq!(bus.video.dimensions(), (256, 224));
    assert_eq!(bus.video.rows, 2);
    assert!(
        bus.video
            .framebuffer()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [255, 0, 0, 255])
    );
    bus.vdp.write_control(0x8c81, Region::NtscUs).unwrap();
    advance(&mut bus, period - 6840 + 224 * 3420, false).unwrap();
    assert_eq!(bus.video.frame_count(), 7);
    assert_eq!(bus.video.dimensions(), (320, 224));
    assert_eq!(bus.video.raster_error(), None);
    assert!(
        bus.video
            .framebuffer()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [255, 0, 0, 255])
    );
    assert_eq!(bus.video.framebuffer().as_ptr(), pointer);
}

#[test]
fn rollback() {
    late_iack_preserves_video_with(Bus::advance_machine_cycle);
}
#[test]
fn tie() {
    capture_row_iack_tie_with(Bus::advance_machine_cycle);
}
#[test]
fn skip() {
    skipped_fields_and_width_with(Bus::advance_machine_cycle);
}

#[test]
fn intensity_levels_reach_framebuffer() {
    let ramp = [
        0, 18, 36, 54, 72, 91, 109, 127, 145, 163, 182, 200, 218, 236, 255,
    ];
    for (level, value) in ramp.into_iter().enumerate() {
        for (shift, channel) in [(0, 0), (4, 1), (8, 2)] {
            let mut expected = [0, 0, 0, 255];
            expected[channel] = value;
            assert_eq!(rgba((level as u16) << shift), expected);
        }
    }
    for level in 0..8 {
        let normal = (level * 255 / 7) as u8;
        assert_eq!(rgba(level * 0x222), [normal, normal, normal, 255]);
    }
}
