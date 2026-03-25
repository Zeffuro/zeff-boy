use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum AudioRecordingFormat {
    #[default]
    Wav16,
    WavFloat,
    OggVorbis,
    Midi,
}

impl AudioRecordingFormat {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Wav16 => "WAV 16-bit PCM",
            Self::WavFloat => "WAV 32-bit Float",
            Self::OggVorbis => "OGG Vorbis",
            Self::Midi => "MIDI (APU channels)",
        }
    }

    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Wav16 | Self::WavFloat => "wav",
            Self::OggVorbis => "ogg",
            Self::Midi => "mid",
        }
    }

    pub(crate) fn is_midi(self) -> bool {
        matches!(self, Self::Midi)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum ScalingMode {
    #[default]
    PixelPerfect,
    HQ2xLike,
    XBR2x,
    Eagle2x,
    Bilinear,
}

impl ScalingMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::PixelPerfect => "Pixel Perfect",
            Self::HQ2xLike => "HQ2x-like",
            Self::XBR2x => "xBR 2x",
            Self::Eagle2x => "Eagle 2x",
            Self::Bilinear => "Bilinear",
        }
    }

    pub(crate) fn is_upscaler(self) -> bool {
        matches!(self, Self::HQ2xLike | Self::XBR2x | Self::Eagle2x)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum EffectPreset {
    #[default]
    None,
    CRT,
    Scanlines,
    LCDGrid,
    GbcPalette,
    Custom,
}

impl EffectPreset {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::CRT => "CRT",
            Self::Scanlines => "Scanlines",
            Self::LCDGrid => "LCD Grid",
            Self::GbcPalette => "GBC Palette",
            Self::Custom => "Custom (file)",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum ShaderPreset {
    #[default]
    None,
    CRT,
    Scanlines,
    LCDGrid,
    HQ2xLike,
    XBR2x,
    Eagle2x,
    GbcPalette,
    Custom,
}

impl ShaderPreset {
    pub(crate) fn to_scaling_and_effect(self) -> (ScalingMode, EffectPreset) {
        match self {
            Self::None => (ScalingMode::PixelPerfect, EffectPreset::None),
            Self::CRT => (ScalingMode::PixelPerfect, EffectPreset::CRT),
            Self::Scanlines => (ScalingMode::PixelPerfect, EffectPreset::Scanlines),
            Self::LCDGrid => (ScalingMode::PixelPerfect, EffectPreset::LCDGrid),
            Self::HQ2xLike => (ScalingMode::HQ2xLike, EffectPreset::None),
            Self::XBR2x => (ScalingMode::XBR2x, EffectPreset::None),
            Self::Eagle2x => (ScalingMode::Eagle2x, EffectPreset::None),
            Self::GbcPalette => (ScalingMode::PixelPerfect, EffectPreset::GbcPalette),
            Self::Custom => (ScalingMode::PixelPerfect, EffectPreset::Custom),
        }
    }
}

pub(crate) fn default_offscreen_scale() -> u32 {
    4
}

pub(crate) use zeff_gb_core::color_correction::ColorCorrection;
pub(crate) use zeff_gb_core::color_correction::default_color_correction_matrix;

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
    pub(crate) fn to_gpu_bytes(&self) -> [u8; 32] {
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
) -> [u8; 96] {
    let mut buf = [0u8; 96];
    buf[0..4].copy_from_slice(&params.scanline_intensity.to_le_bytes());
    buf[4..8].copy_from_slice(&params.crt_curvature.to_le_bytes());
    buf[8..12].copy_from_slice(&params.grid_intensity.to_le_bytes());
    buf[12..16].copy_from_slice(&params.upscale_edge_strength.to_le_bytes());
    buf[16..20].copy_from_slice(&params.palette_mix.to_le_bytes());
    buf[20..24].copy_from_slice(&params.palette_warmth.to_le_bytes());
    buf[24..28].copy_from_slice(&160.0_f32.to_le_bytes());
    buf[28..32].copy_from_slice(&144.0_f32.to_le_bytes());

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum VsyncMode {
    Off,
    #[default]
    On,
    Adaptive,
}

impl VsyncMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Off => "Off (tearing allowed)",
            Self::On => "On (VSync)",
            Self::Adaptive => "Adaptive",
        }
    }

    pub(crate) fn to_present_mode(
        self,
        capabilities: &[wgpu::PresentMode],
    ) -> wgpu::PresentMode {
        match self {
            Self::Off => {
                if capabilities.contains(&wgpu::PresentMode::Immediate) {
                    wgpu::PresentMode::Immediate
                } else if capabilities.contains(&wgpu::PresentMode::Mailbox) {
                    wgpu::PresentMode::Mailbox
                } else {
                    wgpu::PresentMode::Fifo
                }
            }
            Self::On => wgpu::PresentMode::Fifo,
            Self::Adaptive => {
                if capabilities.contains(&wgpu::PresentMode::AutoVsync) {
                    wgpu::PresentMode::AutoVsync
                } else {
                    wgpu::PresentMode::Fifo
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum LeftStickMode {
    Dpad,
    Tilt,
    #[default]
    Auto,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum TiltInputMode {
    #[default]
    Keyboard,
    Mouse,
    Auto,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vsync_on_always_fifo() {
        let caps = vec![
            wgpu::PresentMode::Fifo,
            wgpu::PresentMode::Immediate,
            wgpu::PresentMode::Mailbox,
        ];
        assert_eq!(VsyncMode::On.to_present_mode(&caps), wgpu::PresentMode::Fifo);
    }

    #[test]
    fn vsync_off_prefers_immediate() {
        let caps = vec![
            wgpu::PresentMode::Fifo,
            wgpu::PresentMode::Immediate,
            wgpu::PresentMode::Mailbox,
        ];
        assert_eq!(VsyncMode::Off.to_present_mode(&caps), wgpu::PresentMode::Immediate);
    }

    #[test]
    fn vsync_off_falls_back_to_mailbox() {
        let caps = vec![wgpu::PresentMode::Fifo, wgpu::PresentMode::Mailbox];
        assert_eq!(VsyncMode::Off.to_present_mode(&caps), wgpu::PresentMode::Mailbox);
    }

    #[test]
    fn vsync_off_falls_back_to_fifo() {
        let caps = vec![wgpu::PresentMode::Fifo];
        assert_eq!(VsyncMode::Off.to_present_mode(&caps), wgpu::PresentMode::Fifo);
    }

    #[test]
    fn vsync_adaptive_prefers_auto_vsync() {
        let caps = vec![
            wgpu::PresentMode::Fifo,
            wgpu::PresentMode::AutoVsync,
        ];
        assert_eq!(VsyncMode::Adaptive.to_present_mode(&caps), wgpu::PresentMode::AutoVsync);
    }

    #[test]
    fn vsync_adaptive_falls_back_to_fifo() {
        let caps = vec![wgpu::PresentMode::Fifo, wgpu::PresentMode::Immediate];
        assert_eq!(VsyncMode::Adaptive.to_present_mode(&caps), wgpu::PresentMode::Fifo);
    }

    #[test]
    fn vsync_default_is_on() {
        assert_eq!(VsyncMode::default(), VsyncMode::On);
    }
}

