use anyhow::Context;
use crate::settings::ColorCorrection;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WatchType {
    Read,
    Write,
    ReadWrite,
}

#[derive(Clone, Debug)]
pub(crate) struct WatchpointDisplay {
    pub(crate) address: u16,
    pub(crate) watch_type: WatchType,
}

#[derive(Clone, Debug)]
pub(crate) struct WatchHitDisplay {
    pub(crate) address: u16,
    pub(crate) old_value: u8,
    pub(crate) new_value: u8,
    pub(crate) watch_type: WatchType,
}

pub(crate) struct DebugSection {
    pub(crate) heading: String,
    pub(crate) lines: Vec<String>,
}

pub(crate) struct CpuDebugSnapshot {
    pub(crate) register_lines: Vec<String>,
    pub(crate) flags: Vec<(char, bool)>,
    pub(crate) status_text: String,
    pub(crate) cpu_state: String,

    pub(crate) cycles: u64,

    pub(crate) last_opcode_line: String,
    pub(crate) sections: Vec<DebugSection>,
    pub(crate) mem_around_pc: Vec<(u16, u8)>,
    pub(crate) recent_op_lines: Vec<String>,

    pub(crate) breakpoints: Vec<u16>,
    pub(crate) watchpoints: Vec<WatchpointDisplay>,
    pub(crate) hit_breakpoint: Option<u16>,
    pub(crate) hit_watchpoint: Option<WatchHitDisplay>,
}

pub(crate) struct ApuChannelDebug {
    pub(crate) name: String,
    pub(crate) enabled: bool,
    pub(crate) muted: bool,
    pub(crate) register_lines: Vec<String>,
    pub(crate) detail_line: String,
    pub(crate) waveform: Vec<f32>,
}

pub(crate) struct ApuDebugInfo {
    pub(crate) master_lines: Vec<String>,
    pub(crate) master_waveform: Vec<f32>,
    pub(crate) channels: Vec<ApuChannelDebug>,
    pub(crate) extra_sections: Vec<DebugSection>,
}

pub(crate) struct OamDebugInfo {
    pub(crate) headers: Vec<String>,
    pub(crate) rows: Vec<Vec<String>>,
}

pub(crate) struct PaletteRowDebug {
    pub(crate) label: String,
    /// RGBA colors in this row.
    pub(crate) colors: Vec<[u8; 4]>,
}

pub(crate) struct PaletteGroupDebug {
    pub(crate) title: String,
    pub(crate) rows: Vec<PaletteRowDebug>,
}

pub(crate) struct PaletteDebugInfo {
    pub(crate) groups: Vec<PaletteGroupDebug>,
}

pub(crate) struct RomInfoSection {
    pub(crate) heading: String,
    pub(crate) fields: Vec<(String, String)>,
}

pub(crate) struct RomDebugInfo {
    pub(crate) sections: Vec<RomInfoSection>,
}

pub(crate) struct InputDebugInfo {
    pub(crate) sections: Vec<DebugSection>,
    pub(crate) progress_bars: Vec<(String, f32)>,
}

pub(crate) enum ConsoleGraphicsData {
    Gb(GbGraphicsData),
    Nes(NesGraphicsData),
}

pub(crate) struct NesGraphicsData {
    pub(crate) chr_data: Vec<u8>,
    pub(crate) nametable_data: Vec<u8>,
    pub(crate) palette_ram: [u8; 32],
    pub(crate) ctrl: u8,
    pub(crate) mirroring: zeff_nes_core::hardware::cartridge::Mirroring,
    pub(crate) scroll_t: u16,
    pub(crate) fine_x: u8,
}

pub(crate) struct GbGraphicsData {
    pub(crate) vram: Vec<u8>,
    pub(crate) ppu: zeff_gb_core::debug::PpuSnapshot,
    pub(crate) cgb_mode: bool,
    pub(crate) bg_palette_ram: [u8; 64],
    pub(crate) obj_palette_ram: [u8; 64],
    pub(crate) color_correction: ColorCorrection,
    pub(crate) color_correction_matrix: [f32; 9],
}

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
