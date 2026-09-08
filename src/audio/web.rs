use super::AudioQueueConfig;
#[cfg(feature = "wasm-browser-tests")]
use std::{cell::Cell, rc::Rc};
use wasm_bindgen::prelude::*;
use web_sys::AudioContext;

pub(super) const BUFFER_FRAMES: usize = 1024;

const MAX_QUEUE_AHEAD_SECS: f64 = 0.12;

const CATCHUP_OFFSET_SECS: f64 = 0.010;

fn context_state(ctx: &AudioContext) -> Option<String> {
    js_sys::Reflect::get(ctx.as_ref(), &JsValue::from_str("state"))
        .ok()
        .and_then(|state| state.as_string())
}

fn context_needs_resume(ctx: &AudioContext) -> bool {
    matches!(
        context_state(ctx).as_deref(),
        Some("suspended" | "interrupted")
    )
}

fn consume_audio_promise(promise: Result<js_sys::Promise, JsValue>) {
    if let Ok(promise) = promise {
        wasm_bindgen_futures::spawn_local(async move {
            let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
        });
    }
}

fn resume_if_needed(ctx: &AudioContext) -> bool {
    if !context_needs_resume(ctx) {
        return false;
    }
    consume_audio_promise(ctx.resume());
    true
}

#[cfg(feature = "wasm-browser-tests")]
pub(crate) struct BrowserAudioDiagnostic {
    pub context_state: String,
    pub current_time: f64,
    pub scheduled_sources: u64,
    pub activation_resume_attempts: u64,
    pub fallback_resume_attempts: u64,
}

#[cfg(feature = "wasm-browser-tests")]
mod browser_tests;

pub(crate) struct AudioOutput {
    ctx: AudioContext,
    activation_target: Option<web_sys::Window>,
    activation_listeners: Vec<(&'static str, Closure<dyn FnMut()>)>,
    resume_requested: bool,
    #[cfg(feature = "wasm-browser-tests")]
    activation_resume_attempts: Rc<Cell<u64>>,
    #[cfg(feature = "wasm-browser-tests")]
    fallback_resume_attempts: u64,
    #[cfg(feature = "wasm-browser-tests")]
    scheduled_sources: u64,
    sample_rate: u32,
    buffer: Vec<f32>,
    next_play_time: f64,
    left: Vec<f32>,
    right: Vec<f32>,
    playback_speed: usize,
}

impl AudioOutput {
    pub(crate) fn new(_preferred_sample_rate: Option<u32>) -> anyhow::Result<Self> {
        let ctx = AudioContext::new()
            .map_err(|e| anyhow::anyhow!("failed to create AudioContext: {e:?}"))?;

        let sample_rate = ctx.sample_rate() as u32;
        let activation_target = web_sys::window();
        let mut activation_listeners = Vec::new();
        #[cfg(feature = "wasm-browser-tests")]
        let activation_resume_attempts = Rc::new(Cell::new(0));

        if let Some(window) = &activation_target {
            for event_name in ["pointerdown", "pointerup", "keydown", "touchend"] {
                let resume_ctx = ctx.clone();
                #[cfg(feature = "wasm-browser-tests")]
                let resume_attempts = Rc::clone(&activation_resume_attempts);
                let listener = Closure::wrap(Box::new(move || {
                    // A blocked context can resume only during browser user activation.
                    if context_needs_resume(&resume_ctx) {
                        #[cfg(feature = "wasm-browser-tests")]
                        resume_attempts.set(resume_attempts.get() + 1);
                        consume_audio_promise(resume_ctx.resume());
                    }
                }) as Box<dyn FnMut()>);
                if window
                    .add_event_listener_with_callback_and_bool(
                        event_name,
                        listener.as_ref().unchecked_ref(),
                        true,
                    )
                    .is_ok()
                {
                    activation_listeners.push((event_name, listener));
                }
            }
        }

        Ok(Self {
            ctx,
            activation_target,
            activation_listeners,
            resume_requested: false,
            #[cfg(feature = "wasm-browser-tests")]
            activation_resume_attempts,
            #[cfg(feature = "wasm-browser-tests")]
            fallback_resume_attempts: 0,
            #[cfg(feature = "wasm-browser-tests")]
            scheduled_sources: 0,
            sample_rate,
            buffer: Vec::with_capacity(BUFFER_FRAMES * 4),
            next_play_time: 0.0,
            left: Vec::with_capacity(BUFFER_FRAMES),
            right: Vec::with_capacity(BUFFER_FRAMES),
            playback_speed: 1,
        })
    }

    pub(crate) fn emulator_sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub(crate) fn discard_queued_samples(&mut self) {
        self.buffer.clear();
        self.left.clear();
        self.right.clear();
        self.next_play_time = self.ctx.current_time();
    }

    pub(crate) fn queue_samples(&mut self, samples: &[f32], config: &AudioQueueConfig) {
        if !self.resume_requested {
            if context_needs_resume(&self.ctx) {
                #[cfg(feature = "wasm-browser-tests")]
                {
                    self.fallback_resume_attempts += 1;
                }
            }
            resume_if_needed(&self.ctx);
            self.resume_requested = true;
        }

        let playback_speed = config.playback_speed.max(1);
        if playback_speed != self.playback_speed {
            self.discard_queued_samples();
            self.playback_speed = playback_speed;
        }

        if playback_speed > 1 && config.mute_during_fast_forward {
            self.buffer.clear();
            return;
        }

        let gain = config.master_volume.clamp(0.0, 1.0);

        for frame in samples[..samples.len() & !1]
            .as_chunks::<2>()
            .0
            .iter()
            .step_by(playback_speed)
        {
            self.buffer.push(frame[0] * gain);
            self.buffer.push(frame[1] * gain);
        }

        while self.buffer.len() >= BUFFER_FRAMES * 2 {
            let current_time = self.ctx.current_time();

            if self.next_play_time < current_time {
                self.next_play_time = current_time + CATCHUP_OFFSET_SECS;
            }

            if self.next_play_time > current_time + MAX_QUEUE_AHEAD_SECS {
                break;
            }

            let Ok(audio_buffer) =
                self.ctx
                    .create_buffer(2, BUFFER_FRAMES as u32, self.sample_rate as f32)
            else {
                self.buffer.drain(..BUFFER_FRAMES * 2);
                continue;
            };

            self.left.clear();
            self.right.clear();
            for pair in self.buffer[..BUFFER_FRAMES * 2].as_chunks::<2>().0 {
                self.left.push(pair[0]);
                self.right.push(pair[1]);
            }
            self.buffer.drain(..BUFFER_FRAMES * 2);

            let _ = audio_buffer.copy_to_channel(&self.left, 0);
            let _ = audio_buffer.copy_to_channel(&self.right, 1);

            if let Ok(source) = self.ctx.create_buffer_source() {
                source.set_buffer(Some(&audio_buffer));
                let _ = source.connect_with_audio_node(&self.ctx.destination());
                if source.start_with_when(self.next_play_time).is_ok() {
                    #[cfg(feature = "wasm-browser-tests")]
                    {
                        self.scheduled_sources += 1;
                    }
                }
            }

            self.next_play_time += BUFFER_FRAMES as f64 / self.sample_rate as f64;
        }

        let max_buffered = self.sample_rate as usize * 2 * 200 / 1000;
        if self.buffer.len() > max_buffered {
            let excess = self.buffer.len() - max_buffered;
            let drop = excess & !1;
            self.buffer.drain(..drop);
        }
    }

    #[cfg(feature = "wasm-browser-tests")]
    pub(crate) fn browser_test_diagnostic(&self) -> BrowserAudioDiagnostic {
        BrowserAudioDiagnostic {
            context_state: context_state(&self.ctx).unwrap_or_default(),
            current_time: self.ctx.current_time(),
            scheduled_sources: self.scheduled_sources,
            activation_resume_attempts: self.activation_resume_attempts.get(),
            fallback_resume_attempts: self.fallback_resume_attempts,
        }
    }
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        if let Some(window) = &self.activation_target {
            for (event_name, listener) in &self.activation_listeners {
                let _ = window.remove_event_listener_with_callback_and_bool(
                    event_name,
                    listener.as_ref().unchecked_ref(),
                    true,
                );
            }
        }
        consume_audio_promise(self.ctx.close());
    }
}
