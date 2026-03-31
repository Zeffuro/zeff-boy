mod audio;
mod display;
mod input;
mod shader;
mod theme;
mod video;

pub(crate) use audio::AudioRecordingFormat;
pub(crate) use shader::{
    ColorCorrection, ShaderParams, build_gpu_params, default_color_correction_matrix,
};
pub(crate) use display::VsyncMode;
pub(crate) use input::{LeftStickMode, TiltInputMode};
pub(crate) use theme::UiThemePreset;
pub(crate) use video::{EffectPreset, ScalingMode, ShaderPreset, default_offscreen_scale};

#[cfg(test)]
mod tests;

