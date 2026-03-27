use crate::cheats::CheatCode;
use crate::debug::common::WatchType;
use crate::settings::{BindingAction, InputBindingAction, ShortcutAction};
use egui::{Color32, ColorImage, TextureHandle};
use std::collections::HashMap;

pub(crate) struct PerfInfo {
    pub(crate) fps: f64,
    pub(crate) speed_mode_label: String,
    pub(crate) frames_in_flight: usize,
    pub(crate) cycles: u64,
    pub(crate) platform_name: &'static str,
    pub(crate) hardware_label: String,
    pub(crate) hardware_pref_label: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemorySearchMode {
    ByteValue,
    ByteSequence,
    AsciiString,
}

#[derive(Clone)]
pub(crate) struct MemorySearchResult {
    pub(crate) address: u16,
    pub(crate) matched_bytes: Vec<u8>,
}

pub(crate) struct MemoryViewerState {
    pub(crate) view_start: u16,
    pub(crate) jump_input: String,
    pub(crate) prev_start: Option<u16>,
    pub(crate) prev_bytes: Vec<u8>,
    pub(crate) flash_ticks: Vec<u8>,
    pub(crate) edit_addr: Option<u16>,
    pub(crate) edit_addr_input: String,
    pub(crate) edit_value: String,
    pub(crate) enable_editing: bool,
    pub(crate) search_query: String,
    pub(crate) search_mode: MemorySearchMode,
    pub(crate) search_results: Vec<MemorySearchResult>,
    pub(crate) search_max_results: usize,
    pub(crate) search_pending: bool,
    pub(crate) tbl_map: HashMap<u8, String>,
    pub(crate) tbl_path: Option<String>,
}

impl MemoryViewerState {
    pub(crate) fn new() -> Self {
        Self {
            view_start: 0,
            jump_input: String::from("0000"),
            prev_start: None,
            prev_bytes: Vec::new(),
            flash_ticks: vec![0; 256],
            edit_addr: None,
            edit_addr_input: String::new(),
            edit_value: String::new(),
            enable_editing: false,
            search_query: String::new(),
            search_mode: MemorySearchMode::ByteValue,
            search_results: Vec::new(),
            search_max_results: 256,
            search_pending: false,
            tbl_map: HashMap::new(),
            tbl_path: None,
        }
    }
}

pub(crate) struct RomViewerState {
    pub(crate) view_start: u32,
    pub(crate) jump_input: String,
    pub(crate) rom_size: u32,
    pub(crate) tbl_map: HashMap<u8, String>,
    pub(crate) tbl_path: Option<String>,
    pub(crate) search_query: String,
    pub(crate) search_mode: MemorySearchMode,
    pub(crate) search_results: Vec<RomSearchResult>,
    pub(crate) search_max_results: usize,
    pub(crate) search_pending: bool,
}

#[derive(Clone)]
pub(crate) struct RomSearchResult {
    pub(crate) offset: u32,
    pub(crate) matched_bytes: Vec<u8>,
}

impl RomViewerState {
    pub(crate) fn new() -> Self {
        Self {
            view_start: 0,
            jump_input: String::from("000000"),
            rom_size: 0,
            tbl_map: HashMap::new(),
            tbl_path: None,
            search_query: String::new(),
            search_mode: MemorySearchMode::ByteValue,
            search_results: Vec::new(),
            search_max_results: 256,
            search_pending: false,
        }
    }
}

pub(crate) struct TilemapViewerState {
    pub(crate) image: ColorImage,
    pub(crate) texture: Option<TextureHandle>,
    pub(crate) vram_dirty: bool,
    pub(crate) last_vram_signature: u64,
    pub(crate) last_bg_palette_signature: u64,
    pub(crate) last_lcdc: u8,
    pub(crate) last_bgp: u8,
    pub(crate) last_cgb_mode: bool,
    pub(crate) last_use_window_map: Option<bool>,
    pub(crate) last_show_attr_overlay: Option<bool>,
    pub(crate) last_render_cgb_colors: Option<bool>,
    pub(crate) last_color_correction: crate::settings::ColorCorrection,
    pub(crate) last_color_correction_matrix: [f32; 9],
}

impl TilemapViewerState {
    pub(crate) fn new() -> Self {
        Self {
            image: ColorImage::filled([256, 256], Color32::BLACK),
            texture: None,
            vram_dirty: true,
            last_vram_signature: 0,
            last_bg_palette_signature: 0,
            last_lcdc: 0,
            last_bgp: 0,
            last_cgb_mode: false,
            last_use_window_map: None,
            last_show_attr_overlay: None,
            last_render_cgb_colors: None,
            last_color_correction: crate::settings::ColorCorrection::None,
            last_color_correction_matrix: [
                1.0, 0.0, 0.0,
                0.0, 1.0, 0.0,
                0.0, 0.0, 1.0,
            ],
        }
    }

    pub(crate) fn invalidate_cache(&mut self) {
        self.vram_dirty = true;
    }

    pub(crate) fn update_dirty_inputs(
        &mut self,
        vram: &[u8],
        bg_palette_ram: &[u8; 64],
        ppu: zeff_gb_core::debug::PpuSnapshot,
        cgb_mode: bool,
        color_correction: crate::settings::ColorCorrection,
        color_correction_matrix: [f32; 9],
    ) {
        let vram_sig = fold_bytes(vram);
        let bg_palette_sig = fold_bytes(bg_palette_ram);
        let changed = self.last_vram_signature != vram_sig
            || self.last_bg_palette_signature != bg_palette_sig
            || self.last_lcdc != ppu.lcdc
            || self.last_bgp != ppu.bgp
            || self.last_cgb_mode != cgb_mode
            || self.last_color_correction != color_correction
            || self.last_color_correction_matrix != color_correction_matrix;

        self.vram_dirty |= changed;
        self.last_vram_signature = vram_sig;
        self.last_bg_palette_signature = bg_palette_sig;
        self.last_lcdc = ppu.lcdc;
        self.last_bgp = ppu.bgp;
        self.last_cgb_mode = cgb_mode;
        self.last_color_correction = color_correction;
        self.last_color_correction_matrix = color_correction_matrix;
    }
}

pub(crate) struct TileViewerState {
    pub(crate) image: ColorImage,
    pub(crate) texture: Option<TextureHandle>,
    pub(crate) vram_dirty: bool,
    pub(crate) last_vram_signature: u64,
    pub(crate) last_bg_palette_signature: u64,
    pub(crate) last_obj_palette_signature: u64,
    pub(crate) last_bgp: u8,
    pub(crate) last_cgb_mode: bool,
    pub(crate) last_vram_bank: Option<usize>,
    pub(crate) last_use_cgb_colors: Option<bool>,
    pub(crate) last_use_obj_palette: Option<bool>,
    pub(crate) last_cgb_palette_index: Option<u8>,
    pub(crate) last_color_correction: crate::settings::ColorCorrection,
    pub(crate) last_color_correction_matrix: [f32; 9],
}

impl TileViewerState {
    pub(crate) fn new() -> Self {
        Self {
            image: ColorImage::filled([128, 192], Color32::BLACK),
            texture: None,
            vram_dirty: true,
            last_vram_signature: 0,
            last_bg_palette_signature: 0,
            last_obj_palette_signature: 0,
            last_bgp: 0,
            last_cgb_mode: false,
            last_vram_bank: None,
            last_use_cgb_colors: None,
            last_use_obj_palette: None,
            last_cgb_palette_index: None,
            last_color_correction: crate::settings::ColorCorrection::None,
            last_color_correction_matrix: [
                1.0, 0.0, 0.0,
                0.0, 1.0, 0.0,
                0.0, 0.0, 1.0,
            ],
        }
    }

    pub(crate) fn invalidate_cache(&mut self) {
        self.vram_dirty = true;
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update_dirty_inputs(
        &mut self,
        vram: &[u8],
        bg_palette_ram: &[u8; 64],
        obj_palette_ram: &[u8; 64],
        bgp: u8,
        cgb_mode: bool,
        color_correction: crate::settings::ColorCorrection,
        color_correction_matrix: [f32; 9],
    ) {
        let vram_sig = fold_bytes(vram);
        let bg_palette_sig = fold_bytes(bg_palette_ram);
        let obj_palette_sig = fold_bytes(obj_palette_ram);
        let changed = self.last_vram_signature != vram_sig
            || self.last_bg_palette_signature != bg_palette_sig
            || self.last_obj_palette_signature != obj_palette_sig
            || self.last_bgp != bgp
            || self.last_cgb_mode != cgb_mode
            || self.last_color_correction != color_correction
            || self.last_color_correction_matrix != color_correction_matrix;

        self.vram_dirty |= changed;
        self.last_vram_signature = vram_sig;
        self.last_bg_palette_signature = bg_palette_sig;
        self.last_obj_palette_signature = obj_palette_sig;
        self.last_bgp = bgp;
        self.last_cgb_mode = cgb_mode;
        self.last_color_correction = color_correction;
        self.last_color_correction_matrix = color_correction_matrix;
    }
}

pub(crate) struct CheatState {
    pub(crate) user_codes: Vec<CheatCode>,
    pub(crate) libretro_codes: Vec<CheatCode>,
    pub(crate) input: String,
    pub(crate) name_input: String,
    pub(crate) parse_error: Option<String>,
    pub(crate) rom_title: Option<String>,
    pub(crate) rom_crc32: Option<u32>,
    pub(crate) rom_metadata_title: Option<String>,
    pub(crate) rom_metadata_rom_name: Option<String>,
    pub(crate) rom_is_gbc: bool,
    pub(crate) libretro_search_hints: Vec<String>,
    pub(crate) libretro_search: String,
    pub(crate) libretro_results: Vec<String>,
    pub(crate) libretro_status: Option<String>,
    pub(crate) libretro_file_list: Option<Vec<String>>,
    pub(crate) libretro_show: bool,
    pub(crate) cheats_dirty: bool,
}

impl CheatState {
    pub(crate) fn new() -> Self {
        Self {
            user_codes: Vec::new(),
            libretro_codes: Vec::new(),
            input: String::new(),
            name_input: String::new(),
            parse_error: None,
            rom_title: None,
            rom_crc32: None,
            rom_metadata_title: None,
            rom_metadata_rom_name: None,
            rom_is_gbc: false,
            libretro_search_hints: Vec::new(),
            libretro_search: String::new(),
            libretro_results: Vec::new(),
            libretro_status: None,
            libretro_file_list: None,
            libretro_show: false,
            cheats_dirty: true,
        }
    }
}

pub(crate) struct BreakpointState {
    pub(crate) input: String,
    pub(crate) watchpoint_input: String,
    pub(crate) watchpoint_type: WatchType,
}

impl BreakpointState {
    pub(crate) fn new() -> Self {
        Self {
            input: String::new(),
            watchpoint_input: String::new(),
            watchpoint_type: WatchType::Write,
        }
    }
}

pub(crate) struct DebugWindowState {
    pub(crate) show_cpu_debug: bool,
    pub(crate) show_input_viewer: bool,
    pub(crate) show_apu_viewer: bool,
    pub(crate) show_rom_info: bool,
    pub(crate) show_disassembler: bool,
    pub(crate) show_memory_viewer: bool,
    pub(crate) show_tile_viewer: bool,
    pub(crate) show_tilemap_viewer: bool,
    pub(crate) show_oam_viewer: bool,
    pub(crate) show_palette_viewer: bool,
    pub(crate) memory: MemoryViewerState,
    pub(crate) bp: BreakpointState,
    pub(crate) rebinding_action: Option<InputBindingAction>,
    pub(crate) rebinding_shortcut: Option<ShortcutAction>,
    pub(crate) rebinding_gamepad: Option<BindingAction>,
    pub(crate) rebinding_gamepad_action: Option<crate::settings::GamepadAction>,
    pub(crate) rebinding_speedup: bool,
    pub(crate) rebinding_rewind: bool,
    pub(crate) last_disasm_pc: Option<u16>,
    pub(crate) tilemap: TilemapViewerState,
    pub(crate) tiles: TileViewerState,
    pub(crate) show_performance: bool,
    pub(crate) show_breakpoints_window: bool,
    pub(crate) show_cheats: bool,
    pub(crate) show_rom_viewer: bool,
    pub(crate) rom_viewer: RomViewerState,
    pub(crate) perf_history: crate::debug::perf_monitor::PerfHistory,
    pub(crate) settings_tab: usize,
    pub(crate) cheat: CheatState,
    pub(crate) layer_enable_bg: bool,
    pub(crate) layer_enable_window: bool,
    pub(crate) layer_enable_sprites: bool,
}

impl DebugWindowState {
    pub(crate) fn new() -> Self {
        Self {
            show_cpu_debug: true,
            show_input_viewer: false,
            show_apu_viewer: false,
            show_rom_info: false,
            show_disassembler: false,
            show_memory_viewer: false,
            show_tile_viewer: false,
            show_tilemap_viewer: false,
            show_oam_viewer: false,
            show_palette_viewer: false,
            memory: MemoryViewerState::new(),
            bp: BreakpointState::new(),
            rebinding_action: None,
            rebinding_shortcut: None,
            rebinding_gamepad: None,
            rebinding_gamepad_action: None,
            rebinding_speedup: false,
            rebinding_rewind: false,
            last_disasm_pc: None,
            tilemap: TilemapViewerState::new(),
            tiles: TileViewerState::new(),
            show_performance: false,
            show_breakpoints_window: false,
            show_cheats: false,
            show_rom_viewer: false,
            rom_viewer: RomViewerState::new(),
            perf_history: crate::debug::perf_monitor::PerfHistory::new(),
            settings_tab: 0,
            cheat: CheatState::new(),
            layer_enable_bg: true,
            layer_enable_window: true,
            layer_enable_sprites: true,
        }
    }
}

fn fold_bytes(bytes: &[u8]) -> u64 {
    crc32fast::hash(bytes) as u64
}
