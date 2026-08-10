use std::borrow::Cow;

use crate::debug::common::WatchType;
use crate::debug::common::format_addr;
use crate::settings::{ColorCorrection, DmgPalettePreset};
use zeff_emu_common::address::Address;

#[derive(Clone, Debug)]
pub(crate) struct WatchpointDisplay {
    pub(crate) address: Address,
    pub(crate) watch_type: WatchType,
}

#[derive(Clone, Debug)]
pub(crate) struct WatchHitDisplay {
    pub(crate) address: Address,
    pub(crate) old_value: u8,
    pub(crate) new_value: u8,
    pub(crate) watch_type: WatchType,
}

#[derive(Clone, Debug)]
pub(crate) struct RecentOpcodeDisplay {
    pub(crate) address: Address,
    pub(crate) bytes: Vec<u8>,
    pub(crate) detail: Option<String>,
    pub(crate) repeat_count: usize,
}

impl RecentOpcodeDisplay {
    pub(crate) fn line(&self) -> String {
        let bytes = self
            .bytes
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(" ");
        let mut line = format!("{}: {bytes}", format_addr(self.address));
        if let Some(detail) = &self.detail {
            line.push_str(" (");
            line.push_str(detail);
            line.push(')');
        }
        if self.repeat_count > 1 {
            line.push_str(&format!(" (x{})", self.repeat_count));
        }
        line
    }
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
    pub(crate) mem_around_pc: [(Address, u8); 32],
    pub(crate) recent_opcodes: Vec<RecentOpcodeDisplay>,

    pub(crate) breakpoints: Vec<Address>,
    pub(crate) watchpoints: Vec<WatchpointDisplay>,
    pub(crate) hit_breakpoint: Option<Address>,
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
    Gba(GbaGraphicsData),
    Nes(Box<NesGraphicsData>),
    Sega8(Box<Sega8GraphicsData>),
}

pub(crate) struct GbaGraphicsData {
    pub(crate) vram: Vec<u8>,
    pub(crate) palette_ram: Vec<u8>,
    pub(crate) oam: Vec<u8>,
    pub(crate) ppu: zeff_gba_core::hardware::ppu::PpuDebugSnapshot,
}

pub(crate) struct NesGraphicsData {
    pub(crate) chr_data: Vec<u8>,
    pub(crate) nametable_data: Vec<u8>,
    pub(crate) oam: [u8; 256],
    pub(crate) palette_ram: [u8; 32],
    pub(crate) palette_lut: [[u8; 4]; 64],
    pub(crate) ctrl: u8,
    pub(crate) mirroring: zeff_nes_core::hardware::cartridge::Mirroring,
    pub(crate) scroll_t: u16,
    pub(crate) fine_x: u8,
}

pub(crate) struct Sega8GraphicsData {
    pub(crate) system: zeff_sega8_core::hardware::cartridge::Sega8System,
    pub(crate) vram: Vec<u8>,
    pub(crate) cram: Vec<u8>,
    pub(crate) oam: Vec<u8>,
    pub(crate) registers: [u8; 16],
    pub(crate) status: u8,
    pub(crate) address: u16,
    pub(crate) code: u8,
    pub(crate) v_counter: u8,
    pub(crate) h_counter: u8,
    pub(crate) scanline: u16,
    pub(crate) scanline_cycle: u32,
    pub(crate) line_counter: u8,
    pub(crate) frame_interrupt_enabled: bool,
    pub(crate) line_interrupt_enabled: bool,
    pub(crate) interrupt_pending: bool,
    pub(crate) line_interrupt_pending: bool,
    pub(crate) display_enabled: bool,
    pub(crate) tms9918_mode: String,
    pub(crate) sprite_table_base: usize,
    pub(crate) mode4: zeff_sega8_core::hardware::vdp::Mode4VdpDebugSnapshot,
}

pub(crate) struct GbGraphicsData {
    pub(crate) vram: Vec<u8>,
    pub(crate) oam: Vec<u8>,
    pub(crate) ppu: zeff_gb_core::debug::PpuSnapshot,
    pub(crate) cgb_mode: bool,
    pub(crate) bg_palette_ram: [u8; 64],
    pub(crate) obj_palette_ram: [u8; 64],
    pub(crate) color_correction: ColorCorrection,
    pub(crate) color_correction_matrix: [f32; 9],
    pub(crate) dmg_palette_preset: DmgPalettePreset,
}

#[cfg(test)]
mod tests {
    use super::RecentOpcodeDisplay;

    #[test]
    fn recent_opcode_line_formats_bytes_details_and_repeats() {
        let display = RecentOpcodeDisplay {
            address: 0x1234,
            bytes: vec![0xCB, 0x7C],
            detail: Some("CB prefix".into()),
            repeat_count: 3,
        };

        assert_eq!(display.line(), "1234: CB 7C (CB prefix) (x3)");
    }

    #[test]
    fn recent_opcode_line_omits_optional_parts() {
        let display = RecentOpcodeDisplay {
            address: 0x00AF,
            bytes: vec![0xEA],
            detail: None,
            repeat_count: 1,
        };

        assert_eq!(display.line(), "00AF: EA");
    }
}
