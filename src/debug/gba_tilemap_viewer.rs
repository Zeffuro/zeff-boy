use crate::debug::TilemapViewerState;
use crate::debug::types::GbaGraphicsData;

pub(super) fn draw_gba_tilemap_viewer_content(
    ui: &mut egui::Ui,
    gfx: &GbaGraphicsData,
    window_state: &mut TilemapViewerState,
) {
    let bg_id = ui.make_persistent_id("gba_tilemap_bg");
    let mut bg = ui
        .ctx()
        .data_mut(|d| d.get_persisted::<usize>(bg_id))
        .unwrap_or(0)
        .min(3);
    ui.horizontal(|ui| {
        ui.label("BG:");
        for i in 0..4usize {
            ui.selectable_value(&mut bg, i, format!("BG{}", i));
        }
    });
    ui.ctx().data_mut(|d| d.insert_persisted(bg_id, bg));

    let control = gfx.ppu.bgcnt[bg];
    let affine = gfx.ppu.display_mode == 2 || (gfx.ppu.display_mode == 1 && bg == 2);
    let (width, height) = if affine {
        let size = 128usize << ((control >> 14) & 0x3);
        (size, size)
    } else {
        match (control >> 14) & 0x3 {
            0 => (256, 256),
            1 => (512, 256),
            2 => (256, 512),
            _ => (512, 512),
        }
    };
    let width = width.min(512);
    let height = height.min(512);
    let signature = fold_bytes(&gfx.vram)
        ^ fold_bytes(&gfx.palette_ram).rotate_left(1)
        ^ ((bg as u64) << 4)
        ^ ((control as u64) << 16)
        ^ ((affine as u64) << 48);
    let changed = window_state.tracker.last_vram_signature != signature;
    window_state.tracker.last_vram_signature = signature;
    if changed || window_state.image.size != [width, height] {
        window_state.image = egui::ColorImage::filled([width, height], egui::Color32::BLACK);
        if affine {
            render_affine_map(&mut window_state.image, gfx, control);
        } else {
            render_text_map(&mut window_state.image, gfx, control);
        }
    }

    ui.monospace(format!(
        "Mode {}  BG{} CNT={:04X}  map={}x{}  {}",
        gfx.ppu.display_mode,
        bg,
        control,
        width,
        height,
        if affine { "affine" } else { "text" }
    ));
    super::common::show_viewer_texture(
        ui,
        &mut window_state.texture,
        &window_state.image,
        "gba_tilemap_viewer",
        "gba_tilemap.png",
        1.0,
    );
}

fn render_text_map(image: &mut egui::ColorImage, gfx: &GbaGraphicsData, control: u16) {
    let char_base = (((control >> 2) & 0x3) as usize) * 0x4000;
    let color_256 = control & (1 << 7) != 0;
    let screen_base = (((control >> 8) & 0x1F) as usize) * 0x800;
    let width = image.size[0];
    let height = image.size[1];
    for y in 0..height {
        for x in 0..width {
            let Some(color_index) =
                text_color_index(&gfx.vram, char_base, screen_base, x, y, width, color_256)
            else {
                continue;
            };
            let rgba = super::gba_tile_viewer::gba_palette_rgba(&gfx.palette_ram, color_index);
            image[(x, y)] =
                egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
        }
    }
}

fn render_affine_map(image: &mut egui::ColorImage, gfx: &GbaGraphicsData, control: u16) {
    let char_base = (((control >> 2) & 0x3) as usize) * 0x4000;
    let screen_base = (((control >> 8) & 0x1F) as usize) * 0x800;
    let width = image.size[0];
    let height = image.size[1];
    let tiles_per_row = width / 8;
    for y in 0..height {
        for x in 0..width {
            let tile_x = x / 8;
            let tile_y = y / 8;
            let map_offset = screen_base + tile_y * tiles_per_row + tile_x;
            let tile = gfx.vram.get(map_offset).copied().unwrap_or(0) as usize;
            let tile_offset = char_base + tile * 64 + (y & 7) * 8 + (x & 7);
            let color_index = gfx.vram.get(tile_offset).copied().unwrap_or(0) as usize;
            if color_index == 0 {
                continue;
            }
            let rgba = super::gba_tile_viewer::gba_palette_rgba(&gfx.palette_ram, color_index);
            image[(x, y)] =
                egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
        }
    }
}

fn text_color_index(
    vram: &[u8],
    char_base: usize,
    screen_base: usize,
    x: usize,
    y: usize,
    bg_width: usize,
    color_256: bool,
) -> Option<usize> {
    let screen_x = x / 256;
    let screen_y = y / 256;
    let block = match (bg_width > 256, screen_y > 0, screen_x > 0) {
        (false, true, _) => 1,
        (true, false, true) => 1,
        (true, true, false) => 2,
        (true, true, true) => 3,
        _ => 0,
    };
    let tile_x = (x % 256) / 8;
    let tile_y = (y % 256) / 8;
    let entry_offset = screen_base + block * 0x800 + (tile_y * 32 + tile_x) * 2;
    let entry = u16::from_le_bytes([
        vram.get(entry_offset).copied().unwrap_or(0),
        vram.get(entry_offset + 1).copied().unwrap_or(0),
    ]);
    let tile = usize::from(entry & 0x03FF);
    let hflip = entry & (1 << 10) != 0;
    let vflip = entry & (1 << 11) != 0;
    let palette_bank = usize::from((entry >> 12) & 0xF);
    let px = if hflip { 7 - (x & 7) } else { x & 7 };
    let py = if vflip { 7 - (y & 7) } else { y & 7 };
    if color_256 {
        Some(
            vram.get(char_base + tile * 64 + py * 8 + px)
                .copied()
                .unwrap_or(0) as usize,
        )
    } else {
        let byte = vram
            .get(char_base + tile * 32 + py * 4 + px / 2)
            .copied()
            .unwrap_or(0);
        let nibble = if px & 1 == 0 { byte & 0x0F } else { byte >> 4 };
        Some(palette_bank * 16 + nibble as usize)
    }
}

fn fold_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0u64, |acc, &b| {
        acc.rotate_left(5).wrapping_add(u64::from(b) + 0x9E37_79B9)
    })
}
