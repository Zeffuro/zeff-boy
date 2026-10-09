use std::{cell::RefCell, rc::Rc};

use wasm_bindgen::prelude::*;

use super::{AudioOutput, BUFFER_FRAMES, BrowserAudioDiagnostic};
use crate::audio::AudioQueueConfig;

#[cfg(test)]
mod pending;

#[cfg(test)]
#[wasm_bindgen_test::wasm_bindgen_test]
fn browser_netplay_audio_uses_requested_source_rate() {
    for rate in [44_100, 48_000] {
        let audio = AudioOutput::new(Some(rate)).unwrap();
        assert_eq!(audio.emulator_sample_rate(), rate);
        assert_eq!(audio.ctx.sample_rate(), rate as f32);
    }
}

#[cfg(test)]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn browser_netplay_audio_discard_and_speed_change_release_suspended_sources() {
    use wasm_bindgen_futures::JsFuture;

    let mut audio = AudioOutput::new(Some(48_000)).unwrap();
    JsFuture::from(audio.ctx.suspend().unwrap()).await.unwrap();
    audio.resume_requested = true;
    for _ in 0..128 {
        queue(&mut audio);
    }
    let bound = (super::MAX_QUEUE_AHEAD_SECS * audio.sample_rate as f64 / BUFFER_FRAMES as f64)
        .ceil() as usize
        + 1;
    assert!(audio.sources.len() > 1 && audio.sources.len() <= bound);
    assert!(audio.buffer.len() <= audio.sample_rate as usize * 2 / 5);
    audio.discard_queued_samples();
    audio.discard_queued_samples();
    assert_eq!(audio.sources.len(), 0);
    assert!(audio.buffer.is_empty() && audio.left.is_empty() && audio.right.is_empty());
    assert_eq!(audio.next_play_time, audio.ctx.current_time());
    audio.pump_queued_samples();
    assert_eq!(audio.sources.len(), 0);

    queue(&mut audio);
    assert_eq!(audio.sources.len(), 1);
    audio.queue_samples(
        &[],
        &AudioQueueConfig {
            master_volume: 1.0,
            playback_speed: 2,
            mute_during_fast_forward: true,
            low_pass_enabled: false,
            low_pass_cutoff_hz: 20_000,
        },
    );
    assert_eq!(audio.sources.len(), 0);
    assert!(audio.buffer.is_empty());
}

#[cfg(test)]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn browser_netplay_audio_full_source_queue_preserves_pending_pcm() {
    use wasm_bindgen_futures::JsFuture;

    let mut audio = AudioOutput::new(Some(48_000)).unwrap();
    JsFuture::from(audio.ctx.suspend().unwrap()).await.unwrap();
    audio.resume_requested = true;
    for _ in 0..128 {
        if audio.sources.is_full() {
            break;
        }
        let buffer = audio
            .ctx
            .create_buffer(2, BUFFER_FRAMES as u32, 48_000.0)
            .unwrap();
        let node = audio.ctx.create_buffer_source().unwrap();
        node.set_buffer(Some(&buffer));
        node.connect_with_audio_node(&audio.ctx.destination())
            .unwrap();
        audio.sources.start(node, 0.0).unwrap();
    }
    assert!(audio.sources.is_full());
    queue(&mut audio);
    assert_eq!(audio.buffer, samples());
    assert_eq!(audio.next_play_time, 0.0);
    assert_eq!(audio.scheduled_sources, 0);
    audio.sources.cancel();
    audio.pump_queued_samples();
    assert!(audio.buffer.is_empty());
    assert_eq!(audio.sources.len(), 1);
    assert_eq!(
        audio.left,
        samples().iter().step_by(2).copied().collect::<Vec<_>>()
    );
    assert_eq!(audio.right, audio.left);
    assert_eq!(audio.scheduled_sources, 1);
}

struct BrowserAudioTest {
    audio: Rc<RefCell<AudioOutput>>,
    button: web_sys::Element,
    click_listener: Closure<dyn FnMut()>,
}

thread_local! {
    static BROWSER_AUDIO_TEST: RefCell<Option<BrowserAudioTest>> = const { RefCell::new(None) };
}

fn samples() -> Vec<f32> {
    (0..BUFFER_FRAMES)
        .flat_map(|index| {
            let sample = if index & 1 == 0 { 0.25 } else { -0.25 };
            [sample, sample]
        })
        .collect()
}

fn queue(audio: &mut AudioOutput) {
    audio.queue_samples(
        &samples(),
        &AudioQueueConfig {
            master_volume: 1.0,
            playback_speed: 1,
            mute_during_fast_forward: false,
            low_pass_enabled: false,
            low_pass_cutoff_hz: 20_000,
        },
    );
}

#[wasm_bindgen(js_name = zeffAudioBrowserTestSetup)]
pub fn setup() -> Result<(), JsValue> {
    BROWSER_AUDIO_TEST.with(|slot| -> Result<(), JsValue> {
        if slot.borrow().is_some() {
            return Ok(());
        }
        let document = web_sys::window()
            .and_then(|window| window.document())
            .ok_or_else(|| JsValue::from_str("browser audio test requires a document"))?;
        let button = document.create_element("button")?;
        button.set_id("zeff-audio-activation");
        button.set_text_content(Some("Activate audio"));
        document
            .body()
            .ok_or_else(|| JsValue::from_str("browser audio test requires a body"))?
            .append_child(&button)?;

        let audio = Rc::new(RefCell::new(
            AudioOutput::new(None).map_err(|error| JsValue::from_str(&error.to_string()))?,
        ));
        queue(&mut audio.borrow_mut());
        let click_audio = Rc::clone(&audio);
        let click_listener = Closure::wrap(Box::new(move || {
            queue(&mut click_audio.borrow_mut());
        }) as Box<dyn FnMut()>);
        button
            .add_event_listener_with_callback("click", click_listener.as_ref().unchecked_ref())?;
        *slot.borrow_mut() = Some(BrowserAudioTest {
            audio,
            button,
            click_listener,
        });
        Ok(())
    })
}

#[wasm_bindgen(js_name = zeffAudioBrowserTestDiagnostic)]
pub fn diagnostic() -> Result<JsValue, JsValue> {
    BROWSER_AUDIO_TEST.with(|slot| {
        let test = slot.borrow();
        let diagnostic = test
            .as_ref()
            .ok_or_else(|| JsValue::from_str("browser audio test is not initialized"))?
            .audio
            .borrow()
            .browser_test_diagnostic();
        diagnostic_object(&diagnostic)
    })
}

fn diagnostic_object(diagnostic: &BrowserAudioDiagnostic) -> Result<JsValue, JsValue> {
    let result = js_sys::Object::new();
    for (name, value) in [
        ("contextState", JsValue::from_str(&diagnostic.context_state)),
        ("currentTime", JsValue::from_f64(diagnostic.current_time)),
        (
            "scheduledSources",
            JsValue::from_f64(diagnostic.scheduled_sources as f64),
        ),
        (
            "activationResumeAttempts",
            JsValue::from_f64(diagnostic.activation_resume_attempts as f64),
        ),
        (
            "fallbackResumeAttempts",
            JsValue::from_f64(diagnostic.fallback_resume_attempts as f64),
        ),
    ] {
        js_sys::Reflect::set(&result, &JsValue::from_str(name), &value)?;
    }
    Ok(result.into())
}

impl Drop for BrowserAudioTest {
    fn drop(&mut self) {
        let _ = self.button.remove_event_listener_with_callback(
            "click",
            self.click_listener.as_ref().unchecked_ref(),
        );
        self.button.remove();
    }
}
