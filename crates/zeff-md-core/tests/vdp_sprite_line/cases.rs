use super::fixtures::*;

pub fn geometry_and_clipping_with(
    mut compose: impl FnMut(&VdpMemory, &[VdpSpritePattern]) -> Result<VdpSpriteLine, VdpRenderError>,
) {
    let mut fixture = Fixture::new();
    patterns(&mut fixture, 100);
    for size in 0..16u8 {
        fixture.entry(0, 128, size, 0);
        let width = i32::from(size / 4 + 1) * 8;
        let height = u16::from(size % 4 + 1) * 8;
        for flip in 0..4u16 {
            fixture.word(4, 100 | (flip * 0x800) | (u16::from(size % 4) * 0x2000));
            for y in [0, height / 2, height - 1] {
                for raw_x in [1, 96, 121, 127, 128, 129, 376, 383, 384, 511] {
                    fixture.word(6, raw_x);
                    let pattern = fixture_pattern(&fixture, y);
                    let line = compose(&fixture.memory, &[pattern]).unwrap();
                    for x in 0..256 {
                        let local = x - (i32::from(raw_x) - 128);
                        let expected = if local >= 0 && local < width {
                            let sx = if flip % 2 == 1 {
                                width - 1 - local
                            } else {
                                local
                            } as u16;
                            let sy = if flip >= 2 { height - 1 - y } else { y };
                            let tile = (sx / 8) * (height / 8) + sy / 8;
                            let color = color(tile, sy % 8, sx % 8);
                            if color == 0 {
                                clear()
                            } else {
                                VdpPixel {
                                    color,
                                    palette: size % 4,
                                    priority: false,
                                }
                            }
                        } else {
                            clear()
                        };
                        assert_eq!(line.pixels()[x as usize], expected);
                    }
                }
            }
        }
    }
}

pub fn linked_alpha_priority_with(
    mut compose: impl FnMut(&VdpMemory, &[VdpSpritePattern]) -> Result<VdpSpriteLine, VdpRenderError>,
) {
    let mut fixture = Fixture::new();
    patterns(&mut fixture, 100);
    for flags in 0..16u16 {
        for shift in [-7, 0, 1, 7, 8] {
            for (index, tile, x, link, palette, priority) in [
                (0u8, 100u16, 12, 63, flags % 4, flags / 4 % 2),
                (63, 101, 12 + shift, 7, 3, flags / 8),
                (7, 102, 14, 0, 2, 1),
            ] {
                fixture.entry(index, 128, 0, link);
                fixture.word(
                    u32::from(index) * 8 + 4,
                    tile | (palette * 0x2000) | (priority * 0x8000),
                );
                fixture.word(u32::from(index) * 8 + 6, (x + 128) as u16);
            }
            let selected = fixture.cache.select_row(0).unwrap();
            assert_eq!(selected.indices(), &[0, 63, 7]);
            let prepared: Vec<_> = (0..3)
                .map(|slot| {
                    selected
                        .prepare_sprite(&fixture.cache, &fixture.memory, &fixture.bank, slot)
                        .unwrap()
                        .unwrap()
                })
                .collect();
            let line = compose(&fixture.memory, &prepared).unwrap();
            for x in 0..256i32 {
                let expected = [
                    (12, 0, flags % 4, flags / 4 % 2 == 1),
                    (12 + shift, 1, 3, flags / 8 == 1),
                    (14, 2, 2, true),
                ]
                .into_iter()
                .find_map(|(start, tile, palette, priority)| {
                    let local = x - start;
                    if !(0..8).contains(&local) {
                        return None;
                    }
                    let value = color(tile, 0, local as u16);
                    (value != 0).then_some(VdpPixel {
                        color: value,
                        palette: palette as u8,
                        priority,
                    })
                })
                .unwrap_or_else(clear);
                assert_eq!(line.pixels()[x as usize], expected);
            }
        }
    }
}

pub fn budget_and_mask_guards_with(
    mut compose: impl FnMut(&VdpMemory, &[VdpSpritePattern]) -> Result<VdpSpriteLine, VdpRenderError>,
) {
    let mut fixture = Fixture::new();
    patterns(&mut fixture, 100);
    fixture.word(4, 100);
    fixture.word(6, 128);
    assert!(
        compose(&fixture.memory, &[])
            .unwrap()
            .pixels()
            .iter()
            .all(|pixel| *pixel == clear())
    );
    for columns in 1..=4u8 {
        fixture.entry(0, 128, (columns - 1) * 4, 0);
        let pattern = fixture_pattern(&fixture, 0);
        for count in 0..=17 {
            let patterns = vec![pattern; count];
            let actual = compose(&fixture.memory, &patterns);
            if count > 16 || count * usize::from(columns) * 8 > 256 {
                assert_eq!(actual, Err(VdpRenderError::UnsupportedSpriteRow));
            } else {
                assert!(actual.is_ok());
            }
        }
    }
    fixture.entry(0, 128, 12, 0);
    let mut offscreen = fixture_pattern(&fixture, 0);
    offscreen.x = 383;
    assert!(
        compose(&fixture.memory, &[offscreen; 8])
            .unwrap()
            .pixels()
            .iter()
            .all(|pixel| pixel.is_transparent())
    );
    assert_eq!(
        compose(&fixture.memory, &[offscreen; 9]),
        Err(VdpRenderError::UnsupportedSpriteRow)
    );
    for address in [3200, 3202, 3232, 3234, 3264, 3266, 3296, 3298] {
        fixture.word(address, 0);
    }
    let transparent = fixture_pattern(&fixture, 0);
    assert!(
        compose(&fixture.memory, &[transparent; 8])
            .unwrap()
            .pixels()
            .iter()
            .all(|pixel| *pixel == clear())
    );
    assert_eq!(
        compose(&fixture.memory, &[transparent; 9]),
        Err(VdpRenderError::UnsupportedSpriteRow)
    );
    fixture.word(3200, 0x1111);
    fixture.word(3202, 0x1111);
    fixture.entry(0, 128, 0, 0);
    fixture.word(6, 0x200);
    let masked = fixture_pattern(&fixture, 0);
    fixture.word(6, 128);
    let ordinary = fixture_pattern(&fixture, 0);
    let saved = fixture.memory.clone();
    for slot in [0, 7, 15] {
        let mut patterns = [ordinary; 16];
        patterns[slot] = masked;
        assert_eq!(
            compose(&fixture.memory, &patterns),
            Err(VdpRenderError::UnsupportedSpriteMask)
        );
        assert_eq!(fixture.memory, saved);
    }
}

pub fn captured_pixels_with(
    mut compose: impl FnMut(&VdpMemory, &[VdpSpritePattern]) -> Result<VdpSpriteLine, VdpRenderError>,
) {
    let mut fixture = Fixture::new();
    patterns(&mut fixture, 100);
    fixture.entry(0, 128, 0, 0);
    fixture.word(4, 0xc864);
    fixture.word(6, 129);
    let pattern = fixture_pattern(&fixture, 0);
    let before = compose(&fixture.memory, &[pattern]).unwrap();
    let copied = before;
    fixture.word(3200, 0x1234);
    fixture.word(3202, 0x5678);
    fixture.word(4, 200);
    fixture.word(6, 300);
    fixture.entry(0, 400, 0, 0);
    fixture.cache.invalidate();
    let after = compose(&fixture.memory, &[pattern]).unwrap();
    assert_ne!(before, after);
    assert_eq!(before, copied);
    for (offset, color) in [8, 7, 6, 5, 4, 3, 2, 1].into_iter().enumerate() {
        assert_eq!(
            after.pixels()[offset + 1],
            VdpPixel {
                color,
                palette: 2,
                priority: true
            }
        );
    }
    assert_eq!(after.pixels()[0], clear());
}

pub fn geometry_and_clipping() {
    geometry_and_clipping_with(|memory, patterns| memory.compose_sprite_line(patterns));
}

pub fn linked_alpha_priority() {
    linked_alpha_priority_with(|memory, patterns| memory.compose_sprite_line(patterns));
}

pub fn budget_and_mask_guards() {
    budget_and_mask_guards_with(|memory, patterns| memory.compose_sprite_line(patterns));
}

pub fn captured_pixels() {
    captured_pixels_with(|memory, patterns| memory.compose_sprite_line(patterns));
}
