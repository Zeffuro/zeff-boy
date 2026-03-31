use egui::{Color32, ColorImage, TextureHandle};

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
    pub(crate) last_dmg_palette_preset: crate::settings::DmgPalettePreset,
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
            last_color_correction_matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            last_dmg_palette_preset: crate::settings::DmgPalettePreset::default(),
        }
    }

    pub(crate) fn invalidate_cache(&mut self) {
        self.vram_dirty = true;
    }

    pub(crate) fn update_dirty_inputs(&mut self, gfx: &super::data_models::GbGraphicsData) {
        let vram_sig = super::fold_bytes(&gfx.vram);
        let bg_palette_sig = super::fold_bytes(&gfx.bg_palette_ram);
        let changed = self.last_vram_signature != vram_sig
            || self.last_bg_palette_signature != bg_palette_sig
            || self.last_lcdc != gfx.ppu.lcdc
            || self.last_bgp != gfx.ppu.bgp
            || self.last_cgb_mode != gfx.cgb_mode
            || self.last_color_correction != gfx.color_correction
            || self.last_color_correction_matrix != gfx.color_correction_matrix
            || self.last_dmg_palette_preset != gfx.dmg_palette_preset;

        self.vram_dirty |= changed;
        self.last_vram_signature = vram_sig;
        self.last_bg_palette_signature = bg_palette_sig;
        self.last_lcdc = gfx.ppu.lcdc;
        self.last_bgp = gfx.ppu.bgp;
        self.last_cgb_mode = gfx.cgb_mode;
        self.last_color_correction = gfx.color_correction;
        self.last_color_correction_matrix = gfx.color_correction_matrix;
        self.last_dmg_palette_preset = gfx.dmg_palette_preset;
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
    pub(crate) last_dmg_palette_preset: crate::settings::DmgPalettePreset,
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
            last_color_correction_matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            last_dmg_palette_preset: crate::settings::DmgPalettePreset::default(),
        }
    }

    pub(crate) fn invalidate_cache(&mut self) {
        self.vram_dirty = true;
    }

    pub(crate) fn update_dirty_inputs(&mut self, gfx: &super::data_models::GbGraphicsData) {
        let vram_sig = super::fold_bytes(&gfx.vram);
        let bg_palette_sig = super::fold_bytes(&gfx.bg_palette_ram);
        let obj_palette_sig = super::fold_bytes(&gfx.obj_palette_ram);
        let changed = self.last_vram_signature != vram_sig
            || self.last_bg_palette_signature != bg_palette_sig
            || self.last_obj_palette_signature != obj_palette_sig
            || self.last_bgp != gfx.ppu.bgp
            || self.last_cgb_mode != gfx.cgb_mode
            || self.last_color_correction != gfx.color_correction
            || self.last_color_correction_matrix != gfx.color_correction_matrix
            || self.last_dmg_palette_preset != gfx.dmg_palette_preset;

        self.vram_dirty |= changed;
        self.last_vram_signature = vram_sig;
        self.last_bg_palette_signature = bg_palette_sig;
        self.last_obj_palette_signature = obj_palette_sig;
        self.last_bgp = gfx.ppu.bgp;
        self.last_cgb_mode = gfx.cgb_mode;
        self.last_color_correction = gfx.color_correction;
        self.last_color_correction_matrix = gfx.color_correction_matrix;
        self.last_dmg_palette_preset = gfx.dmg_palette_preset;
    }
}

pub(crate) struct PerfInfo {
    pub(crate) fps: f64,
    pub(crate) speed_mode_label: String,
    pub(crate) frames_in_flight: usize,
    pub(crate) cycles: u64,
    pub(crate) platform_name: &'static str,
    pub(crate) hardware_label: String,
    pub(crate) hardware_pref_label: String,
}
