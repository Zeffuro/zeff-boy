use std::{cell::RefCell, rc::Rc};

use wasm_bindgen::prelude::*;

use super::{AudioOutput, BUFFER_FRAMES, BrowserAudioDiagnostic};
use crate::audio::AudioQueueConfig;

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
