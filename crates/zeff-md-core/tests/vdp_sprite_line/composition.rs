use super::fixtures::*;

pub fn background_merge_with(
    mut sample: impl FnMut(
        &VdpMemory,
        &VdpRegisterPipeline,
        &VdpSpriteLine,
        u16,
        u16,
    ) -> Result<u16, VdpRenderError>,
) {
    previous::shadow_highlight_with(|memory, bank| {
        let prepared = prepare(memory, bank, 0);
        let line = memory.compose_sprite_line_with_registers(bank, &prepared).unwrap();
        sample(memory, bank, &line, 0, 0)
    });
    previous::sprite_plane_priority_with(|memory, bank| {
        let prepared = prepare(memory, bank, 0);
        let line = memory.compose_sprite_line(&prepared).unwrap();
        sample(memory, bank, &line, 0, 0)
    });
    let mut bank = legacy::bank();
    let mut memory = VdpMemory::default();
    legacy::palette(&mut memory);
    legacy::solid(&mut memory, 3, 3);
    legacy::at(&mut memory, 0, (0, 0), 0, 0, 0x8003);
    let prepared = prepare(&memory, &bank, 0);
    let line = memory.compose_sprite_line(&prepared).unwrap();
    assert_eq!(
        sample(&memory, &bank, &line, 0, 0),
        Ok(legacy::cram_word(3))
    );
    legacy::write(&mut memory, 6, 3, 0x246);
    assert_eq!(sample(&memory, &bank, &line, 0, 0), Ok(0x246));
    assert_eq!(
        sample(&memory, &bank, &line, 8, 0),
        Err(VdpRenderError::UnknownRegister(7))
    );
    legacy::publish(&mut bank, 7, 63);
    assert_eq!(
        sample(&memory, &bank, &line, 8, 0),
        Ok(legacy::cram_word(63))
    );
    legacy::solid(&mut memory, 1, 1);
    legacy::write(&mut memory, 0xc002, 1, 0x8001);
    assert_eq!(
        sample(&memory, &bank, &line, 8, 0),
        Ok(legacy::cram_word(1))
    );
    legacy::solid(&mut memory, 2, 2);
    legacy::write(&mut memory, 0xf002, 1, 0xa002);
    legacy::publish(&mut bank, 17, 0x80);
    assert_eq!(
        sample(&memory, &bank, &line, 8, 0),
        Ok(legacy::cram_word(18))
    );
    assert_eq!(sample(&memory, &bank, &line, 0, 0), Ok(0x246));
    for (x, y) in [(256, 0), (0, 256), (u16::MAX, u16::MAX)] {
        assert_eq!(
            sample(&memory, &bank, &line, x, y),
            Err(VdpRenderError::CoordinateOutOfBounds { x, y })
        );
    }
    legacy::publish(&mut bank, 12, 2);
    assert_eq!(
        sample(&memory, &bank, &line, 0, 0),
        Err(VdpRenderError::UnsupportedRenderMode(2))
    );
}

pub fn raster_pixels() -> [u16; previous::WIDTH * previous::HEIGHT] {
    let (memory, bank) = previous::raster_scene();
    let mut pixels = [0; previous::WIDTH * previous::HEIGHT];
    for y in 0..previous::HEIGHT {
        let prepared = prepare(&memory, &bank, y as u16);
        let line = memory.compose_sprite_line(&prepared).unwrap();
        for x in 0..previous::WIDTH {
            let actual = memory
                .sample_sprite_line(&bank, &line, x as u16, y as u16)
                .unwrap();
            assert_eq!(actual, previous::expected_raster(x, y));
            pixels[y * previous::WIDTH + x] = actual;
        }
    }
    pixels
}

pub fn prepared_raster() {
    assert_eq!(raster_pixels()[0], 0xeee);
}

pub fn background_merge() {
    background_merge_with(|memory, bank, line, x, y| memory.sample_sprite_line(bank, line, x, y));
}
