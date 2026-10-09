use super::*;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn browser_netplay_audio_pump_respects_pause_and_session_fences() {
    for locally_paused in [false, true] {
        for session_paused in [false, true] {
            for (phase, expected) in [
                (Phase::Idle, !locally_paused),
                (
                    Phase::Preparing(Request::Join {
                        invitation: String::new(),
                        scope: ConnectionScope::Loopback,
                        allow_different_versions: false,
                    }),
                    false,
                ),
                (Phase::Connecting, false),
                (Phase::Admission, false),
                (Phase::Running, !session_paused),
                (Phase::Stopping, false),
                (Phase::Poisoned, false),
            ] {
                let frontend = Frontend {
                    phase,
                    paused: session_paused,
                    ..Frontend::default()
                };
                assert_eq!(frontend.pumps_audio(locally_paused), expected);
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn netplay_auto_audio_budget_preserves_explicit_presets_and_pause_lifetime() {
    use crate::settings::AudioBufferPolicy;
    for paused in [false, true] {
        for phase in [
            Phase::Idle,
            Phase::Preparing(Request::Join {
                invitation: String::new(),
                scope: ConnectionScope::Loopback,
                allow_different_versions: false,
            }),
            Phase::Connecting,
            Phase::Admission,
            Phase::Running,
            Phase::Stopping,
            Phase::Poisoned,
        ] {
            let frontend = Frontend {
                phase,
                paused,
                ..Frontend::default()
            };
            for policy in [
                AudioBufferPolicy::Auto,
                AudioBufferPolicy::LowLatency,
                AudioBufferPolicy::Balanced,
                AudioBufferPolicy::Stable,
            ] {
                assert_eq!(
                    frontend.auto_audio_budget(policy),
                    frontend.running() && policy == AudioBufferPolicy::Auto
                );
            }
            if frontend.phase == Phase::Running {
                assert!(frontend.auto_audio_budget(AudioBufferPolicy::Auto));
            } else {
                assert!(!frontend.auto_audio_budget(AudioBufferPolicy::Auto));
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn netplay_auto_audio_budget_app_hooks_cover_pause_restore_poison_and_replacement() {
    use crate::settings::AudioBufferPolicy;
    let root = tempfile::tempdir().unwrap();
    let mut app = tests::app(root.path(), "audio-budget");
    app.settings.audio.buffer_policy = AudioBufferPolicy::Auto;
    app.settings.audio.volume = 1.0;
    app.settings.audio.low_pass_enabled = false;
    let original_settings = serde_json::to_value(&app.settings.audio).unwrap();
    let (output, mut driver): (_, crate::audio::AudioTestDriver) =
        crate::audio::AudioOutput::test_output(AudioBufferPolicy::Auto);
    app.audio = Some(output);
    app.netplay.phase = Phase::Admission;
    app.pump_emulator_audio();
    assert_eq!(driver.preroll(), 9_600);
    app.consume_netplay_response(EmuResponse::Netplay(Response::Ready));
    app.queue_emulator_audio(&[0.25; 30_000], 1);
    assert_eq!(driver.preroll(), 3_552);
    assert_eq!(driver.buffered(), 7_200);
    assert_eq!(driver.render(9_600), vec![0.0; 9_600]);
    app.pump_emulator_audio();
    assert_eq!(driver.preroll(), 9_600);
    app.consume_netplay_response(EmuResponse::Netplay(Response::Paused {
        frame: 0,
        local: true,
        peer: false,
    }));
    app.pump_emulator_audio();
    assert_eq!(driver.preroll(), 9_600);
    assert_eq!(driver.render(512), vec![0.0; 512]);
    let (mut replacement, replacement_driver) =
        crate::audio::AudioOutput::test_output(AudioBufferPolicy::Auto);
    replacement.inherit_netplay_buffer(app.audio.as_ref().unwrap());
    app.audio = Some(replacement);
    let mut driver = replacement_driver;
    app.consume_netplay_response(EmuResponse::Netplay(Response::Paused {
        frame: 0,
        local: false,
        peer: false,
    }));
    app.pump_emulator_audio();
    assert_eq!(driver.preroll(), 9_600);
    app.consume_netplay_response(EmuResponse::Netplay(Response::Stopped {
        reason: "restored".into(),
        restored: true,
    }));
    app.pump_emulator_audio();
    assert!(!app.netplay.fenced());
    assert_eq!(driver.preroll(), 9_600);
    app.netplay.phase = Phase::Admission;
    app.consume_netplay_response(EmuResponse::Netplay(Response::Ready));
    app.pump_emulator_audio();
    assert_eq!(driver.preroll(), 3_552);
    app.consume_netplay_response(EmuResponse::Netplay(Response::Stopped {
        reason: "failed restore".into(),
        restored: false,
    }));
    app.pump_emulator_audio();
    assert!(app.netplay.phase == Phase::Poisoned);
    assert_eq!(driver.preroll(), 9_600);
    assert_eq!(driver.render(512), vec![0.0; 512]);
    app.retire_netplay_frontend();
    app.pump_emulator_audio();
    assert!(!app.netplay.fenced());
    assert_eq!(
        serde_json::to_value(&app.settings.audio).unwrap(),
        original_settings
    );
    assert!(
        app.debug_windows
            .settings_ui
            .audio_host_status
            .buffer_fallback
            .is_none()
    );
    app.settings.audio.buffer_policy = AudioBufferPolicy::LowLatency;
    app.netplay.phase = Phase::Running;
    app.pump_emulator_audio();
    assert_eq!(driver.preroll(), 9_600);
}
