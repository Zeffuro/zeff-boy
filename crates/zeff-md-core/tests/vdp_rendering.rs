mod vdp_render {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn tile_rows_cover_vram() {
        cases::tile_rows_cover_vram();
    }
    #[test]
    fn tile_attributes_and_nibbles() {
        cases::tile_attributes_and_nibbles();
    }
    #[test]
    fn invalid_rows_preserve_memory() {
        cases::invalid_rows_preserve_memory();
    }
    #[test]
    fn plane_sizes_and_bounds() {
        cases::plane_sizes_and_bounds();
    }
    #[test]
    fn plane_table_strides_and_limits() {
        cases::plane_table_strides_and_limits();
    }
    #[test]
    fn published_bank_and_fifo_feed_pixels() {
        cases::published_bank_and_fifo_feed_pixels();
    }
    #[test]
    fn transparent_pixels_keep_attributes() {
        cases::transparent_pixels_keep_attributes();
    }
}

mod vdp_composition {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn priority_matrix() {
        cases::priority_matrix();
    }
    #[test]
    fn window_replacement() {
        cases::window_replacement();
    }
    #[test]
    fn backdrop_and_publication() {
        cases::backdrop_and_publication();
    }
    #[test]
    fn composition_guards() {
        cases::composition_guards();
    }
    #[test]
    fn static_raster() {
        cases::static_raster();
    }
}

mod vdp_window_scroll {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn window_regions_and_guards() {
        cases::window_regions_and_guards();
    }
    #[test]
    fn window_table_addresses() {
        cases::window_table_addresses();
    }
    #[test]
    fn scroll_tables_and_modes() {
        cases::scroll_tables_and_modes();
    }
    #[test]
    fn scrolled_coordinates_and_columns() {
        cases::scrolled_coordinates_and_columns();
    }
    #[test]
    fn scroll_guards() {
        cases::scroll_guards();
    }
    #[test]
    fn publication_and_fifo() {
        cases::publication_and_fifo();
    }
}

mod vdp_h40_render {
    use zeff_emu_common::time::MasterTicks;
    use zeff_md_core::{
        ControlCommand, VdpMemory, VdpPlane, VdpRegisterPipeline, VdpRenderError, VdpSpriteCache,
        VdpSpritePattern, VdpWrite,
    };

    struct Fixture {
        memory: VdpMemory,
        registers: VdpRegisterPipeline,
        cache: VdpSpriteCache,
    }

    impl Fixture {
        fn new() -> Self {
            let mut fixture = Self {
                memory: VdpMemory::default(),
                registers: VdpRegisterPipeline::at_h32_edge(MasterTicks::ZERO),
                cache: VdpSpriteCache::default(),
            };
            for (index, value) in [
                (1, 4),
                (2, 0x30),
                (3, 0x2e),
                (4, 5),
                (5, 0x11),
                (7, 15),
                (11, 0),
                (12, 0x81),
                (13, 0x3f),
                (16, 1),
                (17, 0),
                (18, 0),
            ] {
                fixture.register(index, value);
            }
            for row in 0..8 {
                fixture.word(1, 32 + row * 4, 0x1234);
                fixture.word(1, 34 + row * 4, 0x5678);
                fixture.word(1, 64 + row * 4, 0x9abc);
                fixture.word(1, 66 + row * 4, 0xdef1);
            }
            for index in 0..16 {
                fixture.word(
                    3,
                    index * 2,
                    (((index & 7) * 2) | ((index >> 3) * 0x20)) as u16,
                );
            }
            fixture
        }

        fn register(&mut self, index: u8, value: u8) {
            self.registers
                .advance_to(MasterTicks::new(self.registers.now().get() + 1))
                .unwrap();
            let write = self.registers.queue_register(index, value).unwrap();
            self.registers.advance_to(write.publish_at).unwrap();
        }

        fn word(&mut self, code: u8, address: u32, data: u16) {
            self.memory
                .write_word(ControlCommand { address, code }, data)
                .unwrap();
        }

        fn sprite(&mut self, index: u8, y: u16, layout: u16, x: u16) {
            for (offset, data) in [(0, y), (2, layout), (4, 1), (6, x)] {
                self.cache
                    .complete_write(
                        &mut self.memory,
                        &self.registers,
                        VdpWrite {
                            command: ControlCommand {
                                address: 0x2000 + u32::from(index) * 8 + offset,
                                code: 1,
                            },
                            data,
                        },
                    )
                    .unwrap();
            }
        }

        fn pattern(&self, index: u8, row: u16) -> VdpSpritePattern {
            let selected = self
                .cache
                .select_row_with_registers(&self.registers, row)
                .unwrap();
            let slot = selected
                .indices()
                .iter()
                .position(|&value| value == index)
                .unwrap();
            selected
                .prepare_sprite(&self.cache, &self.memory, &self.registers, slot as u8)
                .unwrap()
                .unwrap()
        }
    }

    pub fn h40_background_right_edge_and_h32_bounds() {
        let mut fixture = Fixture::new();
        for (column, name) in [(31, 1), (32, 2), (39, 1)] {
            fixture.word(1, 0xc000 + (27 * 64 + column) * 2, name);
        }
        for (x, expected) in [(255, 0x20), (256, 0x22), (319, 0x20)] {
            assert_eq!(
                fixture.memory.sample_background(&fixture.registers, x, 223),
                Ok(expected)
            );
        }
        assert!(matches!(
            fixture
                .memory
                .sample_background(&fixture.registers, 320, 223),
            Err(VdpRenderError::CoordinateOutOfBounds { .. })
        ));
        fixture.register(12, 0);
        assert_eq!(
            fixture
                .memory
                .sample_background(&fixture.registers, 255, 223),
            Ok(0x20)
        );
        assert_eq!(
            fixture
                .memory
                .sample_background(&fixture.registers, 256, 223),
            Err(VdpRenderError::CoordinateOutOfBounds { x: 256, y: 223 })
        );
    }

    pub fn h40_window_uses_64_columns_and_masked_base() {
        let mut fixture = Fixture::new();
        fixture.register(17, 0x80);
        fixture.word(1, 0xb000 + (64 + 32) * 2, 2);
        fixture.word(1, 0xb000 + (32 + 32) * 2, 1);
        fixture.word(1, 0xb800 + (64 + 32) * 2, 1);
        let pixel = fixture
            .memory
            .sample_window(&fixture.registers, 256, 8)
            .unwrap()
            .unwrap();
        assert_eq!(pixel.color, 9);
        assert_eq!(fixture.registers.window_base(), Some(0xb000));
        fixture.register(12, 0);
        assert_eq!(fixture.registers.window_base(), Some(0xb800));
    }

    pub fn h40_twenty_scroll_pairs() {
        let mut fixture = Fixture::new();
        fixture.register(11, 4);
        fixture.word(5, 76, 37);
        fixture.word(5, 78, 91);
        assert_eq!(
            fixture
                .memory
                .scroll_offsets(&fixture.registers, VdpPlane::A, 0, 19),
            Ok((0, 37))
        );
        assert_eq!(
            fixture
                .memory
                .scroll_offsets(&fixture.registers, VdpPlane::B, 0, 19),
            Ok((0, 91))
        );
        assert_eq!(
            fixture
                .memory
                .scroll_offsets(&fixture.registers, VdpPlane::A, 0, 20),
            Err(VdpRenderError::InvalidScrollColumn(20))
        );
        fixture.register(12, 0);
        assert_eq!(
            fixture
                .memory
                .scroll_offsets(&fixture.registers, VdpPlane::A, 0, 16),
            Err(VdpRenderError::InvalidScrollColumn(16))
        );
    }

    pub fn h40_sat_index_79_cache_and_sprite_edge() {
        let mut fixture = Fixture::new();
        fixture.register(5, 0x10);
        fixture.sprite(0, 351, 79, 383);
        fixture.sprite(79, 351, 0, 440);
        let row = fixture
            .cache
            .select_row_with_registers(&fixture.registers, 223)
            .unwrap();
        assert_eq!(row.indices(), &[0, 79]);
        assert_eq!(
            fixture
                .cache
                .entry_words_with_registers(&fixture.registers, 79),
            Ok([Some(351), Some(0)])
        );
        assert_eq!(
            fixture.cache.entry_words(79),
            Err(VdpRenderError::InvalidSpriteIndex(79))
        );
        let patterns = [fixture.pattern(0, 223), fixture.pattern(79, 223)];
        let line = fixture
            .memory
            .compose_sprite_line_with_registers(&fixture.registers, &patterns)
            .unwrap();
        assert_eq!(line.pixels().len(), 320);
        for (x, color) in [(255, 1), (256, 2), (319, 8)] {
            assert_eq!(line.pixels()[x].color, color);
        }
        assert_eq!(
            fixture.memory.sprite_entry(&fixture.registers, 80),
            Err(VdpRenderError::InvalidSpriteIndex(80))
        );
        fixture.sprite(79, 351, 79, 440);
        assert_eq!(
            fixture
                .cache
                .select_row_with_registers(&fixture.registers, 223),
            Err(VdpRenderError::SpriteLinkCycle(79))
        );
        fixture.sprite(79, 351, 127, 440);
        assert_eq!(
            fixture
                .cache
                .select_row_with_registers(&fixture.registers, 223),
            Err(VdpRenderError::InvalidSpriteIndex(127))
        );
        fixture.register(12, 0);
        assert_eq!(
            fixture
                .memory
                .compose_sprite_line(&[])
                .unwrap()
                .pixels()
                .len(),
            256
        );
        assert_eq!(
            fixture.memory.sprite_entry(&fixture.registers, 79),
            Err(VdpRenderError::InvalidSpriteIndex(79))
        );
    }

    pub fn h40_row_and_pixel_budgets() {
        let mut fixture = Fixture::new();
        fixture.register(5, 0x10);
        for index in 0..21 {
            fixture.sprite(
                index,
                128,
                if index == 20 { 0 } else { u16::from(index + 1) },
                128,
            );
        }
        let selected = fixture
            .cache
            .select_row_with_registers(&fixture.registers, 0)
            .unwrap();
        assert_eq!(selected.indices(), &(0..20).collect::<Vec<_>>());
        let narrow = fixture.pattern(0, 0);
        assert!(
            fixture
                .memory
                .compose_sprite_line_with_registers(&fixture.registers, &[narrow; 20])
                .is_ok()
        );
        assert_eq!(
            fixture
                .memory
                .compose_sprite_line_with_registers(&fixture.registers, &[narrow; 21]),
            Err(VdpRenderError::UnsupportedSpriteRow)
        );
        fixture.sprite(0, 128, 0x0c00, 128);
        let wide = fixture.pattern(0, 0);
        assert!(
            fixture
                .memory
                .compose_sprite_line_with_registers(&fixture.registers, &[wide; 10])
                .is_ok()
        );
        assert_eq!(
            fixture
                .memory
                .compose_sprite_line_with_registers(&fixture.registers, &[wide; 11]),
            Err(VdpRenderError::UnsupportedSpriteRow)
        );
        assert!(fixture.memory.compose_sprite_line(&[wide; 8]).is_ok());
        assert_eq!(
            fixture.memory.compose_sprite_line(&[wide; 9]),
            Err(VdpRenderError::UnsupportedSpriteRow)
        );
    }

    #[test]
    fn background() {
        h40_background_right_edge_and_h32_bounds();
    }
    #[test]
    fn window() {
        h40_window_uses_64_columns_and_masked_base();
    }
    #[test]
    fn scroll() {
        h40_twenty_scroll_pairs();
    }
    #[test]
    fn sprites() {
        h40_sat_index_79_cache_and_sprite_edge();
    }
    #[test]
    fn budgets() {
        h40_row_and_pixel_budgets();
    }
}
