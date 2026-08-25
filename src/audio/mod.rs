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
    pub playback_speed: usize,
    pub mute_during_fast_forward: bool,
    pub low_pass_enabled: bool,
    pub low_pass_cutoff_hz: u32,
}

fn copy_stereo_at_speed(samples: &[f32], speed: usize, output: &mut Vec<f32>) {
    output.clear();
    let speed = speed.max(1);
    let complete = &samples[..samples.len() & !1];
    if speed == 1 {
        output.extend_from_slice(complete);
        return;
    }

    output.reserve((complete.len() / 2).div_ceil(speed) * 2);
    for frame in complete.as_chunks::<2>().0.iter().step_by(speed) {
        output.extend_from_slice(frame);
    }
}

pub(crate) const DEFAULT_AUDIO_SAMPLE_RATE: u32 = 48_000;

#[cfg(not(target_arch = "wasm32"))]
#[cfg(test)]
mod tests;
