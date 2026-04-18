use anyhow::Context;
use std::borrow::Cow;
use std::collections::HashMap;

pub(crate) const COLOR_ADDR: egui::Color32 = egui::Color32::from_rgb(140, 140, 170);
pub(crate) const COLOR_DIM: egui::Color32 = egui::Color32::from_rgb(90, 90, 90);
pub(crate) const COLOR_FLASH: egui::Color32 = egui::Color32::from_rgb(255, 100, 80);
pub(crate) const COLOR_BREAKPOINT_HIT: egui::Color32 = egui::Color32::from_rgb(255, 80, 80);
pub(crate) const COLOR_WATCHPOINT_HIT: egui::Color32 = egui::Color32::from_rgb(255, 180, 60);
pub(crate) const COLOR_CONTINUE_BUTTON: egui::Color32 = egui::Color32::from_rgb(40, 100, 40);
pub(crate) const COLOR_PC_HIGHLIGHT_BG: egui::Color32 = egui::Color32::from_rgb(45, 65, 45);

pub(crate) const HEX_BYTES_PER_ROW: usize = 16;
pub(crate) const HEX_ROWS_VISIBLE: usize = 16;
pub(crate) const HEX_PAGE_SIZE: usize = HEX_ROWS_VISIBLE * HEX_BYTES_PER_ROW;
pub(crate) const DEBUG_MONO_FONT_SIZE: f32 = 13.0;

pub(crate) use zeff_emu_common::debug::WatchType;

pub(crate) fn printable_ascii(byte: u8) -> char {
    if byte.is_ascii_graphic() || byte == b' ' {
        byte as char
    } else {
        '.'
    }
}

pub(crate) fn tbl_lookup<'a>(byte: u8, tbl_map: &'a HashMap<u8, String>) -> Cow<'a, str> {
    if let Some(mapped) = tbl_map.get(&byte) {
        Cow::Borrowed(mapped.as_str())
    } else {
        let ch = printable_ascii(byte);
        let mut buf = [0u8; 4];
        Cow::Owned(ch.encode_utf8(&mut buf).to_owned())
    }
}

pub(crate) fn load_tbl_file(path: &std::path::Path) -> anyhow::Result<HashMap<u8, String>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read TBL file: {}", path.display()))?;
    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some((hex_part, char_part)) = line.split_once('=') {
            let hex_part = hex_part.trim();
            let char_part = char_part.to_string();
            if let Ok(byte) = u8::from_str_radix(hex_part, 16) {
                map.insert(
                    byte,
                    if char_part.is_empty() {
                        ".".to_string()
                    } else {
                        char_part
                    },
                );
            }
        }
    }
    Ok(map)
}

macro_rules! parse_hex_fn {
    ($name:ident, $ty:ty) => {
        pub(crate) fn $name(input: &str) -> Option<$ty> {
            let trimmed = input.trim();
            let hex = trimmed
                .strip_prefix("0x")
                .or_else(|| trimmed.strip_prefix("0X"))
                .unwrap_or(trimmed);
            <$ty>::from_str_radix(hex, 16)
                .ok()
                .or_else(|| trimmed.parse::<$ty>().ok())
        }
    };
}

parse_hex_fn!(parse_hex_u8, u8);
parse_hex_fn!(parse_hex_u16, u16);
parse_hex_fn!(parse_hex_u32, u32);

pub(crate) fn nes_palette_rgba(
    palette_ram: &[u8; 32],
    palette_index: u8,
    color_id: u8,
    palette_mode: zeff_nes_core::hardware::ppu::NesPaletteMode,
) -> [u8; 4] {
    use zeff_nes_core::hardware::ppu::{NES_PALETTE, apply_nes_palette_mode};
    let pal_addr = (palette_index as usize) * 4 + (color_id as usize);
    let nes_color = if color_id == 0 {
        palette_ram[0] as usize & 0x3F
    } else {
        palette_ram[pal_addr] as usize & 0x3F
    };
    let (r, g, b) = NES_PALETTE[nes_color];
    let (r, g, b) = apply_nes_palette_mode(palette_mode, (r, g, b));
    [r, g, b, 255]
}

pub(super) fn show_viewer_texture(
    ui: &mut egui::Ui,
    texture: &mut Option<egui::TextureHandle>,
    image: &egui::ColorImage,
    name: &str,
    export_filename: &str,
    scale: f32,
) -> egui::Response {
    let tex = texture.get_or_insert_with(|| {
        ui.ctx()
            .load_texture(name, image.clone(), egui::TextureOptions::NEAREST)
    });
    tex.set(image.clone(), egui::TextureOptions::NEAREST);

    let display_size = egui::vec2(
        image.size[0] as f32 * scale,
        image.size[1] as f32 * scale,
    );
    ui.horizontal(|ui| {
        super::export::export_png_button(ui, export_filename, image);
    });
    egui::ScrollArea::both()
        .show(ui, |ui| ui.image((tex.id(), display_size)))
        .inner
}

pub(super) fn persisted_checkbox(ui: &mut egui::Ui, id: egui::Id, label: &str, default: bool) -> bool {
    let mut val = ui
        .ctx()
        .data_mut(|d| d.get_persisted::<bool>(id))
        .unwrap_or(default);
    ui.checkbox(&mut val, label);
    ui.ctx().data_mut(|d| d.insert_persisted(id, val));
    val
}

pub(super) fn hover_pixel_coords(
    response: &egui::Response,
    width: usize,
    height: usize,
) -> Option<(usize, usize)> {
    let pointer_pos = response.hover_pos()?;
    let rel_x = ((pointer_pos.x - response.rect.min.x) * (width as f32)
        / response.rect.width())
    .floor();
    let rel_y = ((pointer_pos.y - response.rect.min.y) * (height as f32)
        / response.rect.height())
    .floor();
    if rel_x >= 0.0 && rel_y >= 0.0 {
        let px = rel_x as usize;
        let py = rel_y as usize;
        if px < width && py < height {
            return Some((px, py));
        }
    }
    None
}
