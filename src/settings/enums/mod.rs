mod audio;
mod display;
mod input;
mod shader;
mod theme;
mod ui;
mod video;

pub(crate) use audio::AudioRecordingFormat;
pub(crate) use display::VsyncMode;
pub(crate) use input::{LeftStickMode, TiltInputMode};
pub(crate) use shader::{
    ColorCorrection, DmgPalettePreset, EffectiveColorCorrection, GbaColorCorrection, ShaderParams,
    WonderSwanColorCorrection, build_gpu_params, default_color_correction_matrix,
    effective_gb_color_correction, effective_gba_color_correction,
    effective_wonderswan_color_correction,
};
pub(crate) use theme::{UiDensity, UiThemePreset};
pub(crate) use ui::DebugPresentation;
pub(crate) use video::{
    EffectPreset, NesPaletteMode, PceOverscanMode, PcePaletteMode, ScalingMode, ShaderPreset,
    default_offscreen_scale,
};

#[cfg(test)]
mod tests;
