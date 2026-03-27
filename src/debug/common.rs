use crate::settings::ColorCorrection;

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

    #[allow(dead_code)]
    pub(crate) pc: u16,
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
    /// Progress bars: `(label, 0.0..1.0)`.
    pub(crate) progress_bars: Vec<(String, f32)>,
}

pub(crate) enum ConsoleGraphicsData {
    Gb(GbGraphicsData),
    // Future: Nes(NesGraphicsData),
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

