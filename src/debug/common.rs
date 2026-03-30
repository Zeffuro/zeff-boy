use anyhow::Context;
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

pub(crate) fn tbl_lookup(byte: u8, tbl_map: &HashMap<u8, String>) -> (String, bool) {
    if let Some(mapped) = tbl_map.get(&byte) {
        (mapped.clone(), true)
    } else {
        let ch = printable_ascii(byte);
        (ch.to_string(), false)
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

pub(crate) fn nes_palette_rgba(palette_ram: &[u8; 32], palette_index: u8, color_id: u8) -> [u8; 4] {
    use zeff_nes_core::hardware::ppu::NES_PALETTE;
    let pal_addr = (palette_index as usize) * 4 + (color_id as usize);
    let nes_color = if color_id == 0 {
        palette_ram[0] as usize & 0x3F
    } else {
        palette_ram[pal_addr] as usize & 0x3F
    };
    let (r, g, b) = NES_PALETTE[nes_color];
    [r, g, b, 255]
}

