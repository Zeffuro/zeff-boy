use std::collections::VecDeque;
use crate::hardware::cartridge::CartridgeDebugInfo;
use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};
use crate::debug::breakpoints::WatchType;
use crate::settings::BindingAction;

pub(crate) struct DebugInfo {
    pub(crate) pc: u16,
    pub(crate) sp: u16,
    pub(crate) a: u8,
    pub(crate) f: u8,
    pub(crate) b: u8,
    pub(crate) c: u8,
    pub(crate) d: u8,
    pub(crate) e: u8,
    pub(crate) h: u8,
    pub(crate) l: u8,

    pub(crate) cycles: u64,
    pub(crate) ime: &'static str,
    pub(crate) cpu_state: &'static str,
    pub(crate) last_opcode: u8,
    pub(crate) last_opcode_pc: u16,

    pub(crate) fps: f64,
    pub(crate) speed_mode_label: &'static str,
    pub(crate) ppu: PpuSnapshot,
    pub(crate) hardware_mode: HardwareMode,
    pub(crate) hardware_mode_preference: HardwareModePreference,

    pub(crate) div: u8,
    pub(crate) tima: u8,
    pub(crate) tma: u8,
    pub(crate) tac: u8,

    pub(crate) if_reg: u8,
    pub(crate) ie: u8,

    pub(crate) mem_around_pc: Vec<(u16, u8)>,

    pub(crate) recent_ops: Vec<String>,
    pub(crate) breakpoints: Vec<u16>,
    pub(crate) watchpoints: Vec<String>,
    pub(crate) hit_breakpoint: Option<u16>,
    pub(crate) hit_watchpoint: Option<String>,
}

#[derive(Clone, Copy)]
pub(crate) struct PpuSnapshot {
    pub(crate) lcdc: u8,
    pub(crate) stat: u8,
    pub(crate) scy: u8,
    pub(crate) scx: u8,
    pub(crate) ly: u8,
    pub(crate) lyc: u8,
    pub(crate) wy: u8,
    pub(crate) wx: u8,
    pub(crate) bgp: u8,
    pub(crate) obp0: u8,
    pub(crate) obp1: u8,
}

pub(crate) struct DebugWindowState {
    pub(crate) show_cpu_debug: bool,
    pub(crate) show_apu_viewer: bool,
    pub(crate) show_rom_info: bool,
    pub(crate) show_disassembler: bool,
    pub(crate) show_memory_viewer: bool,
    pub(crate) show_tile_viewer: bool,
    pub(crate) show_tilemap_viewer: bool,
    pub(crate) show_oam_viewer: bool,
    pub(crate) show_palette_viewer: bool,
    pub(crate) memory_view_start: u16,
    pub(crate) memory_jump_input: String,
    pub(crate) memory_prev_start: Option<u16>,
    pub(crate) memory_prev_bytes: Vec<u8>,
    pub(crate) memory_flash_ticks: Vec<u8>,
    pub(crate) memory_edit_addr: Option<u16>,
    pub(crate) memory_edit_value: String,
    pub(crate) breakpoint_input: String,
    pub(crate) watchpoint_input: String,
    pub(crate) watchpoint_type: WatchType,
    pub(crate) rebinding_action: Option<BindingAction>,
}

impl DebugWindowState {
    pub(crate) fn new() -> Self {
        Self {
            show_cpu_debug: true,
            show_apu_viewer: false,
            show_rom_info: false,
            show_disassembler: false,
            show_memory_viewer: false,
            show_tile_viewer: false,
            show_tilemap_viewer: false,
            show_oam_viewer: false,
            show_palette_viewer: false,
            memory_view_start: 0,
            memory_jump_input: String::from("0000"),
            memory_prev_start: None,
            memory_prev_bytes: Vec::new(),
            memory_flash_ticks: vec![0; 256],
            memory_edit_addr: None,
            memory_edit_value: String::new(),
            breakpoint_input: String::new(),
            watchpoint_input: String::new(),
            watchpoint_type: WatchType::Write,
            rebinding_action: None,
        }
    }
}

pub(crate) struct RomInfoViewData {
    pub(crate) title: String,
    pub(crate) manufacturer: String,
    pub(crate) publisher: String,
    pub(crate) cartridge_type: String,
    pub(crate) rom_size: String,
    pub(crate) ram_size: String,
    pub(crate) cgb_flag: u8,
    pub(crate) sgb_flag: u8,
    pub(crate) is_cgb_compatible: bool,
    pub(crate) is_cgb_exclusive: bool,
    pub(crate) is_sgb_supported: bool,
    pub(crate) header_checksum_valid: bool,
    pub(crate) global_checksum_valid: bool,
    pub(crate) hardware_mode: HardwareMode,
    pub(crate) cartridge_state: CartridgeDebugInfo,
}

pub(crate) struct DebugViewerData {
    pub(crate) vram: Vec<u8>,
    pub(crate) oam: Vec<u8>,
    pub(crate) apu_regs: [u8; 0x17],
    pub(crate) apu_wave_ram: [u8; 0x10],
    pub(crate) apu_nr52: u8,
    pub(crate) apu_channel_samples: [[f32; 512]; 4],
    pub(crate) apu_master_samples: [f32; 512],
    pub(crate) apu_channel_muted: [bool; 4],
    pub(crate) ppu: PpuSnapshot,
    pub(crate) cgb_mode: bool,
    pub(crate) bg_palette_ram: [u8; 64],
    pub(crate) obj_palette_ram: [u8; 64],
}

pub(crate) struct OpcodeLog {
    entries: VecDeque<String>,
    capacity: usize,
}

impl OpcodeLog {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub(crate) fn push(&mut self, pc: u16, opcode: u8, is_cb: bool) {
        let label = if is_cb {
            format!("{:04X}: CB {:02X}", pc, opcode)
        } else {
            format!("{:04X}: {:02X}", pc, opcode)
        };
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(label);
    }

    pub(crate) fn recent(&self, n: usize) -> Vec<String> {
        self.entries.iter().rev().take(n).cloned().collect()
    }
}

