use super::*;
use crate::constants::VDP_LINE_TICKS;

const FIELD: u64 = 1_070_460;

fn mode(machine: &mut Emulator, value: u8) {
    machine.bus.advance_machine(1).unwrap();
    let pending = machine.queue_vdp_register(1, value).unwrap();
    super::vdp_row_delivery::advance(machine, pending.publish_at.get() - machine.bus.now.get())
        .unwrap();
}

fn fixture(concurrent_sound: bool, h40: bool) -> Emulator {
    let mut image = vec![0; 0x200];
    image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    image[0x100..0x104].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
    let mut machine = Emulator::new(
        image,
        LoadConfig {
            region: Region::Pal,
            concurrent_sound,
        },
    )
    .unwrap();
    if h40 {
        let pending = machine.queue_vdp_register(12, 0x81).unwrap();
        super::vdp_row_delivery::advance(&mut machine, pending.publish_at.get() - 56).unwrap();
    }
    mode(&mut machine, 0x4c);
    machine
}

pub fn height_changes_require_complete_fields_with(
    mut advance: impl FnMut(&mut Emulator, u64) -> Result<(), BusError>,
) {
    for concurrent in [false, true] {
        for h40 in [false, true] {
            let mut machine = fixture(concurrent, h40);
            let width = if h40 { 320 } else { 256 };
            let end = FIELD + 100 * VDP_LINE_TICKS;
            let delta = end - machine.bus.now.get();
            advance(&mut machine, delta).unwrap();
            assert_eq!(machine.framebuffer_dimensions(), (width, 240));
            assert!(machine.frame_ready());
            assert_eq!(machine.framebuffer().len(), width * 240 * 4);
            mode(&mut machine, 0x44);
            let delta = FIELD + 101 * VDP_LINE_TICKS - machine.bus.now.get();
            advance(&mut machine, delta).unwrap();
            assert_eq!(machine.framebuffer_dimensions(), (width, 224));
            assert!(!machine.frame_ready());
            let delta = 2 * FIELD - machine.bus.now.get();
            advance(&mut machine, delta).unwrap();
            assert!(!machine.frame_ready());
            let delta = 2 * FIELD + 224 * VDP_LINE_TICKS - machine.bus.now.get();
            advance(&mut machine, delta).unwrap();
            assert!(machine.frame_ready());
            assert_eq!(machine.frame_count(), 3);
            assert_eq!(machine.video_error(), None);
            mode(&mut machine, 0x4c);
            let delta = 3 * FIELD + 240 * VDP_LINE_TICKS - machine.bus.now.get();
            advance(&mut machine, delta).unwrap();
            assert_eq!(machine.framebuffer_dimensions(), (width, 240));
            assert!(machine.frame_ready());
            assert_eq!(machine.frame_count(), 4);
        }
    }
}

pub fn height_changes_require_complete_fields() {
    height_changes_require_complete_fields_with(super::vdp_row_delivery::advance);
}

pub fn height_and_region_rollback() {
    for concurrent in [false, true] {
        let mut machine = fixture(concurrent, true);
        let delta = FIELD + 100 * VDP_LINE_TICKS - 1 - machine.bus.now.get();
        super::vdp_row_delivery::advance(&mut machine, delta).unwrap();
        machine
            .queue_vdp_capture(MasterTicks::new(machine.bus.now.get() + 1))
            .unwrap();
        machine.bus.cartridge.initial.pc = 0xc0000e;
        let before = machine.clone();
        let pointer = machine.bus.video.framebuffer().as_ptr();
        assert!(machine.reset_cpu().is_err());
        assert_eq!(machine, before);
        assert_eq!(machine.bus.video.framebuffer().as_ptr(), pointer);
        machine.bus.cartridge.initial.pc = 0x100;
        machine.reset_cpu().unwrap();
        assert_eq!(machine.framebuffer_dimensions(), (320, 240));
        assert_eq!(machine.region(), Region::Pal);
        assert_eq!(
            machine.timing_snapshot().rate().numerator_hz(),
            Region::Pal.master_hz()
        );
        Reset::reset(&mut machine);
        assert_eq!(machine.framebuffer_dimensions(), (256, 224));
        assert_eq!(machine.region(), Region::Pal);
        assert_eq!(machine.bus.video.framebuffer().as_ptr(), pointer);
        assert_eq!(machine.take_vdp_capture(), None);
    }
}

#[test]
fn complete_height_fields() {
    height_changes_require_complete_fields();
}

#[test]
fn region_rollback() {
    height_and_region_rollback();
}
