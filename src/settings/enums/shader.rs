use serde::{Deserialize, Serialize};

pub(crate) use zeff_gb_core::color_correction::ColorCorrection;
pub(crate) use zeff_gb_core::color_correction::default_color_correction_matrix;
pub(crate) use zeff_gb_core::hardware::ppu::DmgPalettePreset;

impl crate::debug::ui_helpers::EnumLabel for DmgPalettePreset {
    fn label(self) -> &'static str {
        DmgPalettePreset::label(self)
    }

    fn all_variants() -> &'static [Self] {
        &[
            Self::Gray,
            Self::DmgGreen,
            Self::Pocket,
            Self::Mint,
            Self::Chocolate,
        ]
    }
}

impl crate::debug::ui_helpers::EnumLabel for ColorCorrection {
    fn label(self) -> &'static str {
        ColorCorrection::label(self)
    }

    fn all_variants() -> &'static [Self] {
        &[Self::None, Self::GbcLcd, Self::Custom]
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub(crate) struct ShaderParams {
    #[serde(default = "default_scanline_intensity")]
    pub(crate) scanline_intensity: f32,
    #[serde(default = "default_crt_curvature")]
    pub(crate) crt_curvature: f32,
    #[serde(default = "default_grid_intensity")]
    pub(crate) grid_intensity: f32,
    #[serde(default = "default_upscale_edge_strength")]
    pub(crate) upscale_edge_strength: f32,
    #[serde(default = "default_palette_mix")]
    pub(crate) palette_mix: f32,
    #[serde(default = "default_palette_warmth")]
    pub(crate) palette_warmth: f32,
}

fn default_scanline_intensity() -> f32 {
    0.18
}
fn default_crt_curvature() -> f32 {
    0.3
}
fn default_grid_intensity() -> f32 {
    0.3
}
fn default_upscale_edge_strength() -> f32 {
    0.65
}
fn default_palette_mix() -> f32 {
    1.0
}
fn default_palette_warmth() -> f32 {
    0.15
}

impl Default for ShaderParams {
    fn default() -> Self {
        Self {
            scanline_intensity: default_scanline_intensity(),
            crt_curvature: default_crt_curvature(),
            grid_intensity: default_grid_intensity(),
            upscale_edge_strength: default_upscale_edge_strength(),
            palette_mix: default_palette_mix(),
            palette_warmth: default_palette_warmth(),
        }
    }
}

impl ShaderParams {
    #[cfg(test)]
    pub(crate) fn to_gpu_bytes(self) -> [u8; 32] {
        let mut buf = [0u8; 32];
        buf[0..4].copy_from_slice(&self.scanline_intensity.to_le_bytes());
        buf[4..8].copy_from_slice(&self.crt_curvature.to_le_bytes());
        buf[8..12].copy_from_slice(&self.grid_intensity.to_le_bytes());
        buf[12..16].copy_from_slice(&self.upscale_edge_strength.to_le_bytes());
        buf[16..20].copy_from_slice(&self.palette_mix.to_le_bytes());
        buf[20..24].copy_from_slice(&self.palette_warmth.to_le_bytes());
        buf
    }
}

pub(crate) use zeff_gb_core::color_correction::gbc_lcd_matrix;

pub(crate) fn build_gpu_params(
    params: &ShaderParams,
    color_correction: ColorCorrection,
    color_correction_matrix: [f32; 9],
    native_width: f32,
    native_height: f32,
) -> [u8; 96] {
    let mut buf = [0u8; 96];
    buf[0..4].copy_from_slice(&params.scanline_intensity.to_le_bytes());
    buf[4..8].copy_from_slice(&params.crt_curvature.to_le_bytes());
    buf[8..12].copy_from_slice(&params.grid_intensity.to_le_bytes());
    buf[12..16].copy_from_slice(&params.upscale_edge_strength.to_le_bytes());
    buf[16..20].copy_from_slice(&params.palette_mix.to_le_bytes());
    buf[20..24].copy_from_slice(&params.palette_warmth.to_le_bytes());
    buf[24..28].copy_from_slice(&native_width.to_le_bytes());
    buf[28..32].copy_from_slice(&native_height.to_le_bytes());

    let mode: u32 = match color_correction {
        ColorCorrection::None => 0,
        ColorCorrection::GbcLcd => 1,
        ColorCorrection::Custom => 2,
    };
    buf[32..36].copy_from_slice(&mode.to_le_bytes());
    buf[36..40].copy_from_slice(&0u32.to_le_bytes());

    let matrix = match color_correction {
        ColorCorrection::None => [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        ColorCorrection::GbcLcd => gbc_lcd_matrix(),
        ColorCorrection::Custom => color_correction_matrix,
    };

    buf[48..52].copy_from_slice(&matrix[0].to_le_bytes());
    buf[52..56].copy_from_slice(&matrix[1].to_le_bytes());
    buf[56..60].copy_from_slice(&matrix[2].to_le_bytes());
    buf[60..64].copy_from_slice(&0.0_f32.to_le_bytes());

    buf[64..68].copy_from_slice(&matrix[3].to_le_bytes());
    buf[68..72].copy_from_slice(&matrix[4].to_le_bytes());
    buf[72..76].copy_from_slice(&matrix[5].to_le_bytes());
    buf[76..80].copy_from_slice(&0.0_f32.to_le_bytes());

    buf[80..84].copy_from_slice(&matrix[6].to_le_bytes());
    buf[84..88].copy_from_slice(&matrix[7].to_le_bytes());
    buf[88..92].copy_from_slice(&matrix[8].to_le_bytes());
    buf[92..96].copy_from_slice(&0.0_f32.to_le_bytes());

    buf
}
