use crate::debug::breakpoints::WatchType;
use crate::hardware::cartridge::CartridgeDebugInfo;
use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};
use crate::settings::InputBindingAction;
use egui::{Color32, ColorImage, TextureHandle};
use std::collections::VecDeque;

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

    pub(crate) tilt_is_mbc7: bool,
    pub(crate) tilt_stick_controls_tilt: bool,
    pub(crate) tilt_left_stick: (f32, f32),
    pub(crate) tilt_keyboard: (f32, f32),
    pub(crate) tilt_mouse: (f32, f32),
    pub(crate) tilt_target: (f32, f32),
    pub(crate) tilt_smoothed: (f32, f32),
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
    pub(crate) rebinding_action: Option<InputBindingAction>,
    pub(crate) tilemap_image: ColorImage,
    pub(crate) tilemap_texture: Option<TextureHandle>,
    pub(crate) tilemap_vram_dirty: bool,
    pub(crate) tilemap_last_vram_signature: u64,
    pub(crate) tilemap_last_bg_palette_signature: u64,
    pub(crate) tilemap_last_lcdc: u8,
    pub(crate) tilemap_last_bgp: u8,
    pub(crate) tilemap_last_cgb_mode: bool,
    pub(crate) tilemap_last_use_window_map: Option<bool>,
    pub(crate) tilemap_last_show_attr_overlay: Option<bool>,
    pub(crate) tilemap_last_render_cgb_colors: Option<bool>,
    pub(crate) tile_viewer_image: ColorImage,
    pub(crate) tile_viewer_texture: Option<TextureHandle>,
    pub(crate) tile_viewer_vram_dirty: bool,
    pub(crate) tile_viewer_last_vram_signature: u64,
    pub(crate) tile_viewer_last_bg_palette_signature: u64,
    pub(crate) tile_viewer_last_obj_palette_signature: u64,
    pub(crate) tile_viewer_last_bgp: u8,
    pub(crate) tile_viewer_last_cgb_mode: bool,
    pub(crate) tile_viewer_last_vram_bank: Option<usize>,
    pub(crate) tile_viewer_last_use_cgb_colors: Option<bool>,
    pub(crate) tile_viewer_last_use_obj_palette: Option<bool>,
    pub(crate) tile_viewer_last_cgb_palette_index: Option<u8>,
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
            tilemap_image: ColorImage::filled([256, 256], Color32::BLACK),
            tilemap_texture: None,
            tilemap_vram_dirty: true,
            tilemap_last_vram_signature: 0,
            tilemap_last_bg_palette_signature: 0,
            tilemap_last_lcdc: 0,
            tilemap_last_bgp: 0,
            tilemap_last_cgb_mode: false,
            tilemap_last_use_window_map: None,
            tilemap_last_show_attr_overlay: None,
            tilemap_last_render_cgb_colors: None,
            tile_viewer_image: ColorImage::filled([128, 192], Color32::BLACK),
            tile_viewer_texture: None,
            tile_viewer_vram_dirty: true,
            tile_viewer_last_vram_signature: 0,
            tile_viewer_last_bg_palette_signature: 0,
            tile_viewer_last_obj_palette_signature: 0,
            tile_viewer_last_bgp: 0,
            tile_viewer_last_cgb_mode: false,
            tile_viewer_last_vram_bank: None,
            tile_viewer_last_use_cgb_colors: None,
            tile_viewer_last_use_obj_palette: None,
            tile_viewer_last_cgb_palette_index: None,
        }
    }

    pub(crate) fn any_viewer_open(&self) -> bool {
        self.show_apu_viewer
            || self.show_tile_viewer
            || self.show_tilemap_viewer
            || self.show_oam_viewer
            || self.show_palette_viewer
    }

    pub(crate) fn any_vram_viewer_open(&self) -> bool {
        self.show_tile_viewer || self.show_tilemap_viewer
    }

    pub(crate) fn invalidate_tilemap_cache(&mut self) {
        self.tilemap_vram_dirty = true;
    }

    pub(crate) fn update_tilemap_dirty_inputs(
        &mut self,
        vram: &[u8],
        bg_palette_ram: &[u8; 64],
        ppu: PpuSnapshot,
        cgb_mode: bool,
    ) {
        let vram_sig = fold_bytes(vram);
        let bg_palette_sig = fold_bytes(bg_palette_ram);
        let changed = self.tilemap_last_vram_signature != vram_sig
            || self.tilemap_last_bg_palette_signature != bg_palette_sig
            || self.tilemap_last_lcdc != ppu.lcdc
            || self.tilemap_last_bgp != ppu.bgp
            || self.tilemap_last_cgb_mode != cgb_mode;

        self.tilemap_vram_dirty |= changed;
        self.tilemap_last_vram_signature = vram_sig;
        self.tilemap_last_bg_palette_signature = bg_palette_sig;
        self.tilemap_last_lcdc = ppu.lcdc;
        self.tilemap_last_bgp = ppu.bgp;
        self.tilemap_last_cgb_mode = cgb_mode;
    }

    pub(crate) fn invalidate_tile_viewer_cache(&mut self) {
        self.tile_viewer_vram_dirty = true;
    }

    pub(crate) fn update_tile_viewer_dirty_inputs(
        &mut self,
        vram: &[u8],
        bg_palette_ram: &[u8; 64],
        obj_palette_ram: &[u8; 64],
        bgp: u8,
        cgb_mode: bool,
    ) {
        let vram_sig = fold_bytes(vram);
        let bg_palette_sig = fold_bytes(bg_palette_ram);
        let obj_palette_sig = fold_bytes(obj_palette_ram);
        let changed = self.tile_viewer_last_vram_signature != vram_sig
            || self.tile_viewer_last_bg_palette_signature != bg_palette_sig
            || self.tile_viewer_last_obj_palette_signature != obj_palette_sig
            || self.tile_viewer_last_bgp != bgp
            || self.tile_viewer_last_cgb_mode != cgb_mode;

        self.tile_viewer_vram_dirty |= changed;
        self.tile_viewer_last_vram_signature = vram_sig;
        self.tile_viewer_last_bg_palette_signature = bg_palette_sig;
        self.tile_viewer_last_obj_palette_signature = obj_palette_sig;
        self.tile_viewer_last_bgp = bgp;
        self.tile_viewer_last_cgb_mode = cgb_mode;
    }
}

fn fold_bytes(bytes: &[u8]) -> u64 {
    // Small, deterministic rolling hash for cheap dirty detection.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0001_0000_01b3);
    }
    hash
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
