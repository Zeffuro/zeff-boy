use egui::{Color32, ColorImage, TextureHandle};

pub(crate) struct ViewerDirtyTracker {
    pub(crate) vram_dirty: bool,
    pub(crate) last_vram_signature: u64,
    pub(crate) last_bg_palette_signature: u64,
    pub(crate) last_bgp: u8,
    pub(crate) last_cgb_mode: bool,
    pub(crate) last_color_correction: crate::settings::ColorCorrection,
    pub(crate) last_color_correction_matrix: [f32; 9],
    pub(crate) last_dmg_palette_preset: crate::settings::DmgPalettePreset,
}

impl ViewerDirtyTracker {
    fn new() -> Self {
        Self {
            vram_dirty: true,
            last_vram_signature: 0,
            last_bg_palette_signature: 0,
            last_bgp: 0,
            last_cgb_mode: false,
            last_color_correction: crate::settings::ColorCorrection::None,
            last_color_correction_matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            last_dmg_palette_preset: crate::settings::DmgPalettePreset::default(),
        }
    }

    pub(crate) fn check_and_update(&mut self, gfx: &super::data_models::GbGraphicsData) -> bool {
        let vram_sig = super::fold_bytes(&gfx.vram);
        let bg_palette_sig = super::fold_bytes(&gfx.bg_palette_ram);
        let changed = self.last_vram_signature != vram_sig
            || self.last_bg_palette_signature != bg_palette_sig
            || self.last_bgp != gfx.ppu.bgp
            || self.last_cgb_mode != gfx.cgb_mode
            || self.last_color_correction != gfx.color_correction
            || self.last_color_correction_matrix != gfx.color_correction_matrix
            || self.last_dmg_palette_preset != gfx.dmg_palette_preset;
        self.last_vram_signature = vram_sig;
        self.last_bg_palette_signature = bg_palette_sig;
        self.last_bgp = gfx.ppu.bgp;
        self.last_cgb_mode = gfx.cgb_mode;
        self.last_color_correction = gfx.color_correction;
        self.last_color_correction_matrix = gfx.color_correction_matrix;
        self.last_dmg_palette_preset = gfx.dmg_palette_preset;
        changed
    }
}

pub(crate) struct TilemapViewerState {
    pub(crate) image: ColorImage,
    pub(crate) texture: Option<TextureHandle>,
    pub(crate) tracker: ViewerDirtyTracker,
    pub(crate) last_lcdc: u8,
    pub(crate) last_use_window_map: Option<bool>,
    pub(crate) last_show_attr_overlay: Option<bool>,
    pub(crate) last_render_cgb_colors: Option<bool>,
}

impl TilemapViewerState {
    pub(crate) fn new() -> Self {
        Self {
            image: ColorImage::filled([256, 256], Color32::BLACK),
            texture: None,
            tracker: ViewerDirtyTracker::new(),
            last_lcdc: 0,
            last_use_window_map: None,
            last_show_attr_overlay: None,
            last_render_cgb_colors: None,
        }
    }

    pub(crate) fn invalidate_cache(&mut self) {
        self.tracker.vram_dirty = true;
    }

    pub(crate) fn update_dirty_inputs(&mut self, gfx: &super::data_models::GbGraphicsData) {
        let mut changed = self.tracker.check_and_update(gfx);
        changed |= self.last_lcdc != gfx.ppu.lcdc;
        self.last_lcdc = gfx.ppu.lcdc;
        self.tracker.vram_dirty |= changed;
    }
}

pub(crate) struct TileViewerState {
    pub(crate) image: ColorImage,
    pub(crate) texture: Option<TextureHandle>,
    pub(crate) tracker: ViewerDirtyTracker,
    pub(crate) last_obj_palette_signature: u64,
    pub(crate) last_vram_bank: Option<usize>,
    pub(crate) last_use_cgb_colors: Option<bool>,
    pub(crate) last_use_obj_palette: Option<bool>,
    pub(crate) last_cgb_palette_index: Option<u8>,
}

impl TileViewerState {
    pub(crate) fn new() -> Self {
        Self {
            image: ColorImage::filled([128, 192], Color32::BLACK),
            texture: None,
            tracker: ViewerDirtyTracker::new(),
            last_obj_palette_signature: 0,
            last_vram_bank: None,
            last_use_cgb_colors: None,
            last_use_obj_palette: None,
            last_cgb_palette_index: None,
        }
    }

    pub(crate) fn invalidate_cache(&mut self) {
        self.tracker.vram_dirty = true;
    }

    pub(crate) fn update_dirty_inputs(&mut self, gfx: &super::data_models::GbGraphicsData) {
        let obj_palette_sig = super::fold_bytes(&gfx.obj_palette_ram);
        let mut changed = self.tracker.check_and_update(gfx);
        changed |= self.last_obj_palette_signature != obj_palette_sig;
        self.last_obj_palette_signature = obj_palette_sig;
        self.tracker.vram_dirty |= changed;
    }
}

pub(crate) struct PerfInfo {
    pub(crate) fps: f64,
    pub(crate) speed_mode_label: &'static str,
    pub(crate) frames_in_flight: usize,
    pub(crate) cycles: u64,
    pub(crate) platform_name: &'static str,
    pub(crate) hardware_label: std::borrow::Cow<'static, str>,
    pub(crate) hardware_pref_label: std::borrow::Cow<'static, str>,
}
