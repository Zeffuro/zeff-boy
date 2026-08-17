use std::borrow::Cow;

use crate::debug::common::WatchType;
use crate::debug::common::format_addr;
use crate::settings::{ColorCorrection, DmgPalettePreset};
use zeff_emu_common::address::Address;

#[derive(Clone, Debug)]
pub(crate) struct WatchpointDisplay {
    pub(crate) address: Address,
    pub(crate) end_address: Address,
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
    pub(crate) storage_offset: Option<u64>,
    pub(crate) bytes: Vec<u8>,
    pub(crate) detail: Option<String>,
    pub(crate) repeat_count: usize,
    pub(crate) thumb: Option<bool>,
}

#[derive(Clone, Debug)]
pub(crate) struct CallStackDisplay {
    pub(crate) target: Address,
    pub(crate) return_address: Address,
    pub(crate) target_rom_offset: Option<u64>,
    pub(crate) return_rom_offset: Option<u64>,
    pub(crate) kind: &'static str,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DebugSection {
    pub(crate) heading: &'static str,
    pub(crate) lines: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IoBitDisplay {
    pub(crate) mask: u32,
    pub(crate) label: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IoRegisterDisplay {
    pub(crate) name: &'static str,
    pub(crate) address: Address,
    pub(crate) value: u32,
    pub(crate) width: u8,
    pub(crate) writable_mask: u32,
    pub(crate) bits: Vec<IoBitDisplay>,
}

pub(crate) struct CpuDebugSnapshot {
    pub(crate) register_lines: Vec<String>,
    pub(crate) flags: Vec<(char, bool)>,
    pub(crate) status_text: String,
    pub(crate) cpu_state: String,
    pub(crate) pc: Address,

    pub(crate) cycles: u64,

    pub(crate) last_opcode_line: String,
    pub(crate) sections: Vec<DebugSection>,
    pub(crate) io_registers: Vec<IoRegisterDisplay>,
    pub(crate) recent_opcodes: Vec<RecentOpcodeDisplay>,
    pub(crate) call_stack: Vec<CallStackDisplay>,
    pub(crate) call_stack_available: bool,

    pub(crate) breakpoints: Vec<Address>,
    pub(crate) one_shot_breakpoints: Vec<Address>,
    pub(crate) rom_breakpoints: Vec<u64>,
    pub(crate) watchpoints: Vec<WatchpointDisplay>,
    pub(crate) hit_breakpoint: Option<Address>,
    pub(crate) hit_rom_breakpoint: Option<u64>,
    pub(crate) hit_watchpoint: Option<WatchHitDisplay>,
}

#[derive(Default)]
pub(crate) struct CpuDebugViewState {
    previous_register_lines: Vec<String>,
    previous_flags: Vec<(char, bool)>,
    previous_section_lines: Vec<Vec<String>>,
    register_flash_ticks: Vec<u8>,
    flag_flash_ticks: Vec<u8>,
    section_flash_ticks: Vec<Vec<u8>>,
}

impl CpuDebugViewState {
    pub(crate) fn sync(&mut self, info: &CpuDebugSnapshot) {
        sync_flash_ticks(
            &mut self.register_flash_ticks,
            &self.previous_register_lines,
            &info.register_lines,
        );
        sync_flash_ticks(
            &mut self.flag_flash_ticks,
            &self.previous_flags,
            &info.flags,
        );
        self.section_flash_ticks
            .resize_with(info.sections.len(), Vec::new);
        for (index, section) in info.sections.iter().enumerate() {
            let previous = self
                .previous_section_lines
                .get(index)
                .map(Vec::as_slice)
                .unwrap_or_default();
            sync_flash_ticks(
                &mut self.section_flash_ticks[index],
                previous,
                &section.lines,
            );
        }
        self.previous_register_lines
            .clone_from(&info.register_lines);
        self.previous_flags.clone_from(&info.flags);
        self.previous_section_lines = info
            .sections
            .iter()
            .map(|section| section.lines.clone())
            .collect();
    }

    pub(crate) fn register_changed(&self, index: usize) -> bool {
        self.register_flash_ticks.get(index).copied().unwrap_or(0) > 0
    }

    pub(crate) fn flag_changed(&self, index: usize) -> bool {
        self.flag_flash_ticks.get(index).copied().unwrap_or(0) > 0
    }

    pub(crate) fn section_line_changed(&self, section: usize, line: usize) -> bool {
        self.section_flash_ticks
            .get(section)
            .and_then(|ticks| ticks.get(line))
            .copied()
            .unwrap_or(0)
            > 0
    }
}

fn sync_flash_ticks<T: PartialEq>(ticks: &mut Vec<u8>, previous: &[T], current: &[T]) {
    const FLASH_TICKS: u8 = 12;
    if previous.is_empty() {
        ticks.clear();
        ticks.resize(current.len(), 0);
        return;
    }
    ticks.resize(current.len(), 0);
    for (index, value) in current.iter().enumerate() {
        if previous.get(index) != Some(value) {
            ticks[index] = FLASH_TICKS;
        } else if ticks[index] > 0 {
            ticks[index] -= 1;
        }
    }
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
    use super::{CpuDebugSnapshot, CpuDebugViewState, RecentOpcodeDisplay};

    #[test]
    fn recent_opcode_line_formats_bytes_details_and_repeats() {
        let display = RecentOpcodeDisplay {
            address: 0x1234,
            storage_offset: None,
            bytes: vec![0xCB, 0x7C],
            detail: Some("CB prefix".into()),
            repeat_count: 3,
            thumb: None,
        };

        assert_eq!(display.line(), "1234: CB 7C (CB prefix) (x3)");
    }

    #[test]
    fn recent_opcode_line_omits_optional_parts() {
        let display = RecentOpcodeDisplay {
            address: 0x00AF,
            storage_offset: None,
            bytes: vec![0xEA],
            detail: None,
            repeat_count: 1,
            thumb: None,
        };

        assert_eq!(display.line(), "00AF: EA");
    }

    #[test]
    fn cpu_view_marks_changed_registers_and_flags() {
        let mut state = CpuDebugViewState::default();
        let mut info = CpuDebugSnapshot {
            register_lines: vec!["AF: 01B0".into()],
            flags: vec![('Z', true)],
            status_text: String::new(),
            cpu_state: String::new(),
            pc: 0,
            cycles: 0,
            last_opcode_line: String::new(),
            sections: Vec::new(),
            io_registers: Vec::new(),
            recent_opcodes: Vec::new(),
            call_stack: Vec::new(),
            call_stack_available: false,
            breakpoints: Vec::new(),
            one_shot_breakpoints: Vec::new(),
            rom_breakpoints: Vec::new(),
            watchpoints: Vec::new(),
            hit_breakpoint: None,
            hit_rom_breakpoint: None,
            hit_watchpoint: None,
        };
        state.sync(&info);
        assert!(!state.register_changed(0));
        assert!(!state.flag_changed(0));

        info.register_lines[0] = "AF: 02B0".into();
        info.flags[0] = ('Z', false);
        info.sections.push(super::DebugSection {
            heading: "Timer",
            lines: vec!["TIMA:01".into()],
        });
        state.sync(&info);
        assert!(state.register_changed(0));
        assert!(state.flag_changed(0));
        assert!(!state.section_line_changed(0, 0));

        info.sections[0].lines[0] = "TIMA:02".into();
        state.sync(&info);
        assert!(state.section_line_changed(0, 0));
    }
}
