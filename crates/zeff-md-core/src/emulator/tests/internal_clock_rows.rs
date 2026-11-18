use super::*;

#[test]
fn pal_row_delivery_samples_height_after_coincident_publications() {
    for internal in [false, true] {
        for grow in [false, true] {
            let source = machine(&[0x60fe, 0x4e71]);
            let region = Region::Pal;
            let mut source = Emulator::new(
                source.rom_bytes().to_vec(),
                LoadConfig {
                    region,
                    concurrent_sound: false,
                },
            )
            .unwrap();
            if internal {
                source.bus.advance_machine(1).unwrap();
                let write = source.queue_vdp_register(12, 1).unwrap();
                source
                    .bus
                    .advance_machine(write.publish_at.get() - source.bus.now.get())
                    .unwrap();
            }
            if !grow {
                source.bus.advance_machine(1).unwrap();
                let write = source.queue_vdp_register(1, 0x0c).unwrap();
                source
                    .bus
                    .advance_machine(write.publish_at.get() - source.bus.now.get())
                    .unwrap();
            }
            while source.bus.vdp.field_position(region).unwrap().line < 224 {
                let next = source.bus.vdp.next_row_deadline(region).unwrap().unwrap();
                source
                    .bus
                    .advance_machine(next.get() - source.bus.now.get())
                    .unwrap();
            }
            source.bus.advance_machine(101).unwrap();
            source.bus.video = Default::default();
            let line = source.bus.vdp.prepare_raster_line(0, region).unwrap();
            for row in 0..224 {
                source.bus.video.publish_row(row, 0, 240, Ok(line.clone()));
            }
            let pixel = 224 * line.colors().len() * 4 + 3;
            assert_eq!(source.framebuffer()[pixel], 0);
            let write = source
                .queue_vdp_register(1, if grow { 0x0c } else { 4 })
                .unwrap();
            let end = source.bus.vdp.next_row_deadline(region).unwrap().unwrap();
            assert!(write.publish_at < end);
            source
                .bus
                .advance_machine(end.get() - source.bus.now.get())
                .unwrap();
            assert_eq!(source.framebuffer()[pixel], if grow { 255 } else { 0 });
        }
    }
}
