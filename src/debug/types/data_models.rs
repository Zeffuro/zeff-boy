use std::borrow::Cow;

use crate::debug::common::WatchType;
use crate::settings::{ColorCorrection, DmgPalettePreset};

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
    pub(crate) heading: &'static str,
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
    pub(crate) mem_around_pc: [(u16, u8); 32],
    pub(crate) recent_op_lines: Vec<String>,

    pub(crate) breakpoints: Vec<u16>,
    pub(crate) watchpoints: Vec<WatchpointDisplay>,
    pub(crate) hit_breakpoint: Option<u16>,
    pub(crate) hit_watchpoint: Option<WatchHitDisplay>,
}

pub(crate) struct ApuChannelDebug {
    pub(crate) name: &'static str,
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
    pub(crate) headers: &'static [&'static str],
    pub(crate) rows: Vec<Vec<String>>,
}

pub(crate) struct PaletteRowDebug {
    pub(crate) label: String,
    /// RGBA colors in this row.
    pub(crate) colors: Vec<[u8; 4]>,
}

pub(crate) struct PaletteGroupDebug {
    pub(crate) title: Cow<'static, str>,
    pub(crate) rows: Vec<PaletteRowDebug>,
}

pub(crate) struct PaletteDebugInfo {
    pub(crate) groups: Vec<PaletteGroupDebug>,
}

pub(crate) struct RomInfoSection {
    pub(crate) heading: &'static str,
    pub(crate) fields: Vec<(&'static str, String)>,
}

pub(crate) struct RomDebugInfo {
    pub(crate) sections: Vec<RomInfoSection>,
}

pub(crate) struct InputDebugInfo {
    pub(crate) sections: Vec<DebugSection>,
    pub(crate) progress_bars: Vec<(&'static str, f32)>,
}

pub(crate) enum ConsoleGraphicsData {
    Gb(GbGraphicsData),
    Nes(NesGraphicsData),
}

pub(crate) struct NesGraphicsData {
    pub(crate) chr_data: Vec<u8>,
    pub(crate) nametable_data: Vec<u8>,
    pub(crate) palette_ram: [u8; 32],
    pub(crate) palette_mode: zeff_nes_core::hardware::ppu::NesPaletteMode,
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
    pub(crate) dmg_palette_preset: DmgPalettePreset,
}
