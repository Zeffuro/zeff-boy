use super::support::*;

pub fn sprite_plane_priority_with(
    mut sample: impl FnMut(&VdpMemory, &VdpRegisterPipeline) -> Result<u16, VdpRenderError>,
) {
    let mut bank = bank();
    publish(&mut bank, 7, 63);
    let mut memory = VdpMemory::default();
    palette(&mut memory);
    for sprite_color in [0, 3] {
        solid(&mut memory, 3, sprite_color);
        for a_color in [0, 1] {
            solid(&mut memory, 1, a_color);
            for b_color in [0, 2] {
                solid(&mut memory, 2, b_color);
                for priority in 0..8u16 {
                    let (s, a, b) = (priority % 2, priority / 2 % 2, priority / 4);
                    at(&mut memory, 0, (0, 0), 0, 0, 0x4003 + s * 0x8000);
                    write(&mut memory, 0xc000, 1, 0x0001 + a * 0x8000);
                    write(&mut memory, 0xe000, 1, 0x2002 + b * 0x8000);
                    let candidates = [
                        (s == 1, sprite_color, 35),
                        (a == 1, a_color, 1),
                        (b == 1, b_color, 18),
                        (s == 0, sprite_color, 35),
                        (a == 0, a_color, 1),
                        (b == 0, b_color, 18),
                    ];
                    let index = candidates
                        .into_iter()
                        .find(|(enabled, color, _)| *enabled && *color != 0)
                        .map_or(63, |(_, _, index)| index);
                    assert_eq!(sample(&memory, &bank), Ok(cram_word(index)));
                }
            }
        }
    }
    solid(&mut memory, 1, 1);
    solid(&mut memory, 2, 2);
    solid(&mut memory, 3, 3);
    at(&mut memory, 0, (0, 0), 0, 1, 3);
    at(&mut memory, 1, (0, 0), 0, 0, 0xc003);
    write(&mut memory, 0xc000, 1, 0x8001);
    assert_eq!(sample(&memory, &bank), Ok(cram_word(1)));
    publish(&mut bank, 17, 0x80);
    write(&mut memory, 0xf000, 1, 0xa002);
    assert_eq!(sample(&memory, &bank), Ok(cram_word(18)));
    write(&mut memory, 0xf000, 1, 0xa000);
    write(&mut memory, 0xe000, 1, 0x8000);
    assert_eq!(sample(&memory, &bank), Ok(cram_word(3)));
}
pub fn sprite_plane_priority() {
    sprite_plane_priority_with(|memory, bank| memory.sample_composite(bank, 0, 0));
}

pub fn shadow_highlight_with(
    mut sample: impl FnMut(&VdpMemory, &VdpRegisterPipeline) -> Result<u16, VdpRenderError>,
) {
    let mut bank = bank();
    let mut memory = VdpMemory::default();
    palette(&mut memory);
    publish(&mut bank, 7, 63);
    for mode in [8, 0x89] {
        publish(&mut bank, 12, mode);
        for window in [false, true] {
            publish(&mut bank, 17, if window { 0x80 } else { 0 });
            let foreground = if window { 0xf000 } else { 0xc000 };
            for transparent in 0..4 {
                let a_color = if transparent & 1 == 0 { 1 } else { 0 };
                let b_color = if transparent & 2 == 0 { 2 } else { 0 };
                solid(&mut memory, 1, a_color);
                solid(&mut memory, 2, b_color);
                for priority in 0..8u16 {
                    let (s, a, b) = (priority & 1 != 0, priority & 2 != 0, priority & 4 != 0);
                    write(&mut memory, foreground, 1, 1 | if a { 0x8000 } else { 0 });
                    write(&mut memory, 0xe000, 1, 2 | if b { 0x8000 } else { 0 });
                    for palette in 0..4u16 {
                        for color in [0, 3, 14, 15] {
                            solid(&mut memory, 3, color);
                            at(
                                &mut memory,
                                0,
                                (0, 0),
                                0,
                                0,
                                3 | (palette << 13) | if s { 0x8000 } else { 0 },
                            );
                            let candidates = [
                                (a, a_color, 1),
                                (b, b_color, 2),
                                (!a, a_color, 1),
                                (!b, b_color, 2),
                            ];
                            let background = candidates
                                .into_iter()
                                .find(|(enabled, color, _)| *enabled && *color != 0)
                                .map_or(63, |(_, _, index)| index);
                            let sprite_visible =
                                color != 0 && (s || !(a && a_color != 0 || b && b_color != 0));
                            let operator = sprite_visible && palette == 3 && color >= 14;
                            let index = if sprite_visible && !operator {
                                palette * 16 + color
                            } else {
                                background
                            };
                            let normal = cram_word(index);
                            let shadow = ((normal & 0xe00) >> 1)
                                | ((normal & 0x0e0) >> 1)
                                | ((normal & 0x00e) >> 1);
                            let expected = if operator && color == 15 {
                                shadow
                            } else if operator && color == 14 {
                                if a || b { shadow + 0x777 } else { normal }
                            } else if a || b || sprite_visible && (s || color == 14) {
                                normal
                            } else {
                                shadow
                            };
                            assert_eq!(
                                sample(&memory, &bank),
                                Ok(expected),
                                "mode {mode:02x} window {window} transparent {transparent} priority {priority} sprite {palette}:{color}"
                            );
                        }
                    }
                }
            }
        }
    }
}

pub fn shadow_highlight() {
    shadow_highlight_with(|memory, bank| memory.sample_composite(bank, 0, 0));
    let mut bank = bank();
    let mut memory = VdpMemory::default();
    publish(&mut bank, 7, 63);
    publish(&mut bank, 12, 8);
    palette(&mut memory);
    solid(&mut memory, 1, 1);
    solid(&mut memory, 2, 14);
    solid(&mut memory, 3, 3);
    write(&mut memory, 0xc000, 1, 0x8001);
    at(&mut memory, 0, (0, 0), 0, 1, 0xe002);
    at(&mut memory, 1, (0, 0), 0, 0, 0x8003);
    assert_eq!(
        memory.sample_composite(&bank, 0, 0),
        Ok((cram_word(1) >> 1) + 0x777)
    );
    publish(&mut bank, 12, 0);
    assert_eq!(memory.sample_composite(&bank, 0, 0), Ok(cram_word(62)));
    publish(&mut bank, 12, 8);
    solid(&mut memory, 2, 15);
    assert_eq!(memory.sample_composite(&bank, 0, 0), Ok(cram_word(1) >> 1));
    at(&mut memory, 0, (0, 0), 0, 1, 0x6002);
    assert_eq!(memory.sample_composite(&bank, 0, 0), Ok(cram_word(1)));
}

pub fn publication_and_fifo() {
    let mut bank = bank();
    let mut memory = VdpMemory::default();
    palette(&mut memory);
    solid(&mut memory, 3, 3);
    assert_eq!(
        memory.sample_composite(&bank, 0, 0),
        Err(VdpRenderError::UnknownRegister(7))
    );
    let mut fifo = VdpWriteFifo::default();
    let mut command = ControlCommand {
        address: u32::from(bank.sprite_table_base().unwrap()),
        code: 1,
    };
    for data in [128, 0, 0x8003, 128] {
        fifo.push_data(&mut command, bank.auto_increment(), data)
            .unwrap();
    }
    assert_eq!(command.address, SAT + 8);
    for _ in 0..3 {
        fifo.complete_write(&mut memory).unwrap();
    }
    assert_eq!(
        memory.sample_composite(&bank, 0, 0),
        Err(VdpRenderError::UnsupportedSpriteMask)
    );
    fifo.complete_write(&mut memory).unwrap();
    assert_eq!(memory.sample_composite(&bank, 0, 0), Ok(cram_word(3)));
    let saved = memory.clone();
    bank.advance_to(MasterTicks::new(bank.now().get() + 1))
        .unwrap();
    let event = bank.queue_register(5, 0x5d).unwrap();
    let mut copied = bank;
    assert_eq!(memory.sample_composite(&bank, 0, 0), Ok(cram_word(3)));
    bank.advance_to(MasterTicks::new(event.publish_at.get() - 1))
        .unwrap();
    assert_eq!(memory.sample_composite(&bank, 0, 0), Ok(cram_word(3)));
    bank.advance_to(event.publish_at).unwrap();
    copied.advance_to(event.publish_at).unwrap();
    assert_eq!(memory.sample_sprite_layer(&bank, 0, 0), Ok(None));
    assert_eq!(
        memory.sample_composite(&bank, 0, 0),
        Err(VdpRenderError::UnknownRegister(7))
    );
    assert_eq!(saved.sample_sprite_layer(&copied, 0, 0), Ok(None));
    assert_eq!(memory, saved);
    let unknown = background::bank();
    assert_eq!(
        memory.sample_sprite_layer(&unknown, 0, 0),
        Err(VdpRenderError::UnknownRegister(5))
    );
    assert_eq!(
        memory.sample_composite(&unknown, 0, 0),
        Err(VdpRenderError::UnknownRegister(5))
    );
    publish(&mut bank, 12, 2);
    assert_eq!(
        memory.sample_composite(&bank, 0, 0),
        Err(VdpRenderError::UnsupportedRenderMode(2))
    );
    publish(&mut bank, 12, 0);
    assert_eq!(
        memory.sample_composite(&bank, 256, 0),
        Err(VdpRenderError::CoordinateOutOfBounds { x: 256, y: 0 })
    );
}

pub const WIDTH: usize = background::RASTER_WIDTH;
pub const HEIGHT: usize = background::RASTER_HEIGHT;
pub fn raster_scene() -> (VdpMemory, VdpRegisterPipeline) {
    let (mut memory, mut bank) = background::raster_scene();
    publish(&mut bank, 5, 0x5c);
    write(&mut memory, 8, 3, 0xe0e);
    write(&mut memory, 10, 3, 0x0ee);
    for base in [10, 20, 30] {
        for tile in base..base + 4 {
            solid(&mut memory, tile, if base == 20 { 5 } else { 4 });
        }
    }
    at(&mut memory, 0, (72, 8), 5, 1, 10);
    at(&mut memory, 1, (40, 8), 5, 2, 0x8014);
    at(&mut memory, 2, (56, 32), 5, 0, 30);
    (memory, bank)
}
pub fn expected_raster(x: usize, y: usize) -> u16 {
    let base = background::expected_raster(x, y);
    if (40..56).contains(&x) && (8..24).contains(&y) {
        return 0x0ee;
    }
    let low_sprite = ((72..88).contains(&x) && (8..24).contains(&y))
        || ((56..72).contains(&x) && (32..48).contains(&y));
    let high_background = x < 64 || (y < 16 && x % 8 < 4 && y % 8 < 4);
    if low_sprite && !high_background {
        0xe0e
    } else {
        base
    }
}
pub fn raster_pixels() -> [u16; WIDTH * HEIGHT] {
    let (memory, bank) = raster_scene();
    std::array::from_fn(|index| {
        let (x, y) = (index % WIDTH, index / WIDTH);
        let actual = memory.sample_composite(&bank, x as u16, y as u16).unwrap();
        assert_eq!(actual, expected_raster(x, y), "pixel {x},{y}");
        actual
    })
}
pub fn sprite_raster() {
    assert_eq!(raster_pixels()[0], 0xeee);
}
