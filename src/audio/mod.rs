#[cfg(not(target_arch = "wasm32"))]
mod resampler;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::AudioOutput;
#[cfg(not(target_arch = "wasm32"))]
#[allow(unused_imports)]
use native::*;

#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub(crate) use web::AudioOutput;

pub(crate) struct AudioQueueConfig {
    pub master_volume: f32,
    pub fast_forward_active: bool,
    pub mute_during_fast_forward: bool,
    pub low_pass_enabled: bool,
    pub low_pass_cutoff_hz: u32,
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(test)]
mod tests;
