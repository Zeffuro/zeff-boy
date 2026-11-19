use super::support::*;

pub fn prepared_columns() {
    let mut fixture = Fixture::new();
    patterns(&mut fixture, 100);
    fixture.word(6, 200);
    for size in 0..16u8 {
        let width = u16::from(size / 4 + 1) * 8;
        let height = u16::from(size % 4 + 1) * 8;
        fixture.entry(0, 128, size, 0);
        for attribute in 0..32 {
            fixture.word(4, 100 | attributes(attribute));
            for y in 0..height {
                let selected = fixture.cache.select_row(y).unwrap();
                let prepared = selected
                    .prepare_sprite(&fixture.cache, &fixture.memory, &fixture.bank, 0)
                    .unwrap()
                    .unwrap();
                assert_eq!(prepared.x, 72);
                assert_eq!(u16::from(prepared.columns()) * 8, width);
                let sy = if attribute >= 16 { height - 1 - y } else { y };
                for column in 0..prepared.columns() {
                    let source_column = if (attribute / 8) % 2 == 1 {
                        width / 8 - 1 - u16::from(column)
                    } else {
                        u16::from(column)
                    };
                    let tile = source_column * (height / 8) + sy / 8;
                    assert_eq!(
                        prepared.row_address(column),
                        Some((100 + tile) * 32 + (sy % 8) * 4)
                    );
                    let row = prepared.tile_row(&fixture.memory, column).unwrap().unwrap();
                    assert_eq!(row.palette, (attribute % 4) as u8);
                    assert_eq!(row.priority, (attribute / 4) % 2 == 1);
                    for pixel in 0..8u16 {
                        let sx = if (attribute / 8) % 2 == 1 {
                            7 - pixel
                        } else {
                            pixel
                        };
                        assert_eq!(row.colors[pixel as usize], color(tile, sy % 8, sx));
                    }
                }
                assert_eq!(prepared.row_address(prepared.columns()), None);
                assert_eq!(prepared.tile_row(&fixture.memory, 255), Ok(None));
            }
        }
    }
}

pub fn selected_slots_and_live_fields() {
    let mut linked = Fixture::new();
    linked.entry(0, 400, 0, 63);
    linked.entry(63, 128, 0, 7);
    linked.entry(7, 128, 0, 0);
    for (index, name, x) in [(63u8, 350u16, 190u16), (7, 450, 290)] {
        linked.word(u32::from(index) * 8 + 4, name);
        linked.word(u32::from(index) * 8 + 6, x);
    }
    let selected = linked.cache.select_row(3).unwrap();
    assert_eq!(selected.indices(), &[63, 7]);
    for (slot, x, name) in [(0u8, 62, 350u16), (1, 162, 450)] {
        let prepared = selected
            .prepare_sprite(&linked.cache, &linked.memory, &linked.bank, slot)
            .unwrap()
            .unwrap();
        assert_eq!(prepared.x, x);
        assert_eq!(prepared.row_address(0), Some(name * 32 + 12));
    }
    let mut fixture = Fixture::new();
    for index in 0..17 {
        fixture.entry(index, 128, 15, if index == 16 { 0 } else { index + 1 });
        fixture.word(u32::from(index) * 8 + 4, 100 + u16::from(index));
        fixture.word(u32::from(index) * 8 + 6, 144 + u16::from(index));
    }
    let selected = fixture.cache.select_row(3).unwrap();
    assert_eq!(selected.indices().len(), 16);
    fixture.entry(0, 128, 15, 0);
    for slot in 0..16 {
        let prepared = selected
            .prepare_sprite(&fixture.cache, &fixture.memory, &fixture.bank, slot)
            .unwrap()
            .unwrap();
        assert_eq!(prepared.x, 16 + i16::from(slot));
        assert_eq!(prepared.columns(), 4);
        assert_eq!(
            prepared.row_address(0),
            Some((100 + u16::from(slot)) * 32 + 12)
        );
    }
    for slot in 16..=255 {
        assert_eq!(
            selected.prepare_sprite(&fixture.cache, &fixture.memory, &fixture.bank, slot),
            Ok(None)
        );
    }
    fixture.word(512 + 4, 400);
    fixture.word(512 + 6, 300);
    fixture.publish(1);
    let prepared = selected
        .prepare_sprite(&fixture.cache, &fixture.memory, &fixture.bank, 0)
        .unwrap()
        .unwrap();
    assert_eq!(prepared.x, 172);
    assert_eq!(prepared.row_address(0), Some(400 * 32 + 12));
    fixture.word(512, 120);
    fixture.word(514, 0x0100);
    let reread = selected
        .prepare_sprite(&fixture.cache, &fixture.memory, &fixture.bank, 0)
        .unwrap()
        .unwrap();
    assert_eq!(reread.columns(), 1);
    assert_eq!(reread.row_address(0), Some(401 * 32 + 12));
    let empty = fixture.cache.select_row(100).unwrap();
    fixture.cache.invalidate();
    assert_eq!(
        empty.prepare_sprite(&fixture.cache, &fixture.memory, &fixture.bank, 0),
        Ok(None)
    );
}

pub fn captured_metadata_and_live_patterns() {
    let mut fixture = Fixture::new();
    patterns(&mut fixture, 100);
    fixture.entry(0, 128, 15, 0);
    fixture.word(4, 100 | 0x6800);
    fixture.word(6, 200);
    let selected = fixture.cache.select_row(9).unwrap();
    let prepared = selected
        .prepare_sprite(&fixture.cache, &fixture.memory, &fixture.bank, 0)
        .unwrap()
        .unwrap();
    let copy = prepared;
    let before = prepared.tile_row(&fixture.memory, 0).unwrap().unwrap();
    fixture.entry(0, 400, 0, 0);
    fixture.word(4, 200 | 0x9000);
    fixture.word(6, 300);
    fixture.publish(1);
    fixture.cache.invalidate();
    assert_eq!(prepared, copy);
    assert_eq!(prepared.x, 72);
    assert_eq!(prepared.tile_row(&fixture.memory, 0), Ok(Some(before)));
    let original_memory = fixture.memory.clone();
    let address = u32::from(prepared.row_address(0).unwrap());
    fixture.word(address, 0x1234);
    fixture.word(address + 2, 0x5678);
    let updated = prepared.tile_row(&fixture.memory, 0).unwrap().unwrap();
    assert_eq!(updated.colors, [8, 7, 6, 5, 4, 3, 2, 1]);
    assert_eq!((updated.palette, updated.priority), (3, false));
    assert_eq!(copy.tile_row(&original_memory, 0), Ok(Some(before)));
    assert_eq!(prepared, copy);
}

pub fn wrap_positions_and_guards() {
    let mut fixture = Fixture::new();
    fixture.entry(0, 128, 15, 0);
    fixture.word(0x8004, 0x7ff);
    fixture.word(0x8006, 128);
    fixture.publish(0x40);
    patterns(&mut fixture, 0x7ff);
    for flip in 0..4u16 {
        fixture.word(0x8004, 0x7ff | (flip * 0x800));
        for y in 0..32 {
            let selected = fixture.cache.select_row(y).unwrap();
            let prepared = selected
                .prepare_sprite(&fixture.cache, &fixture.memory, &fixture.bank, 0)
                .unwrap()
                .unwrap();
            let sy = if flip >= 2 { 31 - y } else { y };
            for column in 0..4 {
                let sc = if flip % 2 == 1 { 3 - column } else { column };
                let tile = u16::from(sc) * 4 + sy / 8;
                assert_eq!(
                    prepared.row_address(column),
                    Some(((0x7ff + tile) % 2048) * 32 + (sy % 8) * 4)
                );
                let row = prepared.tile_row(&fixture.memory, column).unwrap().unwrap();
                for pixel in 0..8u16 {
                    assert_eq!(
                        row.colors[pixel as usize],
                        color(tile, sy % 8, if flip % 2 == 1 { 7 - pixel } else { pixel })
                    );
                }
            }
        }
    }
    let selected = fixture.cache.select_row(0).unwrap();
    for x in 0..512 {
        fixture.word(0x8006, x | 0xfe00);
        let prepared = selected
            .prepare_sprite(&fixture.cache, &fixture.memory, &fixture.bank, 0)
            .unwrap()
            .unwrap();
        assert_eq!(prepared.x, x as i16 - 128);
    }
    let cache_before = fixture.cache;
    let memory_before = fixture.memory.clone();
    let unknown_bank = VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0));
    assert_eq!(
        selected.prepare_sprite(&fixture.cache, &fixture.memory, &unknown_bank, 0),
        Err(VdpRenderError::UnknownRegister(5))
    );
    assert_eq!(fixture.cache, cache_before);
    assert_eq!(fixture.memory, memory_before);
    fixture.word(0x8000, 400);
    assert_eq!(
        selected.prepare_sprite(&fixture.cache, &fixture.memory, &fixture.bank, 0),
        Err(VdpRenderError::UnsupportedSpriteRow)
    );
    fixture.word(0x8000, 129);
    assert_eq!(
        selected.prepare_sprite(&fixture.cache, &fixture.memory, &fixture.bank, 0),
        Err(VdpRenderError::UnsupportedSpriteRow)
    );
    for missing in [VdpSpriteCache::default(), {
        let mut cache = fixture.cache;
        cache.invalidate();
        cache
    }] {
        assert_eq!(
            selected.prepare_sprite(&missing, &fixture.memory, &fixture.bank, 0),
            Err(VdpRenderError::UnknownSpriteCache(0))
        );
    }
}
