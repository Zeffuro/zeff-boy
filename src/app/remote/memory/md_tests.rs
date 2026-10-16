use super::*;
use crate::app::tas_control::tests::harness::app_with_worker;
use crate::debug::{DebugUiActions, dock::TabDataRequirements};
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_thread::{
    AudioConfig, AudioRecordingCapture, EmuCommand, EmuThread, FrameInput, JoypadInput,
    SpeculationBlockers,
};
use crate::live_control::{LiveCommand, LiveReply};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn app() -> App {
    let mut rom = vec![0; 0x20000];
    rom[..4].copy_from_slice(&0x00ff_fffc_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    rom[0x10010..0x10014].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
    let mut words = Vec::new();
    for (address, value) in [
        (0xff0040_u32, 0xde),
        (0xff0041, 0xad),
        (0xfffffe, 0x12),
        (0xffffff, 0x34),
    ] {
        words.extend([0x13fc_u16, value, (address >> 16) as u16, address as u16]);
    }
    for (port, value) in [
        (4, 0x8104),
        (4, 0x8f02),
        (4, 0xc000),
        (4, 0),
        (0, 0xf246),
        (4, 0xc07e),
        (4, 0),
        (0, 0xffff),
        (4, 0x4000),
        (4, 0x10),
        (0, 0xfa35),
        (4, 0x404e),
        (4, 0x10),
        (0, 0xffab),
    ] {
        words.extend([0x33fc, value, 0x00c0, port]);
    }
    words.extend([0x60fe, 0x4e71]);
    for (index, word) in words.iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let path = PathBuf::from("memory-parity.md");
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

fn read(app: &mut App, start: u32, length: usize) -> Value {
    match app.handle_live_command_for_test(LiveCommand::MemoryRead {
        space: LiveMemorySpace::Cpu,
        start,
        length,
    }) {
        LiveReply::Ok(value) => value,
        LiveReply::Error(error) => panic!("memory command failed: {error}"),
    }
}

#[test]
fn md_graphics_remote_worker_exposes_canonical_vdp_bytes() {
    let mut app = app();
    assert_eq!(app.live_graphics_json()["ready"], false);
    assert_eq!(read(&mut app, 0xff0040, 2)["ready"], false);
    deliver(&mut app, false);
    let reply = app.live_graphics_json();
    assert_eq!(reply["ready"], true);
    assert_eq!(reply["system"], "md");
    assert_eq!(reply["palette_colors"], 64);
    assert_eq!(reply["tile_height"], 8);
    let space = LiveMemorySpace::Region("vram".into());
    let first = app.live_memory_json(&space, 0, 4);
    assert_eq!(first["ready"], true);
    assert_eq!(first["bytes"], json!([0, 0, 0, 0]));
    let last = app.live_memory_json(&space, 0xffff, 4);
    assert_eq!(last["start"], 0xffff);
    assert_eq!(last["bytes"], json!([0]));
    assert_eq!(app.live_memory_json(&space, 0x10000, 1)["ready"], false);
    for (name, size, first_bytes, last_bytes) in [
        ("cram", 128, [0x02, 0x46], [0x0e, 0xee]),
        ("palette", 128, [0x02, 0x46], [0x0e, 0xee]),
        ("paletteram", 128, [0x02, 0x46], [0x0e, 0xee]),
        ("vsram", 80, [0x02, 0x35], [0x07, 0xab]),
    ] {
        let space = LiveMemorySpace::Region(name.into());
        let first = app.live_memory_json(&space, 0, 2);
        assert_eq!(first["ready"], true);
        assert_eq!(first["available"], size);
        assert_eq!(first["bytes"], json!(first_bytes));
        let end = app.live_memory_json(&space, size - 2, usize::MAX);
        assert_eq!(end["bytes"], json!(last_bytes));
        assert_eq!(end["length"], 2);
        let odd = app.live_memory_json(&space, size - 1, 4);
        assert_eq!(odd["bytes"], json!([last_bytes[1]]));
        assert_eq!(app.live_memory_json(&space, 1, 0)["bytes"], json!([]));
        for start in [size, u32::MAX] {
            let outside = app.live_memory_json(&space, start, usize::MAX);
            assert_eq!(outside["ready"], false);
            assert_eq!(outside["available"], size);
        }
    }
    for name in ["oam", "sat"] {
        let reply = app.live_memory_json(&LiveMemorySpace::Region(name.into()), 0, 4);
        assert_eq!(reply["ready"], false);
        assert_eq!(
            reply["note"],
            "unsupported Mega Drive graphics memory space"
        );
    }
    app.emu_thread.as_mut().unwrap().shutdown();
}

fn deliver(app: &mut App, attempt_mutation: bool) {
    let snapshot = app.build_snapshot_request(&TabDataRequirements::default(), false);
    assert!(snapshot.show_memory_viewer);
    let mut actions = DebugUiActions::none();
    if attempt_mutation {
        actions.memory_writes.push((0xff0040, 0x77));
        actions.add_breakpoint = Some(0x200);
    }
    let input = FrameInput {
        frames: 1,
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
        debug_step: false,
        debug_continue: false,
        debug_suspend_after_frame: false,
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
            assert_eq!(result.advanced_frames, 1);
            app.process_frame_result(result);
            return;
        }
        assert!(
            Instant::now() < deadline,
            "MD worker did not publish memory snapshot"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn md_memory_remote_worker_preserves_high_start_and_read_only_guards() {
    let mut app = app();
    assert_eq!(read(&mut app, 0x10010, 4)["ready"], false);
    assert_eq!(app.remote_memory_view_start, Some(0x10010));
    deliver(&mut app, false);
    let reply = read(&mut app, 0x10010, 4);
    assert_eq!(reply["ready"], true);
    assert_eq!(reply["start"], 0x10010);
    assert_eq!(reply["bytes"], json!([0xde, 0xad, 0xbe, 0xef]));
    assert_eq!(read(&mut app, 0xff0040, 2)["ready"], false);
    deliver(&mut app, true);
    let reply = read(&mut app, 0xff0040, 2);
    assert_eq!(reply["ready"], true);
    assert_eq!(reply["bytes"], json!([0xde, 0xad]));
    let features = app
        .cached_ui_data
        .as_ref()
        .unwrap()
        .core_features
        .as_ref()
        .unwrap();
    assert!(!features.supports_debugger);
    assert!(!features.supports_execution_controls);
    assert!(!features.supports_guest_calls);
    assert!(!features.supports_opcode_history);
    assert!(!features.supports_cheats);
    app.emu_thread.as_mut().unwrap().shutdown();
}

#[test]
fn md_memory_remote_boundary_range_is_exact_and_invalid_requests_do_not_schedule() {
    let mut app = app();
    assert_eq!(read(&mut app, 0xfffffe, 2)["ready"], false);
    deliver(&mut app, false);
    let reply = read(&mut app, 0xfffffe, 2);
    assert_eq!(reply["ready"], true);
    assert_eq!(reply["start"], 0xfffffe);
    assert_eq!(reply["length"], 2);
    assert_eq!(reply["available"], 2);
    assert_eq!(reply["bytes"], json!([0x12, 0x34]));
    for (start, length) in [
        (0xfffffe, 3),
        (0xffffff, 2),
        (0x1000000, 1),
        (u32::MAX, usize::MAX),
    ] {
        app.remote_memory_frames_remaining = 0;
        app.remote_memory_view_start = None;
        let reply = read(&mut app, start, length);
        assert_eq!(reply["ready"], false);
        assert_eq!(reply["start"], start);
        assert_eq!(reply["requested_length"], length);
        assert!(reply["note"].as_str().unwrap().contains("24-bit"));
        assert_eq!(app.remote_memory_frames_remaining, 0);
        assert_eq!(app.remote_memory_view_start, None);
    }
    assert_eq!(read(&mut app, 0xffffff, 1)["ready"], false);
    deliver(&mut app, false);
    assert_eq!(read(&mut app, 0xffffff, 1)["bytes"], json!([0x34]));
    app.active_system = ActiveSystem::Nes;
    let reply = read(&mut app, u32::MAX, 4);
    assert_eq!(reply["ready"], false);
    assert_eq!(app.remote_memory_frames_remaining, 3);
    assert_eq!(app.remote_memory_view_start, Some(u32::MAX));
    app.emu_thread.as_mut().unwrap().shutdown();
}
