use serde::{Deserialize, Serialize};

pub(crate) use zeff_nes_core::hardware::ppu::NesPaletteMode;

impl crate::debug::ui_helpers::EnumLabel for NesPaletteMode {
    fn label(self) -> &'static str {
        match self {
            Self::Raw => "Raw (default)",
            Self::Ntsc => "NTSC corrected",
            Self::Pal => "PAL corrected",
        }
    }

    fn all_variants() -> &'static [Self] {
        &[Self::Raw, Self::Ntsc, Self::Pal]
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

impl crate::debug::ui_helpers::EnumLabel for ScalingMode {
    fn label(self) -> &'static str {
        match self {
            Self::PixelPerfect => "Pixel Perfect",
            Self::HQ2xLike => "HQ2x-like",
            Self::XBR2x => "xBR 2x",
            Self::Eagle2x => "Eagle 2x",
            Self::Bilinear => "Bilinear",
        }
    }

    fn all_variants() -> &'static [Self] {
        &[
            Self::PixelPerfect,
            Self::Bilinear,
            Self::HQ2xLike,
            Self::XBR2x,
            Self::Eagle2x,
        ]
    }
}

impl ScalingMode {
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
    Crt,
    Scanlines,
    LcdGrid,
    GbcPalette,
    Custom,
}

impl crate::debug::ui_helpers::EnumLabel for EffectPreset {
    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Crt => "CRT",
            Self::Scanlines => "Scanlines",
            Self::LcdGrid => "LCD Grid",
            Self::GbcPalette => "GBC Palette",
            Self::Custom => "Custom (file)",
        }
    }

    fn all_variants() -> &'static [Self] {
        &[
            Self::None,
            Self::Scanlines,
            Self::LcdGrid,
            Self::Crt,
            Self::GbcPalette,
            Self::Custom,
        ]
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum ShaderPreset {
    #[default]
    None,
    Crt,
    Scanlines,
    LcdGrid,
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
            Self::Crt => (ScalingMode::PixelPerfect, EffectPreset::Crt),
            Self::Scanlines => (ScalingMode::PixelPerfect, EffectPreset::Scanlines),
            Self::LcdGrid => (ScalingMode::PixelPerfect, EffectPreset::LcdGrid),
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
