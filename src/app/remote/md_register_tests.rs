use super::*;
use crate::app::tas_control::tests::harness::app_with_worker;
use crate::debug::{DebugUiActions, dock::TabDataRequirements};
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_thread::{
    AudioConfig, AudioRecordingCapture, EmuCommand, EmuThread, FrameInput, JoypadInput,
    SpeculationBlockers,
};
use crate::live_control::LiveMemorySpace;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn app() -> App {
    let mut rom = vec![0; 0x20000];
    rom[..4].copy_from_slice(&0x00ff_fffc_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x10000_u32.to_be_bytes());
    let words: [u16; 11] = [
        0x203c, 0x1234, 0x5678, 0x13fc, 0x005a, 0x00ff, 0x0040, 0x46fc, 0x2015, 0x60fe, 0x4e71,
    ];
    for (index, word) in words.iter().enumerate() {
        rom[0x10000 + index * 2..0x10002 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let path = PathBuf::from("register-inspection.md");
    let backend = load_backend_from_rom_source(
        ActiveSystem::Md,
        &path,
        &path,
        Some(rom),
        BackendLoadConfig {
            apply_mods: false,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend;
    app_with_worker(EmuThread::spawn(backend, false), 1, ActiveSystem::Md, path)
}

fn debug_info(app: &mut App) -> Value {
    match app.handle_live_command_for_test(LiveCommand::DebugInfo) {
        LiveReply::Ok(value) => value,
        LiveReply::Error(error) => panic!("debug_info failed: {error}"),
    }
}

fn memory(app: &mut App) -> Value {
    match app.handle_live_command_for_test(LiveCommand::MemoryRead {
        space: LiveMemorySpace::Cpu,
        start: 0xff0040,
        length: 1,
    }) {
        LiveReply::Ok(value) => value,
        LiveReply::Error(error) => panic!("memory read failed: {error}"),
    }
}

fn deliver(app: &mut App, attempt_mutation: bool, single_step: bool) {
    let mut actions = DebugUiActions::none();
    if attempt_mutation {
        actions.memory_writes.push((0xff0040, 0xa5));
        actions.add_breakpoint = Some(0x10013);
        actions.add_one_shot_breakpoint = Some(0x10013);
    }
    deliver_actions(
        app,
        actions,
        attempt_mutation,
        single_step,
        usize::from(!single_step),
    );
}

fn deliver_actions(
    app: &mut App,
    actions: DebugUiActions,
    attempt_mutation: bool,
    single_step: bool,
    expected_frames: usize,
) {
    let snapshot = app.build_snapshot_request(&TabDataRequirements::default(), false);
    assert!(snapshot.want_debug_info && snapshot.show_memory_viewer);
    let input = FrameInput {
        #[cfg(all(test, not(target_arch = "wasm32")))]
        audio_timing_request: 0,
        #[cfg(all(test, not(target_arch = "wasm32")))]
        audio_timing_pacing: (0, 0),
        frames: usize::from(!single_step),
        speculation_blockers: SpeculationBlockers::feature_disabled(),
        replay_joypad_frames: None,
        host_tilt: (0.0, 0.0),
        host_camera_frame: None,
        joypad: JoypadInput {
            buttons: 0,
            dpad: 0,
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
        zapper: Default::default(),
        debug_step: attempt_mutation || single_step,
        debug_continue: !single_step,
        debug_suspend_after_frame: attempt_mutation,
        audio: AudioConfig {
            apu_capture_enabled: false,
            skip_audio: false,
            playback_speed: 1,
            recording_capture: AudioRecordingCapture::default(),
        },
        debug_actions: actions,
        snapshot,
        buffers: app.take_reusable_buffers(),
        rewind_enabled: false,
        rewind_seconds: 0,
    };
    assert!(
        app.emu_thread
            .as_ref()
            .unwrap()
            .send_checked(EmuCommand::StepFrames(Box::new(input)))
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(result) = app.emu_thread.as_ref().unwrap().try_recv_frame() {
            assert!(result.runtime_fault.is_none());
            assert_eq!(result.advanced_frames, expected_frames);
            app.process_frame_result(result);
            return;
        }
        assert!(
            Instant::now() < deadline,
            "MD worker did not publish registers"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn md_watchpoint_remote_worker_reports_24_bit_access_hit_without_prior_storage_value() {
    let mut app = app();
    assert!(debug_info(&mut app)["cpu"].is_null());
    assert_eq!(memory(&mut app)["ready"], false);
    let mut actions = DebugUiActions::none();
    actions.add_watchpoint = Some((0xff0040, 0xff0040, zeff_emu_common::debug::WatchType::Write));
    deliver_actions(&mut app, actions, false, false, 0);
    let reply = debug_info(&mut app);
    assert_eq!(reply["cpu"]["state"], "Suspended");
    assert_eq!(reply["cpu"]["pc"], 0x1000e);
    assert_eq!(
        reply["cpu"]["hit_watchpoint"],
        json!({
            "address": 0xff0040, "address_hex": "00FF0040", "old_value": 0,
            "new_value": 0x5a, "type": "Write",
        })
    );
    assert_eq!(reply["cpu"]["watchpoints"][0]["address"], 0xff0040);
    assert_eq!(memory(&mut app)["bytes"], json!([0x5a]));
    let features = app
        .cached_ui_data
        .as_ref()
        .unwrap()
        .core_features
        .as_ref()
        .unwrap();
    let features = core_features::core_features_json(features);
    assert_eq!(features["supports_memory_watchpoints"], true);
    assert_eq!(features["supports_symbol_loading"], true);
    assert_eq!(features["supports_debugger"], true);
    app.emu_thread.as_mut().unwrap().shutdown();
}

#[test]
fn md_register_remote_worker_separates_execution_controls_from_debug_mutations() {
    let mut app = app();
    assert!(debug_info(&mut app)["cpu"].is_null());
    assert_eq!(memory(&mut app)["ready"], false);
    let mut previous_cycles = 0;
    for attempt_mutation in [false, true, false] {
        deliver(&mut app, attempt_mutation, false);
        let reply = debug_info(&mut app);
        assert!(reply["note"].is_null());
        let cpu = &reply["cpu"];
        assert_eq!(cpu["pc"], 0x10012);
        assert_eq!(
            cpu["state"],
            if attempt_mutation {
                "Suspended"
            } else {
                "Running"
            }
        );
        assert_eq!(cpu["status"], "M68000 register snapshot");
        let cycles = cpu["cycles"].as_u64().unwrap();
        assert!(cycles > previous_cycles);
        previous_cycles = cycles;
        assert_eq!(cpu["registers"][0], "D0:12345678  D1:00000000");
        assert_eq!(cpu["registers"][7], "A6:00000000  A7:00FFFFFC");
        assert_eq!(cpu["registers"][9], "PC:00010012  SR:2015");
        assert_eq!(
            cpu["flags"],
            json!([
                {"name": "X", "set": true}, {"name": "N", "set": false},
                {"name": "Z", "set": true}, {"name": "V", "set": false},
                {"name": "C", "set": true}
            ])
        );
        assert_eq!(cpu["recent_opcode_records"].as_array().unwrap().len(), 32);
        assert!(
            cpu["recent_opcode_records"]
                .as_array()
                .unwrap()
                .iter()
                .all(|entry| {
                    entry["address"] == 0x10012 && entry["bytes"] == json!([0x60, 0xfe])
                })
        );
        assert_eq!(cpu["last_opcode"], cpu["recent_opcodes"][0]);
        for field in [
            "call_stack",
            "breakpoints",
            "one_shot_breakpoints",
            "watchpoints",
        ] {
            assert!(cpu[field].as_array().unwrap().is_empty());
        }
        assert_eq!(cpu["call_stack_available"], true);
        assert_eq!(
            cpu["supported_events"],
            json!(["IRQ / NMI", "DMA transfer", "CPU exception"])
        );
        assert!(cpu["hit_breakpoint"].is_null());
        assert_eq!(memory(&mut app)["bytes"], json!([0x5a]));
        let features = app
            .cached_ui_data
            .as_ref()
            .unwrap()
            .core_features
            .as_ref()
            .unwrap();
        assert!(features.supports_debugger);
        assert!(features.supports_execution_controls);
        assert!(features.supports_symbol_loading);
        assert!(features.supports_guest_calls);
        assert!(features.supports_opcode_history);
        assert!(features.supports_instruction_trace);
        assert!(features.supports_cheats);
        assert!(features.supports_rewind);
        assert!(features.supports_replay);
    }
    deliver(&mut app, true, false);
    let before = debug_info(&mut app);
    deliver(&mut app, false, true);
    let after = debug_info(&mut app);
    assert_eq!(after["cpu"]["state"], "Suspended");
    assert_eq!(after["cpu"]["pc"], before["cpu"]["pc"]);
    assert!(after["cpu"]["cycles"].as_u64().unwrap() > before["cpu"]["cycles"].as_u64().unwrap());
    assert_eq!(memory(&mut app)["bytes"], json!([0x5a]));
    app.emu_thread.as_mut().unwrap().shutdown();
}
