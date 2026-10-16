use super::*;

fn fixture(tile_height: usize) -> MdGraphicsData {
    let mut vram = vec![0; 0x10000];
    for tile in 0..tile_count(tile_height) {
        for y in 0..tile_height {
            let row = if y == 0 {
                [0x12, 0x34, 0x56, 0x78]
            } else if y == tile_height - 1 {
                [0x9a, 0xbc, 0xde, 0xf0]
            } else {
                [(y as u8) * 0x11; 4]
            };
            let offset = tile * tile_height * 4 + y * 4;
            vram[offset..offset + 4].copy_from_slice(&row);
        }
    }
    MdGraphicsData {
        vram,
        cram: Vec::new(),
        vsram: Vec::new(),
        palette: std::array::from_fn(|index| [index as u8 * 4, index as u8, 0, 255]),
        tile_height: Some(tile_height),
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_packed_patterns_cover_every_tile_row_pixel_and_palette() {
    for tile_height in [8, 16] {
        let gfx = fixture(tile_height);
        let count = tile_count(tile_height);
        assert_eq!(count, if tile_height == 8 { 2048 } else { 1024 });
        let height = count / COLUMNS * tile_height;
        assert_eq!(height, 1024);
        for palette in 0..4 {
            let mut image = egui::ColorImage::filled([WIDTH, height], egui::Color32::TRANSPARENT);
            render_tiles(&mut image, &gfx, palette, tile_height);
            for tile in 0..count {
                for y in 0..tile_height {
                    let expected = if y == 0 {
                        [1, 2, 3, 4, 5, 6, 7, 8]
                    } else if y == tile_height - 1 {
                        [9, 10, 11, 12, 13, 14, 15, 0]
                    } else {
                        [y; 8]
                    };
                    for (x, color) in expected.into_iter().enumerate() {
                        let index = (palette * 16 + color) as u8;
                        assert_eq!(
                            image[(tile % COLUMNS * 8 + x, tile / COLUMNS * tile_height + y)],
                            egui::Color32::from_rgb(index * 4, index, 0),
                            "height {tile_height}, palette {palette}, tile {tile}, x{x}, y{y}"
                        );
                    }
                }
            }
        }
        assert_eq!(
            pattern_color(&gfx.vram, count - 1, tile_height, 7, tile_height - 1),
            0
        );
        assert_eq!(pattern_color(&[], 0, tile_height, 0, 0), 0);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_pattern_boundaries_use_distinct_vram_addresses() {
    for tile_height in [8, 16] {
        let mut gfx = fixture(tile_height);
        gfx.vram.fill(0);
        let stride = tile_height * 4;
        gfx.vram[0] = 0x12;
        gfx.vram[stride - 1] = 0x34;
        gfx.vram[stride] = 0x56;
        gfx.vram[COLUMNS * stride] = 0x78;
        gfx.vram[0xffff] = 0x9a;
        let mut state = TileViewerState::new();
        refresh_image(&mut state, &gfx, 0);
        for (position, index) in [
            ((0, 0), 1),
            ((1, 0), 2),
            ((6, tile_height - 1), 3),
            ((7, tile_height - 1), 4),
            ((8, 0), 5),
            ((9, 0), 6),
            ((0, tile_height), 7),
            ((1, tile_height), 8),
            ((126, 1023), 9),
            ((127, 1023), 10),
        ] {
            assert_eq!(
                state.image[position],
                egui::Color32::from_rgb(index * 4, index, 0),
                "height {tile_height}, position {position:?}"
            );
        }
        assert_eq!(state.image[(16, 0)], egui::Color32::BLACK);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_atlas_refreshes_vram_cram_palette_geometry_and_restored_images() {
    let mut gfx = fixture(8);
    let mut state = TileViewerState::new();
    refresh_image(&mut state, &gfx, 0);
    let original = state.image.clone();
    let signature = state.tracker.last_vram_signature;
    refresh_image(&mut state, &gfx, 0);
    assert_eq!(state.tracker.last_vram_signature, signature);
    assert_eq!(state.image, original);
    gfx.vram[0] = 0xf2;
    refresh_image(&mut state, &gfx, 0);
    assert_eq!(state.image[(0, 0)], egui::Color32::from_rgb(60, 15, 0));
    gfx.palette[15] = [17, 23, 42, 255];
    refresh_image(&mut state, &gfx, 0);
    assert_eq!(state.image[(0, 0)], egui::Color32::from_rgb(17, 23, 42));
    refresh_image(&mut state, &gfx, 3);
    assert_eq!(state.image[(0, 0)], egui::Color32::from_rgb(252, 63, 0));
    gfx = fixture(16);
    refresh_image(&mut state, &gfx, 0);
    assert_eq!(state.image[(0, 8)], egui::Color32::from_rgb(32, 8, 0));
    assert_eq!(state.image[(0, 16)], egui::Color32::from_rgb(4, 1, 0));
    gfx = fixture(8);
    refresh_image(&mut state, &gfx, 0);
    assert_eq!(state.image, original);
    state.image[(0, 0)] = egui::Color32::TRANSPARENT;
    state.invalidate_cache();
    refresh_image(&mut state, &gfx, 0);
    assert_eq!(state.image, original);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn md_tile_draw_handles_geometry_empty_modes_and_platform_selection() {
    let context = egui::Context::default();
    let mut state = TileViewerState::new();
    let draw = |gfx: &MdGraphicsData, state: &mut TileViewerState| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 480.0),
            )),
            ..Default::default()
        };
        let output = context.run_ui(input, |ui| draw_md_tile_viewer_content(ui, gfx, state));
        assert!(!output.shapes.is_empty());
    };
    let unsupported = MdGraphicsData {
        vram: Vec::new(),
        cram: Vec::new(),
        vsram: Vec::new(),
        palette: [[3, 7, 11, 255]; 64],
        tile_height: None,
    };
    draw(&unsupported, &mut state);
    assert!(state.texture.is_none());
    assert_eq!(state.image.size, [128, 192]);
    let empty = MdGraphicsData {
        tile_height: Some(8),
        ..unsupported
    };
    draw(&empty, &mut state);
    assert!(state.texture.is_some());
    assert_eq!(state.image.size, [128, 1024]);
    assert!(
        state
            .image
            .pixels
            .iter()
            .all(|pixel| *pixel == egui::Color32::from_rgb(3, 7, 11))
    );
    state.select(TileViewerPlatform::Gb, 2047);
    draw(&fixture(8), &mut state);
    assert_eq!(state.selected_platform, Some(TileViewerPlatform::Gb));
    assert_eq!(state.selected, Some(2047));
    assert_eq!(state.image[(0, 0)], egui::Color32::from_rgb(4, 1, 0));
    state.select(TileViewerPlatform::Md, 2047);
    draw(&fixture(8), &mut state);
    assert_eq!(state.selected, Some(2047));
    draw(&fixture(16), &mut state);
    assert_eq!(state.selected_platform, Some(TileViewerPlatform::Md));
    assert_eq!(state.selected, Some(1023));
    assert_eq!(state.image[(0, 8)], egui::Color32::from_rgb(32, 8, 0));
    assert_eq!(state.image[(0, 16)], egui::Color32::from_rgb(4, 1, 0));

    for (tab, vram) in [
        (crate::debug::DebugTab::TileViewer, true),
        (crate::debug::DebugTab::PaletteViewer, false),
    ] {
        let dock = egui_dock::DockState::new(vec![tab]);
        let requirements = crate::debug::compute_tab_requirements(&dock);
        assert!(requirements.needs_viewer_data);
        assert_eq!(requirements.needs_vram, vram);
        assert!(!requirements.needs_debug_info);
    }
}
