use super::*;
use crate::emu_thread::contract_tests::{
    SMS_AUDIO_ROM, assert_active_audio_results_match, assert_gba_results_match, gba_rtc,
    gba_sram_bytes, gba_test_rom,
};
use crate::emu_thread::{
    AudioConfig, AudioRecordingCapture, JoypadInput, MemorySearchRequest, RenderSettings,
    ReusableBuffers, SnapshotRequest, SpeculationBlockers, ZapperInput,
};
use wasm_bindgen_test::wasm_bindgen_test;

#[cfg(feature = "wasm-browser-tests")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
export function browser_test_delay(milliseconds) {
    return new Promise((resolve) => setTimeout(resolve, milliseconds));
}
")]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(catch)]
    async fn browser_test_delay(
        milliseconds: u32,
    ) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>;
}

struct PrimaryObservation {
    state: Vec<u8>,
    framebuffer: Vec<u8>,
    residual_audio: Vec<f32>,
    battery_hash: [u8; 32],
    battery_bytes: Option<Vec<u8>>,
    gba_rtc: Option<zeff_gba_core::hardware::cartridge::RtcDateTime>,
    potentially_dirty: bool,
}

fn sms_thread() -> EmuThread {
    sms_thread_with_recovery(false)
}

fn sms_thread_with_recovery(save_recovery_on_shutdown: bool) -> EmuThread {
    let mut emu = zeff_sega8_core::emulator::Emulator::new_with_hint(
        SMS_AUDIO_ROM,
        44_100,
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
    )
    .unwrap();
    emu.set_instruction_trace_enabled(true);
    let backend = EmuBackend::from_sega8(emu, PathBuf::from("wasm-test.sms"));
    assert!(!backend.save_ram_kind().is_battery_backed());
    EmuThread::spawn(backend, save_recovery_on_shutdown)
}

fn gba_thread(save_recovery_on_shutdown: bool) -> EmuThread {
    let mut emu = zeff_gba_core::emulator::Emulator::new(&gba_test_rom(), 44_100).unwrap();
    let sram = gba_sram_bytes(emu.dump_battery_sram().unwrap().len());
    emu.load_battery_sram(&sram).unwrap();
    assert!(emu.set_rtc_date_time(gba_rtc()));
    emu.set_instruction_trace_enabled(true);
    let backend = EmuBackend::from_gba(emu, PathBuf::from("wasm-emerald-sram.gba"));
    assert!(backend.save_ram_kind().is_battery_backed());
    EmuThread::spawn(backend, save_recovery_on_shutdown)
}

fn frame_input() -> FrameInput {
    FrameInput {
        frames: 1,
        speculation_blockers: SpeculationBlockers::from_app_for_test(false, false),
        replay_joypad_frames: None,
        host_tilt: (0.0, 0.0),
        host_camera_frame: None,
        joypad: JoypadInput {
            buttons: 0x01,
            dpad: 0x02,
            buttons_p2: 0,
            dpad_p2: 0,
            buttons_p3: 0,
            dpad_p3: 0,
            buttons_p4: 0,
            dpad_p4: 0,
            buttons_p5: 0,
            dpad_p5: 0,
        },
        pce_mouse: Default::default(),
        zapper: ZapperInput::default(),
        debug_step: false,
        debug_continue: false,
        debug_suspend_after_frame: false,
        audio: AudioConfig {
            apu_capture_enabled: true,
            skip_audio: false,
            playback_speed: 1,
            recording_capture: AudioRecordingCapture {
                active: true,
                semantic: true,
            },
        },
        debug_actions: crate::debug::DebugUiActions::none(),
        snapshot: SnapshotRequest {
            want_debug_info: true,
            want_perf_info: true,
            any_viewer_open: true,
            any_vram_viewer_open: true,
            show_oam_viewer: true,
            show_apu_viewer: true,
            show_disassembler: true,
            show_rom_info: true,
            show_memory_viewer: true,
            memory_view_start: 0,
            show_rom_viewer: true,
            show_instruction_trace: true,
            trace_after_sequence: None,
            rom_view_start: 0,
            last_disasm_pc: None,
            last_disasm_mapping: None,
            disasm_target: None,
            memory_search: Some(MemorySearchRequest {
                pattern: vec![0x3E, 0x80, 0xD3, 0x7F],
                max_results: 4,
            }),
            rom_search: Some(MemorySearchRequest {
                pattern: vec![0x3E, 0x80, 0xD3, 0x7F],
                max_results: 4,
            }),
            render: RenderSettings {
                color_correction: crate::settings::ColorCorrection::None,
                color_correction_matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                dmg_palette_preset: crate::settings::DmgPalettePreset::default(),
                nes_palette_mode: crate::settings::NesPaletteMode::default(),
                nes_custom_palette: None,
                pce_overscan_mode: crate::settings::PceOverscanMode::default(),
                pce_palette_mode: crate::settings::PcePaletteMode::default(),
                sgb_border_enabled: false,
            },
        },
        buffers: ReusableBuffers {
            audio: None,
            vram: None,
            oam: None,
            memory_page: None,
            nes_chr: None,
            nes_nametable: None,
        },
        rewind_enabled: false,
        rewind_seconds: 10,
    }
}

fn gba_frame_input() -> FrameInput {
    let mut input = frame_input();
    input.snapshot.memory_view_start = 0x0200_0000;
    input.snapshot.memory_search = Some(MemorySearchRequest {
        pattern: vec![0, 0, 0, 0],
        max_results: 4,
    });
    input.snapshot.rom_search = Some(MemorySearchRequest {
        pattern: b"BPEE".to_vec(),
        max_results: 4,
    });
    input
}

fn observe_primary(thread: &EmuThread) -> PrimaryObservation {
    let mut inner = thread.inner.borrow_mut();
    let mut residual_audio = Vec::new();
    inner.backend.drain_audio_samples_into(&mut residual_audio);
    let (battery_bytes, gba_rtc) = match &inner.backend {
        EmuBackend::Gba(backend) => (backend.emu.dump_battery_sram(), backend.emu.rtc_date_time()),
        _ => (None, None),
    };
    PrimaryObservation {
        state: inner.backend.encode_state_bytes().unwrap(),
        framebuffer: inner.backend.framebuffer().to_vec(),
        residual_audio,
        battery_hash: inner.backend.battery_component_hash(),
        battery_bytes,
        gba_rtc,
        potentially_dirty: inner.battery_potentially_dirty,
    }
}

fn assert_primary_matches(left: &PrimaryObservation, right: &PrimaryObservation) {
    assert_eq!(left.state, right.state);
    assert_eq!(left.framebuffer, right.framebuffer);
    assert_eq!(left.residual_audio, right.residual_audio);
    assert!(left.residual_audio.is_empty());
    assert_eq!(left.battery_hash, right.battery_hash);
    assert_eq!(left.battery_bytes, right.battery_bytes);
    assert_eq!(left.gba_rtc, right.gba_rtc);
    assert_eq!(left.potentially_dirty, right.potentially_dirty);
}

#[wasm_bindgen_test]
fn wasm_sms_detached_stepframes_matches_control_and_selects_projection() {
    let control = sms_thread();
    let subject = sms_thread();
    subject
        .inner
        .borrow_mut()
        .speculation
        .force_frames_for_test(1);

    control.send(EmuCommand::StepFrames(Box::new(frame_input())));
    subject.send(EmuCommand::StepFrames(Box::new(frame_input())));

    {
        let inner = control.inner.borrow();
        assert_eq!(inner.speculation.committed_frames_for_test(), 1);
    }
    {
        let inner = subject.inner.borrow();
        assert_eq!(inner.speculation.completed_runs_for_test(), 1);
        assert_eq!(inner.speculation.committed_frames_for_test(), 1);
    }
    let control_result = control.try_recv_frame().unwrap();
    let subject_result = subject.try_recv_frame().unwrap();
    assert_active_audio_results_match(&control_result, &subject_result);

    let expected_projection = {
        let inner = control.inner.borrow();
        let mut detached = inner.backend.fork_detached_for_speculation().unwrap();
        detached.disable_audio_output();
        assert!(detached.step_frames(1));
        detached.framebuffer().to_vec()
    };
    let control_primary = observe_primary(&control);
    let subject_primary = observe_primary(&subject);
    assert_primary_matches(&control_primary, &subject_primary);
    assert_eq!(
        subject.shared_framebuffer.load_full().unwrap().as_slice(),
        expected_projection.as_slice()
    );
    assert_eq!(
        control.shared_framebuffer.load_full().unwrap().as_slice(),
        control_primary.framebuffer.as_slice()
    );
}

fn assert_detached_fallback(wrong_framebuffer_len: bool) {
    let control = sms_thread();
    let subject = sms_thread();
    {
        let mut inner = subject.inner.borrow_mut();
        inner.speculation.force_frames_for_test(1);
        if wrong_framebuffer_len {
            inner.speculation.force_wrong_framebuffer_len_for_test();
        } else {
            inner.speculation.force_operational_failure_for_test();
        }
    }

    control.send(EmuCommand::StepFrames(Box::new(frame_input())));
    subject.send(EmuCommand::StepFrames(Box::new(frame_input())));

    {
        let inner = control.inner.borrow();
        assert_eq!(inner.speculation.committed_frames_for_test(), 1);
    }
    {
        let inner = subject.inner.borrow();
        assert_eq!(inner.speculation.completed_runs_for_test(), 0);
        assert_eq!(inner.speculation.committed_frames_for_test(), 1);
    }
    let control_result = control.try_recv_frame().unwrap();
    let subject_result = subject.try_recv_frame().unwrap();
    assert_active_audio_results_match(&control_result, &subject_result);
    let control_primary = observe_primary(&control);
    let subject_primary = observe_primary(&subject);
    assert_primary_matches(&control_primary, &subject_primary);
    assert_eq!(
        subject.shared_framebuffer.load_full().unwrap().as_slice(),
        subject_primary.framebuffer.as_slice()
    );
}

#[wasm_bindgen_test]
fn wasm_sms_detached_stepframes_falls_back_on_operational_or_size_failure() {
    assert_detached_fallback(false);
    assert_detached_fallback(true);
}

#[wasm_bindgen_test]
fn wasm_gba_detached_stepframes_matches_control_and_selects_projection() {
    let control = gba_thread(false);
    let subject = gba_thread(false);
    subject
        .inner
        .borrow_mut()
        .speculation
        .force_frames_for_test(1);

    control.send(EmuCommand::StepFrames(Box::new(gba_frame_input())));
    subject.send(EmuCommand::StepFrames(Box::new(gba_frame_input())));
    assert_eq!(
        control
            .inner
            .borrow()
            .speculation
            .committed_frames_for_test(),
        1
    );
    assert_eq!(
        subject.inner.borrow().speculation.completed_runs_for_test(),
        1
    );
    assert_eq!(
        subject
            .inner
            .borrow()
            .speculation
            .committed_frames_for_test(),
        1
    );
    let control_result = control.try_recv_frame().unwrap();
    let subject_result = subject.try_recv_frame().unwrap();
    assert_gba_results_match(&control_result, &subject_result);

    let expected_projection = {
        let inner = control.inner.borrow();
        let mut detached = inner.backend.fork_detached_for_speculation().unwrap();
        detached.disable_audio_output();
        assert!(detached.step_frames(1));
        detached.framebuffer().to_vec()
    };
    let control_primary = observe_primary(&control);
    let subject_primary = observe_primary(&subject);
    assert_primary_matches(&control_primary, &subject_primary);
    assert!(control_primary.potentially_dirty);
    assert_eq!(control_primary.gba_rtc, Some(gba_rtc()));
    assert_eq!(subject_primary.gba_rtc, Some(gba_rtc()));
    let battery = control_primary.battery_bytes.as_ref().unwrap();
    assert_eq!(battery, &gba_sram_bytes(battery.len()));
    assert_eq!(
        subject.shared_framebuffer.load_full().unwrap().as_slice(),
        expected_projection.as_slice()
    );
    assert_eq!(
        control.shared_framebuffer.load_full().unwrap().as_slice(),
        control_primary.framebuffer.as_slice()
    );
}

fn assert_gba_detached_fallback(wrong_framebuffer_len: bool) {
    let control = gba_thread(false);
    let subject = gba_thread(false);
    {
        let mut inner = subject.inner.borrow_mut();
        inner.speculation.force_frames_for_test(1);
        if wrong_framebuffer_len {
            inner.speculation.force_wrong_framebuffer_len_for_test();
        } else {
            inner.speculation.force_operational_failure_for_test();
        }
    }
    control.send(EmuCommand::StepFrames(Box::new(gba_frame_input())));
    subject.send(EmuCommand::StepFrames(Box::new(gba_frame_input())));
    assert_eq!(
        control
            .inner
            .borrow()
            .speculation
            .committed_frames_for_test(),
        1
    );
    assert_eq!(
        subject.inner.borrow().speculation.completed_runs_for_test(),
        0
    );
    assert_eq!(
        subject
            .inner
            .borrow()
            .speculation
            .committed_frames_for_test(),
        1
    );
    let control_result = control.try_recv_frame().unwrap();
    let subject_result = subject.try_recv_frame().unwrap();
    assert_gba_results_match(&control_result, &subject_result);
    let control_primary = observe_primary(&control);
    let subject_primary = observe_primary(&subject);
    assert_primary_matches(&control_primary, &subject_primary);
    assert!(control_primary.potentially_dirty);
    assert_eq!(control_primary.gba_rtc, Some(gba_rtc()));
    assert_eq!(subject_primary.gba_rtc, Some(gba_rtc()));
    let battery = control_primary.battery_bytes.as_ref().unwrap();
    assert_eq!(battery, &gba_sram_bytes(battery.len()));
    assert_eq!(
        subject.shared_framebuffer.load_full().unwrap().as_slice(),
        subject_primary.framebuffer.as_slice()
    );
}

#[wasm_bindgen_test]
fn wasm_gba_detached_stepframes_falls_back_on_operational_or_size_failure() {
    assert_gba_detached_fallback(false);
    assert_gba_detached_fallback(true);
}

fn capture_terminal_writes(
    thread: &EmuThread,
) -> (CapturedBatteryWrites, Vec<crate::platform::SaveWrite>) {
    let mut inner = thread.inner.borrow_mut();
    let Inner {
        backend,
        recovery,
        speculation,
        pending_storage,
        ..
    } = &mut *inner;
    assert!(pending_storage.is_none());
    let ready = speculation.prepare_terminal_persistence();
    EmuThread::capture_terminal_save_writes(ready, backend, recovery, true).unwrap()
}

#[wasm_bindgen_test]
fn wasm_sms_detached_terminal_capture_matches_control_without_committing_browser_storage() {
    let control = sms_thread_with_recovery(true);
    let subject = sms_thread_with_recovery(true);
    subject
        .inner
        .borrow_mut()
        .speculation
        .force_frames_for_test(1);

    control.send(EmuCommand::StepFrames(Box::new(frame_input())));
    subject.send(EmuCommand::StepFrames(Box::new(frame_input())));
    let control_result = control.try_recv_frame().unwrap();
    let subject_result = subject.try_recv_frame().unwrap();
    assert_active_audio_results_match(&control_result, &subject_result);
    let control_primary = observe_primary(&control);
    let subject_primary = observe_primary(&subject);
    assert_primary_matches(&control_primary, &subject_primary);

    let (control_capture, control_writes) = capture_terminal_writes(&control);
    let (subject_capture, subject_writes) = capture_terminal_writes(&subject);
    let write_parts = |writes: &[crate::platform::SaveWrite]| {
        writes
            .iter()
            .map(|write| {
                let (key, data) = write.parts_for_test();
                (key.to_owned(), data.to_vec())
            })
            .collect::<Vec<_>>()
    };
    let control_parts = write_parts(&control_writes);
    let subject_parts = write_parts(&subject_writes);
    assert_eq!(control_parts, subject_parts);
    assert_eq!(control_parts.len(), 2);
    assert!(control_parts.iter().all(|(key, _)| !key.ends_with(".sav")));
    assert!(control_capture.path.is_none());
    assert!(subject_capture.path.is_none());
    assert_eq!(control_capture.generation, subject_capture.generation);
    assert!(control_capture.recovery_path.is_some());
    assert_eq!(control_capture.recovery_path, subject_capture.recovery_path);

    let (system, discriminator, media_sha256, component_sha256) = {
        let inner = control.inner.borrow();
        (
            inner.backend.system().storage_subdir().to_owned(),
            inner.backend.recovery_discriminator(),
            inner.backend.rom_hash(),
            inner.backend.battery_component_hash(),
        )
    };
    let generation = crate::save_paths::recovery_state::decode_battery_generation(
        &control_parts[0].1,
        media_sha256,
    )
    .expect("terminal batch should contain a battery generation witness");
    assert_eq!(generation, control_capture.generation);
    assert_eq!(generation.component_sha256, component_sha256);
    assert_eq!(
        generation.component_sha256,
        crate::save_paths::recovery_state::canonical_battery_component_hash(&[])
    );
    let envelope = crate::save_paths::recovery_state::decode_recovery_state(
        &control_parts[1].1,
        crate::save_paths::recovery_state::RecoveryStateIdentity {
            system: &system,
            discriminator: &discriminator,
            media_sha256,
        },
    )
    .expect("terminal batch should contain a recovery-state envelope");
    assert_eq!(envelope.system, system);
    assert_eq!(envelope.discriminator, discriminator);
    assert_eq!(envelope.media_sha256, media_sha256);
    assert_eq!(
        envelope.battery,
        crate::save_paths::recovery_state::BatteryGenerationWitness::Committed {
            generation: generation.generation,
            component_sha256,
        }
    );
    assert_eq!(envelope.native_payload, control_primary.state);
    assert!(control.inner.borrow().pending_storage.is_none());
    assert!(subject.inner.borrow().pending_storage.is_none());
    assert_eq!(
        control.inner.borrow().speculation.completed_runs_for_test(),
        0
    );
    assert_eq!(
        subject.inner.borrow().speculation.completed_runs_for_test(),
        1
    );
}

#[wasm_bindgen_test]
fn wasm_gba_detached_terminal_capture_matches_control_without_committing_browser_storage() {
    let control = gba_thread(true);
    let subject = gba_thread(true);
    subject
        .inner
        .borrow_mut()
        .speculation
        .force_frames_for_test(1);

    control.send(EmuCommand::StepFrames(Box::new(gba_frame_input())));
    subject.send(EmuCommand::StepFrames(Box::new(gba_frame_input())));
    let control_result = control.try_recv_frame().unwrap();
    let subject_result = subject.try_recv_frame().unwrap();
    assert_gba_results_match(&control_result, &subject_result);
    let control_primary = observe_primary(&control);
    let subject_primary = observe_primary(&subject);
    assert_primary_matches(&control_primary, &subject_primary);
    assert!(control_primary.potentially_dirty);
    assert_eq!(control_primary.gba_rtc, Some(gba_rtc()));

    let (control_capture, control_writes) = capture_terminal_writes(&control);
    let (subject_capture, subject_writes) = capture_terminal_writes(&subject);
    let write_parts = |writes: &[crate::platform::SaveWrite]| {
        writes
            .iter()
            .map(|write| {
                let (key, data) = write.parts_for_test();
                (key.to_owned(), data.to_vec())
            })
            .collect::<Vec<_>>()
    };
    let control_parts = write_parts(&control_writes);
    let subject_parts = write_parts(&subject_writes);
    assert_eq!(control_parts, subject_parts);
    assert_eq!(control_parts.len(), 3);
    assert!(control_capture.path.is_some());
    assert_eq!(control_capture.path, subject_capture.path);
    assert_eq!(control_capture.generation, subject_capture.generation);
    assert!(control_capture.recovery_path.is_some());
    assert_eq!(control_capture.recovery_path, subject_capture.recovery_path);
    assert_eq!(
        control_parts[0].1,
        control_primary.battery_bytes.clone().unwrap()
    );

    let (system, discriminator, media_sha256, component_sha256) = {
        let inner = control.inner.borrow();
        (
            inner.backend.system().storage_subdir().to_owned(),
            inner.backend.recovery_discriminator(),
            inner.backend.rom_hash(),
            inner.backend.battery_component_hash(),
        )
    };
    let generation = crate::save_paths::recovery_state::decode_battery_generation(
        &control_parts[1].1,
        media_sha256,
    )
    .expect("terminal batch should contain a battery generation witness");
    assert_eq!(generation, control_capture.generation);
    assert_eq!(generation.component_sha256, component_sha256);
    let envelope = crate::save_paths::recovery_state::decode_recovery_state(
        &control_parts[2].1,
        crate::save_paths::recovery_state::RecoveryStateIdentity {
            system: &system,
            discriminator: &discriminator,
            media_sha256,
        },
    )
    .expect("terminal batch should contain a recovery-state envelope");
    assert_eq!(envelope.system, system);
    assert_eq!(envelope.discriminator, discriminator);
    assert_eq!(envelope.media_sha256, media_sha256);
    assert_eq!(
        envelope.battery,
        crate::save_paths::recovery_state::BatteryGenerationWitness::Committed {
            generation: generation.generation,
            component_sha256,
        }
    );
    assert_eq!(envelope.native_payload, control_primary.state);
    assert!(control.inner.borrow().pending_storage.is_none());
    assert!(subject.inner.borrow().pending_storage.is_none());
    assert_eq!(
        control.inner.borrow().speculation.completed_runs_for_test(),
        0
    );
    assert_eq!(
        subject.inner.borrow().speculation.completed_runs_for_test(),
        1
    );
}

#[cfg(feature = "wasm-browser-tests")]
struct BrowserObservation {
    frame_result: FrameResult,
    primary: PrimaryObservation,
    terminal_responses: Vec<String>,
    stored_entries: Vec<(String, Vec<u8>)>,
}

#[cfg(feature = "wasm-browser-tests")]
async fn wait_for_browser_terminal(thread: &EmuThread) -> Vec<String> {
    let deadline = js_sys::Date::now() + 10_000.0;
    let mut responses = Vec::new();
    loop {
        while let Some(response) = thread.try_recv_response() {
            match response {
                EmuResponse::SramFlushed(path) => {
                    responses.push(format!("sram:{}", path.unwrap_or_default()));
                }
                EmuResponse::RecoverySaved(path) => {
                    responses.push(format!("recovery:{}", path.display()));
                }
                EmuResponse::ShutdownComplete => {
                    responses.push("shutdown".to_string());
                    assert_eq!(responses.len(), 3);
                    assert!(responses[0].starts_with("sram:"));
                    assert!(responses[1].starts_with("recovery:"));
                    assert_eq!(responses[2], "shutdown");
                    assert!(thread.try_recv_response().is_none());
                    return responses;
                }
                _ => panic!("unexpected response during browser shutdown"),
            }
        }
        assert!(
            js_sys::Date::now() < deadline,
            "browser persistence timed out"
        );
        browser_test_delay(5)
            .await
            .expect("browser timer should resolve");
    }
}

#[cfg(feature = "wasm-browser-tests")]
fn assert_browser_gba_entries(
    thread: &EmuThread,
    primary: &PrimaryObservation,
    entries: &[(String, Vec<u8>)],
) {
    assert_eq!(entries.len(), 3);
    let battery = primary
        .battery_bytes
        .as_ref()
        .expect("GBA battery bytes should be present");
    assert_eq!(
        entries
            .iter()
            .filter(|(_, data)| data.as_slice() == battery.as_slice())
            .count(),
        1
    );

    let inner = thread.inner.borrow();
    let media_sha256 = inner.backend.rom_hash();
    let component_sha256 = inner.backend.battery_component_hash();
    let system = inner.backend.system().storage_subdir().to_owned();
    let discriminator = inner.backend.recovery_discriminator();
    let generations = entries
        .iter()
        .filter_map(|(_, data)| {
            crate::save_paths::recovery_state::decode_battery_generation(data, media_sha256)
        })
        .collect::<Vec<_>>();
    assert_eq!(generations.len(), 1);
    assert_eq!(generations[0].component_sha256, component_sha256);
    let envelopes = entries
        .iter()
        .filter_map(|(_, data)| {
            crate::save_paths::recovery_state::decode_recovery_state(
                data,
                crate::save_paths::recovery_state::RecoveryStateIdentity {
                    system: &system,
                    discriminator: &discriminator,
                    media_sha256,
                },
            )
            .ok()
        })
        .collect::<Vec<_>>();
    assert_eq!(envelopes.len(), 1);
    assert_eq!(envelopes[0].system, system);
    assert_eq!(envelopes[0].discriminator, discriminator);
    assert_eq!(envelopes[0].media_sha256, media_sha256);
    assert_eq!(
        envelopes[0].battery,
        crate::save_paths::recovery_state::BatteryGenerationWitness::Committed {
            generation: generations[0].generation,
            component_sha256,
        }
    );
    assert_eq!(envelopes[0].native_payload, primary.state);
}

#[cfg(feature = "wasm-browser-tests")]
async fn run_browser_gba_transaction(force_detached: bool) -> BrowserObservation {
    crate::platform::clear_browser_storage_for_test()
        .await
        .expect("browser storage should clear");
    let thread = gba_thread(true);
    if force_detached {
        thread
            .inner
            .borrow_mut()
            .speculation
            .force_frames_for_test(1);
    }

    thread.send(EmuCommand::StepFrames(Box::new(gba_frame_input())));
    let frame_result = thread
        .try_recv_frame()
        .expect("StepFrames should publish one result");
    let expected_projection = force_detached.then(|| {
        let inner = thread.inner.borrow();
        let mut detached = inner.backend.fork_detached_for_speculation().unwrap();
        detached.disable_audio_output();
        assert!(detached.step_frames(1));
        detached.framebuffer().to_vec()
    });
    let primary = observe_primary(&thread);
    assert!(primary.potentially_dirty);
    assert_eq!(primary.gba_rtc, Some(gba_rtc()));
    let battery = primary.battery_bytes.as_ref().unwrap();
    assert_eq!(battery, &gba_sram_bytes(battery.len()));
    assert_eq!(
        thread
            .inner
            .borrow()
            .speculation
            .committed_frames_for_test(),
        1
    );
    assert_eq!(
        thread.inner.borrow().speculation.completed_runs_for_test(),
        if force_detached { 1 } else { 0 }
    );
    let published = thread.shared_framebuffer.load_full().unwrap();
    if let Some(expected_projection) = expected_projection {
        assert_eq!(published.as_slice(), expected_projection.as_slice());
    } else {
        assert_eq!(published.as_slice(), primary.framebuffer.as_slice());
    }

    thread.send(EmuCommand::Shutdown);
    assert!(thread.inner.borrow().pending_storage.is_some());
    assert!(thread.try_recv_response().is_none());
    let terminal_responses = wait_for_browser_terminal(&thread).await;
    assert_eq!(
        thread.inner.borrow().speculation.completed_runs_for_test(),
        if force_detached { 1 } else { 0 }
    );
    assert!(thread.inner.borrow().pending_storage.is_none());
    let stored_entries = crate::platform::fresh_browser_storage_entries_for_test()
        .await
        .expect("production IndexedDB reload should match stored entries");
    assert_browser_gba_entries(&thread, &primary, &stored_entries);

    BrowserObservation {
        frame_result,
        primary,
        terminal_responses,
        stored_entries,
    }
}

#[cfg(feature = "wasm-browser-tests")]
#[wasm_bindgen_test]
async fn wasm_gba_browser_indexeddb_transaction_matches_detached_control() {
    let control = run_browser_gba_transaction(false).await;
    let subject = run_browser_gba_transaction(true).await;

    assert_gba_results_match(&control.frame_result, &subject.frame_result);
    assert_primary_matches(&control.primary, &subject.primary);
    assert_eq!(control.terminal_responses, subject.terminal_responses);
    assert_eq!(control.stored_entries, subject.stored_entries);
    crate::platform::clear_browser_storage_for_test()
        .await
        .expect("browser storage cleanup should complete");
}

#[cfg(feature = "wasm-browser-tests")]
fn assert_browser_sms_entries(
    thread: &EmuThread,
    primary: &PrimaryObservation,
    entries: &[(String, Vec<u8>)],
) {
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|(key, _)| !key.ends_with(".sav")));
    assert!(
        entries
            .iter()
            .all(|(key, _)| !key.starts_with("zeff-sram-v2:"))
    );
    assert!(primary.battery_bytes.is_none());

    let inner = thread.inner.borrow();
    assert!(!inner.backend.save_ram_kind().is_battery_backed());
    let media_sha256 = inner.backend.rom_hash();
    let component_sha256 = inner.backend.battery_component_hash();
    assert_eq!(
        component_sha256,
        crate::save_paths::recovery_state::canonical_battery_component_hash(&[])
    );
    let system = inner.backend.system().storage_subdir().to_owned();
    let discriminator = inner.backend.recovery_discriminator();
    let generation_path =
        crate::save_paths::recovery_state::battery_generation_path(&system, media_sha256).unwrap();
    let recovery_path = crate::save_paths::recovery_state::recovery_state_path(
        &system,
        inner.backend.system().state_extension(),
        media_sha256,
    )
    .unwrap();
    let mut expected_keys = vec![
        format!("zeff-state-{}", generation_path.display()),
        format!("zeff-state-{}", recovery_path.display()),
    ];
    expected_keys.sort();
    assert_eq!(
        entries
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>(),
        expected_keys
    );
    let generations = entries
        .iter()
        .filter_map(|(_, data)| {
            crate::save_paths::recovery_state::decode_battery_generation(data, media_sha256)
        })
        .collect::<Vec<_>>();
    assert_eq!(generations.len(), 1);
    assert_eq!(generations[0].generation, 0);
    assert_eq!(generations[0].component_sha256, component_sha256);
    let envelopes = entries
        .iter()
        .filter_map(|(_, data)| {
            crate::save_paths::recovery_state::decode_recovery_state(
                data,
                crate::save_paths::recovery_state::RecoveryStateIdentity {
                    system: &system,
                    discriminator: &discriminator,
                    media_sha256,
                },
            )
            .ok()
        })
        .collect::<Vec<_>>();
    assert_eq!(envelopes.len(), 1);
    assert_eq!(envelopes[0].system, system);
    assert_eq!(envelopes[0].discriminator, discriminator);
    assert_eq!(envelopes[0].media_sha256, media_sha256);
    assert_eq!(
        envelopes[0].battery,
        crate::save_paths::recovery_state::BatteryGenerationWitness::Committed {
            generation: 0,
            component_sha256,
        }
    );
    assert_eq!(envelopes[0].native_payload, primary.state);
}

#[cfg(feature = "wasm-browser-tests")]
async fn run_browser_sms_transaction(force_detached: bool) -> BrowserObservation {
    crate::platform::clear_browser_storage_for_test()
        .await
        .expect("browser storage should clear");
    let thread = sms_thread_with_recovery(true);
    if force_detached {
        thread
            .inner
            .borrow_mut()
            .speculation
            .force_frames_for_test(1);
    }

    thread.send(EmuCommand::StepFrames(Box::new(frame_input())));
    let frame_result = thread
        .try_recv_frame()
        .expect("StepFrames should publish one result");
    let expected_projection = force_detached.then(|| {
        let inner = thread.inner.borrow();
        let mut detached = inner.backend.fork_detached_for_speculation().unwrap();
        detached.disable_audio_output();
        assert!(detached.step_frames(1));
        detached.framebuffer().to_vec()
    });
    let primary = observe_primary(&thread);
    assert!(primary.potentially_dirty);
    assert!(primary.battery_bytes.is_none());
    assert_eq!(
        thread
            .inner
            .borrow()
            .speculation
            .committed_frames_for_test(),
        1
    );
    assert_eq!(
        thread.inner.borrow().speculation.completed_runs_for_test(),
        if force_detached { 1 } else { 0 }
    );
    let published = thread.shared_framebuffer.load_full().unwrap();
    if let Some(expected_projection) = expected_projection {
        assert_eq!(published.as_slice(), expected_projection.as_slice());
    } else {
        assert_eq!(published.as_slice(), primary.framebuffer.as_slice());
    }

    let expected_recovery_path = {
        let inner = thread.inner.borrow();
        crate::save_paths::recovery_state::recovery_state_path(
            inner.backend.system().storage_subdir(),
            inner.backend.system().state_extension(),
            inner.backend.rom_hash(),
        )
        .unwrap()
    };
    thread.send(EmuCommand::Shutdown);
    assert!(thread.inner.borrow().pending_storage.is_some());
    assert!(thread.try_recv_response().is_none());
    let terminal_responses = wait_for_browser_terminal(&thread).await;
    assert_eq!(terminal_responses[0], "sram:");
    assert_eq!(
        terminal_responses[1],
        format!("recovery:{}", expected_recovery_path.display())
    );
    assert_eq!(
        thread.inner.borrow().speculation.completed_runs_for_test(),
        if force_detached { 1 } else { 0 }
    );
    assert!(thread.inner.borrow().pending_storage.is_none());
    let stored_entries = crate::platform::fresh_browser_storage_entries_for_test()
        .await
        .expect("production IndexedDB reload should match stored entries");
    assert_browser_sms_entries(&thread, &primary, &stored_entries);

    BrowserObservation {
        frame_result,
        primary,
        terminal_responses,
        stored_entries,
    }
}

#[cfg(feature = "wasm-browser-tests")]
#[wasm_bindgen_test]
async fn wasm_sms_browser_indexeddb_transaction_matches_detached_control() {
    let control = run_browser_sms_transaction(false).await;
    let subject = run_browser_sms_transaction(true).await;

    assert_active_audio_results_match(&control.frame_result, &subject.frame_result);
    assert_primary_matches(&control.primary, &subject.primary);
    assert_eq!(control.terminal_responses, subject.terminal_responses);
    assert_eq!(control.stored_entries, subject.stored_entries);
    crate::platform::clear_browser_storage_for_test()
        .await
        .expect("browser storage cleanup should complete");
}
