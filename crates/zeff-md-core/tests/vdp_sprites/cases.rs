use super::support::*;

pub fn fields_and_table_bounds() {
    let mut bank = bank();
    let mut memory = VdpMemory::default();
    let unknown = VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0));
    assert_eq!(
        memory.sprite_entry(&unknown, 0),
        Err(VdpRenderError::UnknownRegister(5))
    );
    for index in 64..=255 {
        assert_eq!(
            memory.sprite_entry(&bank, index),
            Err(VdpRenderError::InvalidSpriteIndex(index))
        );
    }
    entry(&mut memory, SAT, 0, [0xfe81, 0xfa81, 0xa064, 0xf083]);
    assert_eq!(
        memory.sprite_entry(&bank, 0),
        Ok(VdpSprite {
            x: 3,
            y: 1,
            width: 24,
            height: 24,
            link: 1,
            name: 0xa064
        })
    );
    for value in 0..=255u16 {
        entry(
            &mut memory,
            SAT,
            0,
            [128, value * 256 + (value ^ 255), 0xffff, 128],
        );
        assert_eq!(
            memory.sprite_entry(&bank, 0),
            Ok(VdpSprite {
                x: 0,
                y: 0,
                width: (value as u8 / 4 % 4 + 1) * 8,
                height: (value as u8 % 4 + 1) * 8,
                link: (value as u8 ^ 255) % 128,
                name: 0xffff
            })
        );
    }
    for position in 0..512u16 {
        for ignored in [0, 0x0200, 0x7e00, 0xfe00] {
            entry(
                &mut memory,
                SAT,
                0,
                [position | ignored, 0, 0x1357, (511 - position) | ignored],
            );
            let sprite = memory.sprite_entry(&bank, 0).unwrap();
            assert_eq!(
                (sprite.x, sprite.y),
                ((511 - position) as i16 - 128, position as i16 - 128)
            );
            assert_eq!(sprite.name, 0x1357);
        }
    }
    for value in 0..=255u8 {
        publish(&mut bank, 5, value);
        let base = u32::from(value % 128) * 512;
        for index in [0, 63] {
            entry(&mut memory, base, index, [0xffff; 4]);
            assert_eq!(
                memory.sprite_entry(&bank, index),
                Ok(VdpSprite {
                    x: 383,
                    y: 383,
                    width: 32,
                    height: 32,
                    link: 127,
                    name: 0xffff
                })
            );
        }
        if value == 127 {
            assert_eq!(&memory.vram()[0xfffe..], &[0xff, 0xff]);
        }
    }
}

pub fn shapes_and_whole_flips() {
    let bank = bank();
    let mut memory = VdpMemory::default();
    for tile in 0..6 {
        solid(&mut memory, 100 + tile, tile as u16 + 1);
    }
    for (flags, chart) in [
        (0, [[1, 3, 5], [2, 4, 6]]),
        (0x0800, [[5, 3, 1], [6, 4, 2]]),
        (0x1000, [[2, 4, 6], [1, 3, 5]]),
        (0x1800, [[6, 4, 2], [5, 3, 1]]),
    ] {
        at(&mut memory, 0, (0, 0), 9, 0, 0xa064 | flags);
        for (row, colors) in chart.into_iter().enumerate() {
            for (column, color) in colors.into_iter().enumerate() {
                assert_eq!(
                    memory.sample_sprite(&bank, 0, column as u16 * 8, row as u16 * 8),
                    Ok(Some(pixel(color, 1, true)))
                );
            }
        }
    }
    patterns(&mut memory);
    for size in 0..16u8 {
        let width = u16::from(size / 4 + 1) * 8;
        let height = u16::from(size % 4 + 1) * 8;
        for flip in 0..4u16 {
            at(&mut memory, 0, (0, 0), size, 0, 100 + flip * 0x800);
            for y in 0..height {
                for x in 0..width {
                    let sx = if flip % 2 == 1 { width - 1 - x } else { x };
                    let sy = if flip / 2 == 1 { height - 1 - y } else { y };
                    let tile = (sx / 8) * (height / 8) + sy / 8;
                    let expected = pattern_color(tile, sy % 8, sx % 8);
                    assert_eq!(
                        memory.sample_sprite(&bank, 0, x, y),
                        Ok(Some(pixel(expected, 0, false)))
                    );
                }
            }
        }
        for attributes in 0..32u16 {
            at(&mut memory, 0, (0, 0), size, 0, 100 + attributes * 0x800);
            let sx = if attributes % 2 == 1 { width - 1 } else { 0 };
            let sy = if attributes / 2 % 2 == 1 {
                height - 1
            } else {
                0
            };
            let tile = (sx / 8) * (height / 8) + sy / 8;
            assert_eq!(
                memory.sample_sprite(&bank, 0, 0, 0),
                Ok(Some(pixel(
                    pattern_color(tile, sy % 8, sx % 8),
                    (attributes / 4 % 4) as u8,
                    attributes >= 16
                )))
            );
        }
    }
}

pub fn clipping_and_pattern_wrap() {
    let bank = bank();
    let mut memory = VdpMemory::default();
    patterns(&mut memory);
    at(&mut memory, 0, (-1, -2), 0, 0, 100);
    assert_eq!(
        memory.sample_sprite(&bank, 0, 0, 0),
        Ok(Some(pixel(12, 0, false)))
    );
    assert_eq!(memory.sample_sprite(&bank, 0, 7, 0), Ok(None));
    assert_eq!(memory.sample_sprite(&bank, 0, 0, 6), Ok(None));
    at(&mut memory, 0, (255, 255), 0, 0, 100);
    assert_eq!(
        memory.sample_sprite(&bank, 0, 255, 255),
        Ok(Some(pixel(1, 0, false)))
    );
    at(&mut memory, 0, (256, 256), 0, 0, 100);
    assert_eq!(memory.sample_sprite(&bank, 0, 255, 255), Ok(None));
    for (x, y) in [(256, 0), (0, 256), (65535, 65535)] {
        assert_eq!(
            memory.sample_sprite(&bank, 0, x, y),
            Err(VdpRenderError::CoordinateOutOfBounds { x, y })
        );
        assert_eq!(
            memory.sample_sprite_layer(&bank, x, y),
            Err(VdpRenderError::CoordinateOutOfBounds { x, y })
        );
    }
    solid(&mut memory, 2047, 7);
    solid(&mut memory, 0, 9);
    at(&mut memory, 0, (0, 0), 1, 0, 0xe7ff);
    assert_eq!(
        memory.sample_sprite(&bank, 0, 0, 0),
        Ok(Some(pixel(7, 3, true)))
    );
    assert_eq!(
        memory.sample_sprite(&bank, 0, 0, 8),
        Ok(Some(pixel(9, 3, true)))
    );
    at(&mut memory, 0, (0, 0), 1, 0, 0xf7ff);
    assert_eq!(
        memory.sample_sprite(&bank, 0, 0, 0),
        Ok(Some(pixel(9, 3, true)))
    );
    assert_eq!(
        memory.sample_sprite(&bank, 0, 0, 15),
        Ok(Some(pixel(7, 3, true)))
    );
}

pub fn linked_priority_and_guards() {
    let bank = bank();
    let mut memory = VdpMemory::default();
    solid(&mut memory, 1, 0);
    solid(&mut memory, 2, 2);
    solid(&mut memory, 3, 3);
    at(&mut memory, 0, (0, 0), 0, 5, 1);
    at(&mut memory, 5, (0, 0), 0, 2, 0x2002);
    at(&mut memory, 2, (0, 0), 0, 0, 0xc003);
    assert_eq!(
        memory.sample_sprite_layer(&bank, 0, 0),
        Ok(Some(pixel(2, 1, false)))
    );
    at(&mut memory, 0, (0, 0), 0, 2, 1);
    at(&mut memory, 2, (0, 0), 0, 5, 0xc003);
    at(&mut memory, 5, (0, 0), 0, 0, 0x2002);
    assert_eq!(
        memory.sample_sprite_layer(&bank, 0, 0),
        Ok(Some(pixel(3, 2, true)))
    );
    at(&mut memory, 0, (0, 0), 0, 2, 0x8001);
    assert_eq!(
        memory.sample_sprite_layer(&bank, 0, 0),
        Ok(Some(pixel(3, 2, true)))
    );
    at(&mut memory, 0, (0, 0), 0, 0, 0x2002);
    assert_eq!(
        memory.sample_sprite_layer(&bank, 0, 0),
        Ok(Some(pixel(2, 1, false)))
    );
    at(&mut memory, 0, (0, 0), 0, 1, 2);
    at(&mut memory, 1, (0, 0), 0, 1, 3);
    assert_eq!(
        memory.sample_sprite_layer(&bank, 0, 0),
        Err(VdpRenderError::SpriteLinkCycle(1))
    );
    for link in 64..128 {
        at(&mut memory, 0, (0, 0), 0, link, 2);
        assert_eq!(
            memory.sample_sprite_layer(&bank, 0, 0),
            Err(VdpRenderError::InvalidSpriteIndex(link))
        );
    }
    at(&mut memory, 0, (0, 0), 0, 1, 2);
    entry(&mut memory, SAT, 1, [128, 0, 1, 0x0200]);
    assert_eq!(
        memory.sample_sprite_layer(&bank, 0, 0),
        Err(VdpRenderError::UnsupportedSpriteMask)
    );
    entry(&mut memory, SAT, 0, [128, 0, 1, 0x0200]);
    assert_eq!(
        memory.sample_sprite_layer(&bank, 0, 0),
        Err(VdpRenderError::UnsupportedSpriteMask)
    );
    at(&mut memory, 0, (-128, -128), 0, 0, 1);
    assert_eq!(memory.sample_sprite_layer(&bank, 0, 0), Ok(None));
    let saved = memory.clone();
    let copied = bank;
    assert_eq!(memory.sample_sprite_layer(&bank, 255, 255), Ok(None));
    assert_eq!(memory, saved);
    assert_eq!(bank, copied);
}

pub fn row_limits() {
    let bank = bank();
    for (count, size, expected) in [(16, 4, true), (17, 0, false), (8, 12, true), (9, 12, false)] {
        let mut memory = VdpMemory::default();
        solid(&mut memory, 1, 1);
        for index in 0..count {
            at(
                &mut memory,
                index,
                (if index == 0 { 0 } else { 300 }, 0),
                size,
                if index + 1 == count { 0 } else { index + 1 },
                if index == 0 { 1 } else { 0 },
            );
        }
        let result = if expected {
            Ok(Some(pixel(1, 0, false)))
        } else {
            Err(VdpRenderError::UnsupportedSpriteRow)
        };
        assert_eq!(memory.sample_sprite_layer(&bank, 0, 0), result);
    }
    let mut memory = VdpMemory::default();
    solid(&mut memory, 1, 1);
    for index in 0..64 {
        at(
            &mut memory,
            index,
            (0, if index == 63 { 0 } else { 300 }),
            0,
            if index == 63 { 0 } else { index + 1 },
            1,
        );
    }
    assert_eq!(
        memory.sample_sprite_layer(&bank, 0, 0),
        Ok(Some(pixel(1, 0, false)))
    );
    at(&mut memory, 0, (0, 0), 0, 1, 1);
    for index in 1..16 {
        at(
            &mut memory,
            index,
            (0, 0),
            4,
            if index == 15 { 0 } else { index + 1 },
            0,
        );
    }
    assert_eq!(
        memory.sample_sprite_layer(&bank, 0, 0),
        Ok(Some(pixel(1, 0, false)))
    );
    at(&mut memory, 15, (0, 0), 8, 0, 0);
    assert_eq!(
        memory.sample_sprite_layer(&bank, 0, 0),
        Ok(Some(pixel(1, 0, false)))
    );
    at(&mut memory, 15, (0, 0), 12, 0, 0);
    assert_eq!(
        memory.sample_sprite_layer(&bank, 0, 0),
        Err(VdpRenderError::UnsupportedSpriteRow)
    );
}
