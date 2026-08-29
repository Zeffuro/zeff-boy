use super::{
    EmuLoop, EmuLoopConfig, game_boy_replay_start_capture_blocker, terminal_battery_response,
};
use crate::audio_tooling::{
    AudioChannelId, AudioSemanticFrame, AudioVoiceClass, AudioVoiceState, NTSC_60_TEMPO_US_PER_BEAT,
};
use crate::emu_backend::EmuBackend;
use crate::emu_thread::recovery::RecoveryTestConfig;
use crate::emu_thread::{
    AudioConfig, AudioRecordingCapture, EmuCommand, EmuResponse, EmuThread, FrameInput,
    FrameResult, JoypadInput, MemorySearchRequest, RenderSettings, ReusableBuffers,
    SnapshotRequest, SpeculationBlockers, TcpLinkMode, ZapperInput,
};
use crate::link::transport::TcpLinkTransport;
use crate::link::{LinkEndpointId, LinkSession, LinkSystemType, RemoteLink};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const SMS_AUDIO_ROM: &[u8] = &[
    0x3E, 0x80, 0xD3, 0x7F, // Tone 0 low period.
    0x3E, 0x04, 0xD3, 0x7F, // Tone 0 high period.
    0x3E, 0x90, 0xD3, 0x7F, // Tone 0 full volume.
    0x76, // Halt while the PSG continues.
];

#[test]
fn terminal_battery_response_never_reports_a_failed_barrier_as_flushed() {
    let record = crate::save_paths::recovery_state::BatteryGenerationRecord {
        generation: 2,
        component_sha256: [3; 32],
    };
    assert!(matches!(
        terminal_battery_response(&Ok((Some("game.sav".to_string()), record))),
        EmuResponse::SramFlushed(Some(path)) if path == "game.sav"
    ));
    assert!(matches!(
        terminal_battery_response(&Err(anyhow::anyhow!("injected failure"))),
        EmuResponse::SramFlushFailed(error) if error == "injected failure"
    ));
}

fn test_loop() -> (
    EmuLoop,
    crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
) {
    sega8_test_loop(&[0x00], PathBuf::from("test.sms"))
}

fn audio_test_loop() -> (
    EmuLoop,
    crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
) {
    sega8_test_loop_with_recovery(
        SMS_AUDIO_ROM,
        PathBuf::from("audio-test.sms"),
        false,
        None,
        true,
    )
}

fn gba_test_rom() -> Vec<u8> {
    let mut rom = vec![0; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xAC..0xB0].copy_from_slice(b"BPEE");
    rom[0xB0..0xB2].copy_from_slice(b"01");
    rom[0xB2] = 0x96;
    rom.extend_from_slice(b"SRAM_V113");
    rom
}

fn gba_rtc() -> zeff_gba_core::hardware::cartridge::RtcDateTime {
    zeff_gba_core::hardware::cartridge::RtcDateTime::new(2032, 9, 17, 5, [21, 43, 19]).unwrap()
}

fn gba_sram_bytes(len: usize) -> Vec<u8> {
    (0..len)
        .map(|index| (index as u8).wrapping_mul(17).wrapping_add(3))
        .collect()
}

fn gba_test_loop_with_recovery(
    path: PathBuf,
    save_recovery_on_shutdown: bool,
    recovery: Option<RecoveryTestConfig>,
) -> (
    EmuLoop,
    crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
) {
    let mut emu = zeff_gba_core::emulator::Emulator::new(&gba_test_rom(), 44_100).unwrap();
    let sram = gba_sram_bytes(emu.dump_battery_sram().unwrap().len());
    emu.load_battery_sram(&sram).unwrap();
    assert!(emu.set_rtc_date_time(gba_rtc()));
    emu.set_instruction_trace_enabled(true);
    let backend = EmuBackend::from_gba(emu, path);
    let (_cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
    let (frame_tx, frame_rx) = crossbeam_channel::bounded(2);
    let drain_rx = frame_rx.clone();
    let (resp_tx, resp_rx) = crossbeam_channel::unbounded();
    let shared = crate::emu_thread::types::new_shared_framebuffer();
    (
        EmuLoop::new(
            backend,
            cmd_rx,
            frame_tx,
            drain_rx,
            resp_tx,
            EmuLoopConfig {
                shared_framebuffer: shared,
                save_recovery_on_shutdown,
                recovery,
            },
        ),
        resp_rx,
    )
}

fn gba_test_loop() -> (
    EmuLoop,
    crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
) {
    gba_test_loop_with_recovery(PathBuf::from("emerald-sram.gba"), false, None)
}

fn sega8_test_loop(
    rom: &[u8],
    path: PathBuf,
) -> (
    EmuLoop,
    crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
) {
    sega8_test_loop_with_recovery(rom, path, false, None, false)
}

fn sega8_test_loop_with_recovery(
    rom: &[u8],
    path: PathBuf,
    save_recovery_on_shutdown: bool,
    recovery: Option<RecoveryTestConfig>,
    instruction_trace: bool,
) -> (
    EmuLoop,
    crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
) {
    let mut emu = zeff_sega8_core::emulator::Emulator::new_with_hint(
        rom,
        44_100,
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
    )
    .unwrap();
    emu.set_instruction_trace_enabled(instruction_trace);
    let backend = EmuBackend::from_sega8(emu, path);
    let (_cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
    let (frame_tx, frame_rx) = crossbeam_channel::bounded(2);
    let drain_rx = frame_rx.clone();
    let (resp_tx, resp_rx) = crossbeam_channel::unbounded();
    let shared = crate::emu_thread::types::new_shared_framebuffer();
    (
        EmuLoop::new(
            backend,
            cmd_rx,
            frame_tx,
            drain_rx,
            resp_tx,
            EmuLoopConfig {
                shared_framebuffer: shared,
                save_recovery_on_shutdown,
                recovery,
            },
        ),
        resp_rx,
    )
}

fn test_pal_sega8_loop() -> (
    EmuLoop,
    crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
) {
    let emu = zeff_sega8_core::emulator::Emulator::new_with_hint_and_video_standard(
        &[0x00],
        44_100,
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
        zeff_sega8_core::hardware::timing::Sega8VideoStandard::Pal,
    )
    .unwrap();
    let backend = EmuBackend::from_sega8(emu, PathBuf::from("test-pal.sms"));
    let (_cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
    let (frame_tx, frame_rx) = crossbeam_channel::bounded(2);
    let drain_rx = frame_rx.clone();
    let (resp_tx, resp_rx) = crossbeam_channel::unbounded();
    let shared = crate::emu_thread::types::new_shared_framebuffer();
    (
        EmuLoop::new(
            backend,
            cmd_rx,
            frame_tx,
            drain_rx,
            resp_tx,
            EmuLoopConfig {
                shared_framebuffer: shared,
                save_recovery_on_shutdown: false,
                recovery: None,
            },
        ),
        resp_rx,
    )
}

fn test_fds_loop() -> (
    EmuLoop,
    crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
) {
    use zeff_nes_core::hardware::cartridge::mappers::{FDS_BIOS_SIZE, FDS_SIDE_SIZE};

    let mut disk = vec![0; FDS_SIDE_SIZE];
    disk[0] = 1;
    let emu =
        zeff_nes_core::emulator::Emulator::new_fds(&disk, vec![0xFF; FDS_BIOS_SIZE], 44_100.0)
            .unwrap();
    let backend = EmuBackend::from_nes(emu, PathBuf::from("test.fds"));
    let (_cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
    let (frame_tx, frame_rx) = crossbeam_channel::bounded(2);
    let drain_rx = frame_rx.clone();
    let (resp_tx, resp_rx) = crossbeam_channel::unbounded();
    let shared = crate::emu_thread::types::new_shared_framebuffer();
    (
        EmuLoop::new(
            backend,
            cmd_rx,
            frame_tx,
            drain_rx,
            resp_tx,
            EmuLoopConfig {
                shared_framebuffer: shared,
                save_recovery_on_shutdown: false,
                recovery: None,
            },
        ),
        resp_rx,
    )
}

fn test_gb_loop_with_tcp() -> (
    EmuLoop,
    crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
    TcpLinkTransport,
) {
    let emu = zeff_gb_core::emulator::Emulator::from_rom_data(
        &[0; 0x8000],
        zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::Auto,
    )
    .unwrap();
    let backend = EmuBackend::from_gb(emu, PathBuf::from("test.gb"));
    let (_cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
    let (frame_tx, frame_rx) = crossbeam_channel::bounded(2);
    let drain_rx = frame_rx.clone();
    let (resp_tx, resp_rx) = crossbeam_channel::unbounded();
    let shared = crate::emu_thread::types::new_shared_framebuffer();
    let mut emu_loop = EmuLoop::new(
        backend,
        cmd_rx,
        frame_tx,
        drain_rx,
        resp_tx,
        EmuLoopConfig {
            shared_framebuffer: shared,
            save_recovery_on_shutdown: false,
            recovery: None,
        },
    );

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let host = std::thread::spawn(move || TcpLinkTransport::accept_once(listener).unwrap());
    let peer = TcpLinkTransport::connect(addr).unwrap();
    let transport = host.join().unwrap();
    emu_loop.tcp_link = Some(RemoteLink::GameBoy(
        crate::link::gb::GameBoyRemoteLink::new(LinkSession::new(
            transport,
            LinkSystemType::GameBoy,
            LinkEndpointId(1),
        )),
    ));
    (emu_loop, resp_rx, peer)
}

fn semantic_result() -> FrameResult {
    FrameResult {
        advanced_frames: 1,
        delivery_merged: false,
        replay_events: Vec::new(),
        replay_error: None,
        runtime_fault: None,
        rumble: false,
        audio_samples: Vec::new(),
        audio_playback_speed: 1,
        ui_data: crate::ui::UiFrameData::default(),
        is_mbc7: false,
        is_pocket_camera: false,
        game_boy_serial_device: None,
        game_boy_printer_jobs: Vec::new(),
        media_slot_snapshot: None,
        rewind_fill: 0.0,
        audio_semantic_frames: vec![AudioSemanticFrame {
            frame: 1,
            tempo_us_per_beat: NTSC_60_TEMPO_US_PER_BEAT,
            voices: vec![AudioVoiceState {
                channel: AudioChannelId(0),
                name: "Test",
                class: AudioVoiceClass::Tone,
                active: false,
                pitch_hz: Some(440.0),
                level: Some(0.0),
            }],
        }],
        audio_timeline_discontinuities: Vec::new(),
    }
}

fn frame_input(frames: usize) -> FrameInput {
    FrameInput {
        frames,
        speculation_blockers: SpeculationBlockers::from_app_for_test(false, false),
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
        zapper: ZapperInput::default(),
        debug_step: false,
        debug_continue: false,
        debug_suspend_after_frame: false,
        audio: AudioConfig {
            apu_capture_enabled: false,
            skip_audio: true,
            playback_speed: 1,
            recording_capture: AudioRecordingCapture::default(),
        },
        debug_actions: crate::debug::DebugUiActions::none(),
        snapshot: SnapshotRequest {
            want_debug_info: false,
            want_perf_info: false,
            any_viewer_open: false,
            any_vram_viewer_open: false,
            show_oam_viewer: false,
            show_apu_viewer: false,
            show_disassembler: false,
            show_rom_info: false,
            show_memory_viewer: false,
            memory_view_start: 0,
            show_rom_viewer: false,
            show_instruction_trace: false,
            trace_after_sequence: None,
            rom_view_start: 0,
            last_disasm_pc: None,
            last_disasm_mapping: None,
            disasm_target: None,
            memory_search: None,
            rom_search: None,
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

fn assert_frame_results_match(left: &FrameResult, right: &FrameResult) {
    assert_eq!(left.advanced_frames, right.advanced_frames);
    assert_eq!(left.delivery_merged, right.delivery_merged);
    assert_eq!(left.replay_events, right.replay_events);
    assert_eq!(left.replay_error, right.replay_error);
    assert_eq!(left.runtime_fault, right.runtime_fault);
    assert_eq!(left.rumble, right.rumble);
    assert_f32_equal("PCM", &left.audio_samples, &right.audio_samples);
    assert_eq!(left.audio_playback_speed, right.audio_playback_speed);
    assert_eq!(left.is_mbc7, right.is_mbc7);
    assert_eq!(left.is_pocket_camera, right.is_pocket_camera);
    assert_eq!(left.game_boy_serial_device, right.game_boy_serial_device);
    assert_eq!(
        left.game_boy_printer_jobs.len(),
        right.game_boy_printer_jobs.len()
    );
    assert_eq!(left.media_slot_snapshot, right.media_slot_snapshot);
    assert_eq!(left.rewind_fill.to_bits(), right.rewind_fill.to_bits());
    assert_eq!(left.audio_semantic_frames, right.audio_semantic_frames);
    assert_eq!(
        left.audio_timeline_discontinuities,
        right.audio_timeline_discontinuities
    );
    assert_eq!(left.ui_data.core_features, right.ui_data.core_features);
    assert_eq!(
        left.ui_data.cpu_debug.is_some(),
        right.ui_data.cpu_debug.is_some()
    );
    assert_eq!(
        left.ui_data.perf_info.is_some(),
        right.ui_data.perf_info.is_some()
    );
    assert_eq!(
        left.ui_data.apu_debug.is_some(),
        right.ui_data.apu_debug.is_some()
    );
    assert_eq!(
        left.ui_data.oam_debug.is_some(),
        right.ui_data.oam_debug.is_some()
    );
    assert_eq!(
        left.ui_data.palette_debug.is_some(),
        right.ui_data.palette_debug.is_some()
    );
    assert_eq!(
        left.ui_data.rom_debug.is_some(),
        right.ui_data.rom_debug.is_some()
    );
    assert_eq!(
        left.ui_data.input_debug.is_some(),
        right.ui_data.input_debug.is_some()
    );
    assert_eq!(
        left.ui_data.graphics_data.is_some(),
        right.ui_data.graphics_data.is_some()
    );
    assert_eq!(
        left.ui_data.disassembly_view.is_some(),
        right.ui_data.disassembly_view.is_some()
    );
    assert_eq!(left.ui_data.memory_page, right.ui_data.memory_page);
    assert_eq!(
        left.ui_data.memory_search_results.is_some(),
        right.ui_data.memory_search_results.is_some()
    );
    assert_eq!(left.ui_data.rom_page, right.ui_data.rom_page);
    assert_eq!(left.ui_data.rom_size, right.ui_data.rom_size);
    assert_eq!(
        left.ui_data.rom_search_results.is_some(),
        right.ui_data.rom_search_results.is_some()
    );
    assert_eq!(
        left.ui_data.instruction_trace.is_some(),
        right.ui_data.instruction_trace.is_some()
    );
}

fn active_audio_frame_input() -> FrameInput {
    let mut input = frame_input(1);
    input.joypad.buttons = 0x01;
    input.joypad.dpad = 0x02;
    input.audio.apu_capture_enabled = true;
    input.audio.skip_audio = false;
    input.audio.recording_capture = AudioRecordingCapture {
        active: true,
        semantic: true,
    };
    input.snapshot.want_debug_info = true;
    input.snapshot.want_perf_info = true;
    input.snapshot.any_viewer_open = true;
    input.snapshot.any_vram_viewer_open = true;
    input.snapshot.show_oam_viewer = true;
    input.snapshot.show_apu_viewer = true;
    input.snapshot.show_disassembler = true;
    input.snapshot.show_rom_info = true;
    input.snapshot.show_memory_viewer = true;
    input.snapshot.show_rom_viewer = true;
    input.snapshot.show_instruction_trace = true;
    input.snapshot.memory_search = Some(MemorySearchRequest {
        pattern: SMS_AUDIO_ROM[..4].to_vec(),
        max_results: 4,
    });
    input.snapshot.rom_search = Some(MemorySearchRequest {
        pattern: SMS_AUDIO_ROM[..4].to_vec(),
        max_results: 4,
    });
    input
}

fn gba_frame_input() -> FrameInput {
    let mut input = active_audio_frame_input();
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

fn assert_bytes_equal(label: &str, left: &[u8], right: &[u8]) {
    assert_eq!(left.len(), right.len(), "{label} length");
    assert!(
        left == right,
        "{label} content differs: left={:016X} right={:016X}",
        byte_signature(left),
        byte_signature(right)
    );
}

fn byte_signature(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

fn assert_f32_equal(label: &str, left: &[f32], right: &[f32]) {
    assert_eq!(left.len(), right.len(), "{label} length");
    let exact = left
        .iter()
        .zip(right)
        .all(|(left, right)| left.to_bits() == right.to_bits());
    assert!(
        exact,
        "{label} content differs: left={:016X} right={:016X}",
        f32_signature(left),
        f32_signature(right)
    );
}

fn f32_signature(samples: &[f32]) -> u64 {
    samples.iter().fold(0xcbf2_9ce4_8422_2325, |hash, sample| {
        (hash ^ u64::from(sample.to_bits())).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

fn assert_rich_ui_data_match(left: &FrameResult, right: &FrameResult) {
    assert!(left.ui_data.core_features.is_some());
    let left_cpu = left.ui_data.cpu_debug.as_ref().expect("CPU debug data");
    let right_cpu = right.ui_data.cpu_debug.as_ref().expect("CPU debug data");
    assert_eq!(left_cpu.register_lines, right_cpu.register_lines);
    assert!(!left_cpu.register_lines.is_empty());
    assert_eq!(left_cpu.flags, right_cpu.flags);
    assert_eq!(left_cpu.status_text, right_cpu.status_text);
    assert_eq!(left_cpu.cpu_state, right_cpu.cpu_state);
    assert_eq!(left_cpu.pc, right_cpu.pc);
    assert_eq!(left_cpu.cycles, right_cpu.cycles);
    assert_eq!(left_cpu.last_opcode_line, right_cpu.last_opcode_line);
    assert_eq!(left_cpu.sections, right_cpu.sections);
    assert_eq!(left_cpu.io_registers, right_cpu.io_registers);
    assert_eq!(
        left_cpu.recent_opcodes.len(),
        right_cpu.recent_opcodes.len()
    );
    assert!(!left_cpu.recent_opcodes.is_empty());
    for (left_op, right_op) in left_cpu
        .recent_opcodes
        .iter()
        .zip(&right_cpu.recent_opcodes)
    {
        assert_eq!(left_op.address, right_op.address);
        assert_eq!(left_op.storage_offset, right_op.storage_offset);
        assert_bytes_equal("recent opcode", &left_op.bytes, &right_op.bytes);
        assert_eq!(left_op.detail, right_op.detail);
        assert_eq!(left_op.repeat_count, right_op.repeat_count);
        assert_eq!(left_op.thumb, right_op.thumb);
    }
    assert_eq!(left_cpu.call_stack.len(), right_cpu.call_stack.len());
    for (left_call, right_call) in left_cpu.call_stack.iter().zip(&right_cpu.call_stack) {
        assert_eq!(left_call.target, right_call.target);
        assert_eq!(left_call.return_address, right_call.return_address);
        assert_eq!(left_call.target_rom_offset, right_call.target_rom_offset);
        assert_eq!(left_call.return_rom_offset, right_call.return_rom_offset);
        assert_eq!(left_call.kind, right_call.kind);
    }
    assert_eq!(
        left_cpu.call_stack_available,
        right_cpu.call_stack_available
    );
    assert_eq!(left_cpu.breakpoints, right_cpu.breakpoints);
    assert_eq!(
        left_cpu.one_shot_breakpoints,
        right_cpu.one_shot_breakpoints
    );
    assert_eq!(
        left_cpu.breakpoint_hit_conditions,
        right_cpu.breakpoint_hit_conditions
    );
    assert_eq!(left_cpu.supported_events, right_cpu.supported_events);
    assert_eq!(left_cpu.event_breakpoints, right_cpu.event_breakpoints);
    assert_eq!(left_cpu.rom_breakpoints, right_cpu.rom_breakpoints);
    assert_eq!(left_cpu.watchpoints.len(), right_cpu.watchpoints.len());
    for (left_watch, right_watch) in left_cpu.watchpoints.iter().zip(&right_cpu.watchpoints) {
        assert_eq!(left_watch.address, right_watch.address);
        assert_eq!(left_watch.end_address, right_watch.end_address);
        assert_eq!(left_watch.watch_type, right_watch.watch_type);
    }
    assert_eq!(left_cpu.hit_breakpoint, right_cpu.hit_breakpoint);
    assert_eq!(left_cpu.hit_rom_breakpoint, right_cpu.hit_rom_breakpoint);
    match (&left_cpu.hit_watchpoint, &right_cpu.hit_watchpoint) {
        (None, None) => {}
        (Some(left_hit), Some(right_hit)) => {
            assert_eq!(left_hit.address, right_hit.address);
            assert_eq!(left_hit.old_value, right_hit.old_value);
            assert_eq!(left_hit.new_value, right_hit.new_value);
            assert_eq!(left_hit.watch_type, right_hit.watch_type);
        }
        _ => panic!("watchpoint hit presence differs"),
    }
    assert_eq!(left_cpu.hit_event, right_cpu.hit_event);

    let left_apu = left.ui_data.apu_debug.as_ref().expect("APU data");
    let right_apu = right.ui_data.apu_debug.as_ref().expect("APU data");
    assert_eq!(left_apu.extra_sections, right_apu.extra_sections);

    let left_perf = left.ui_data.perf_info.as_ref().expect("performance data");
    let right_perf = right.ui_data.perf_info.as_ref().expect("performance data");
    assert_eq!(left_perf.fps.to_bits(), right_perf.fps.to_bits());
    assert_eq!(
        left_perf.target_fps.to_bits(),
        right_perf.target_fps.to_bits()
    );
    assert_eq!(left_perf.speed_mode_label, right_perf.speed_mode_label);
    assert_eq!(left_perf.frames_in_flight, right_perf.frames_in_flight);
    assert_eq!(left_perf.cycles, right_perf.cycles);
    assert_eq!(left_perf.platform_name, right_perf.platform_name);
    assert_eq!(left_perf.hardware_label, right_perf.hardware_label);
    assert_eq!(
        left_perf.hardware_pref_label,
        right_perf.hardware_pref_label
    );

    let left_oam = left.ui_data.oam_debug.as_ref().expect("OAM data");
    let right_oam = right.ui_data.oam_debug.as_ref().expect("OAM data");
    assert_eq!(left_oam.headers, right_oam.headers);
    assert_eq!(left_oam.rows, right_oam.rows);
    assert!(!left_oam.rows.is_empty());

    let left_palette = left.ui_data.palette_debug.as_ref().expect("palette data");
    let right_palette = right.ui_data.palette_debug.as_ref().expect("palette data");
    assert_eq!(left_palette.groups.len(), right_palette.groups.len());
    assert!(!left_palette.groups.is_empty());
    for (left_group, right_group) in left_palette.groups.iter().zip(&right_palette.groups) {
        assert_eq!(left_group.title, right_group.title);
        assert_eq!(left_group.rows.len(), right_group.rows.len());
        assert!(!left_group.rows.is_empty());
        for (left_row, right_row) in left_group.rows.iter().zip(&right_group.rows) {
            assert_eq!(left_row.label, right_row.label);
            assert_eq!(left_row.colors, right_row.colors);
            assert!(!left_row.colors.is_empty());
        }
    }

    let left_rom = left.ui_data.rom_debug.as_ref().expect("ROM data");
    let right_rom = right.ui_data.rom_debug.as_ref().expect("ROM data");
    assert_eq!(left_rom.sections.len(), right_rom.sections.len());
    assert!(!left_rom.sections.is_empty());
    for (left_section, right_section) in left_rom.sections.iter().zip(&right_rom.sections) {
        assert_eq!(left_section.heading, right_section.heading);
        assert_eq!(left_section.fields, right_section.fields);
        assert!(!left_section.fields.is_empty());
    }

    match (
        left.ui_data.input_debug.as_ref(),
        right.ui_data.input_debug.as_ref(),
    ) {
        (Some(left_input), Some(right_input)) => {
            assert_eq!(left_input.sections, right_input.sections);
            assert!(!left_input.sections.is_empty());
            assert_eq!(
                left_input.progress_bars.len(),
                right_input.progress_bars.len()
            );
            for ((left_name, left_value), (right_name, right_value)) in left_input
                .progress_bars
                .iter()
                .zip(&right_input.progress_bars)
            {
                assert_eq!(left_name, right_name);
                assert_eq!(left_value.to_bits(), right_value.to_bits());
            }
        }
        (None, None) => {}
        _ => panic!("input data presence differs"),
    }

    match (
        left.ui_data.graphics_data.as_ref(),
        right.ui_data.graphics_data.as_ref(),
    ) {
        (
            Some(crate::debug::ConsoleGraphicsData::Sega8(left)),
            Some(crate::debug::ConsoleGraphicsData::Sega8(right)),
        ) => {
            assert_eq!(left.system, right.system);
            assert_bytes_equal("VRAM", &left.vram, &right.vram);
            assert_bytes_equal("CRAM", &left.cram, &right.cram);
            assert_bytes_equal("OAM", &left.oam, &right.oam);
            assert!(!left.vram.is_empty());
            assert!(!left.cram.is_empty());
            assert!(!left.oam.is_empty());
            assert_eq!(left.registers, right.registers);
            assert_eq!(left.status, right.status);
            assert_eq!(left.address, right.address);
            assert_eq!(left.code, right.code);
            assert_eq!(left.v_counter, right.v_counter);
            assert_eq!(left.h_counter, right.h_counter);
            assert_eq!(left.scanline, right.scanline);
            assert_eq!(left.scanline_cycle, right.scanline_cycle);
            assert_eq!(left.line_counter, right.line_counter);
            assert_eq!(left.frame_interrupt_enabled, right.frame_interrupt_enabled);
            assert_eq!(left.line_interrupt_enabled, right.line_interrupt_enabled);
            assert_eq!(left.interrupt_pending, right.interrupt_pending);
            assert_eq!(left.line_interrupt_pending, right.line_interrupt_pending);
            assert_eq!(left.display_enabled, right.display_enabled);
            assert_eq!(left.tms9918_mode, right.tms9918_mode);
            assert_eq!(left.sprite_table_base, right.sprite_table_base);
            assert_eq!(left.mode4, right.mode4);
            assert_eq!(left.tms9918, right.tms9918);
        }
        (
            Some(crate::debug::ConsoleGraphicsData::Gba(left)),
            Some(crate::debug::ConsoleGraphicsData::Gba(right)),
        ) => {
            assert_bytes_equal("GBA VRAM", &left.vram, &right.vram);
            assert_bytes_equal("GBA palette RAM", &left.palette_ram, &right.palette_ram);
            assert_bytes_equal("GBA OAM", &left.oam, &right.oam);
            assert!(!left.vram.is_empty());
            assert!(!left.palette_ram.is_empty());
            assert!(!left.oam.is_empty());
            assert_eq!(left.ppu, right.ppu);
        }
        _ => panic!("graphics data family differs"),
    }

    let left_disassembly = left
        .ui_data
        .disassembly_view
        .as_ref()
        .expect("disassembly data");
    let right_disassembly = right
        .ui_data
        .disassembly_view
        .as_ref()
        .expect("disassembly data");
    assert_eq!(left_disassembly.pc, right_disassembly.pc);
    assert_eq!(left_disassembly.mapping, right_disassembly.mapping);
    assert_eq!(
        left_disassembly.is_navigation_target,
        right_disassembly.is_navigation_target
    );
    assert_eq!(
        left_disassembly.is_static_target,
        right_disassembly.is_static_target
    );
    assert_eq!(
        left_disassembly.location_symbol,
        right_disassembly.location_symbol
    );
    assert_eq!(left_disassembly.lines.len(), right_disassembly.lines.len());
    assert!(!left_disassembly.lines.is_empty());
    for (left_line, right_line) in left_disassembly.lines.iter().zip(&right_disassembly.lines) {
        assert_eq!(left_line.address, right_line.address);
        assert_eq!(left_line.storage_offset, right_line.storage_offset);
        assert_eq!(left_line.bank, right_line.bank);
        assert_eq!(left_line.symbol, right_line.symbol);
        assert_eq!(left_line.control_target, right_line.control_target);
        assert_eq!(
            left_line.control_target_storage,
            right_line.control_target_storage
        );
        assert_eq!(
            left_line.control_target_bank,
            right_line.control_target_bank
        );
        assert_eq!(
            left_line.control_target_symbol,
            right_line.control_target_symbol
        );
        assert_eq!(left_line.source, right_line.source);
        assert_bytes_equal("disassembly bytes", &left_line.bytes, &right_line.bytes);
        assert_eq!(left_line.mnemonic, right_line.mnemonic);
    }
    assert_eq!(left_disassembly.breakpoints, right_disassembly.breakpoints);
    assert_eq!(
        left_disassembly.one_shot_breakpoints,
        right_disassembly.one_shot_breakpoints
    );
    assert_eq!(
        left_disassembly.rom_breakpoints,
        right_disassembly.rom_breakpoints
    );
    assert_eq!(
        left_disassembly.hit_rom_breakpoint,
        right_disassembly.hit_rom_breakpoint
    );

    let left_memory = left.ui_data.memory_page.as_ref().expect("memory page");
    let right_memory = right.ui_data.memory_page.as_ref().expect("memory page");
    assert_eq!(left_memory, right_memory);
    assert!(!left_memory.is_empty());
    let left_memory_search = left
        .ui_data
        .memory_search_results
        .as_ref()
        .expect("memory search");
    let right_memory_search = right
        .ui_data
        .memory_search_results
        .as_ref()
        .expect("memory search");
    assert_eq!(left_memory_search.len(), right_memory_search.len());
    assert!(!left_memory_search.is_empty());
    for (left_match, right_match) in left_memory_search.iter().zip(right_memory_search) {
        assert_eq!(left_match.address, right_match.address);
        assert_bytes_equal(
            "memory search match",
            &left_match.matched_bytes,
            &right_match.matched_bytes,
        );
    }

    let left_rom_page = left.ui_data.rom_page.as_ref().expect("ROM page");
    let right_rom_page = right.ui_data.rom_page.as_ref().expect("ROM page");
    assert_eq!(left_rom_page, right_rom_page);
    assert!(!left_rom_page.is_empty());
    assert_eq!(left.ui_data.rom_size, right.ui_data.rom_size);
    assert!(left.ui_data.rom_size > 0);
    let left_rom_search = left
        .ui_data
        .rom_search_results
        .as_ref()
        .expect("ROM search");
    let right_rom_search = right
        .ui_data
        .rom_search_results
        .as_ref()
        .expect("ROM search");
    assert_eq!(left_rom_search.len(), right_rom_search.len());
    assert!(!left_rom_search.is_empty());
    for (left_match, right_match) in left_rom_search.iter().zip(right_rom_search) {
        assert_eq!(left_match.offset, right_match.offset);
        assert_bytes_equal(
            "ROM search match",
            &left_match.matched_bytes,
            &right_match.matched_bytes,
        );
    }

    let left_trace = left
        .ui_data
        .instruction_trace
        .as_ref()
        .expect("instruction trace");
    let right_trace = right
        .ui_data
        .instruction_trace
        .as_ref()
        .expect("instruction trace");
    assert!(left_trace.enabled);
    assert_eq!(left_trace.enabled, right_trace.enabled);
    assert_eq!(left_trace.capacity, right_trace.capacity);
    assert_eq!(left_trace.retained, right_trace.retained);
    assert!(left_trace.retained > 0);
    assert_eq!(left_trace.oldest_sequence, right_trace.oldest_sequence);
    assert_eq!(left_trace.newest_sequence, right_trace.newest_sequence);
    assert_eq!(left_trace.entries, right_trace.entries);
    assert!(!left_trace.entries.is_empty());
}

fn assert_active_audio_results_match(left: &FrameResult, right: &FrameResult) {
    assert_frame_results_match(left, right);
    assert_rich_ui_data_match(left, right);
    assert!(left.ui_data.input_debug.is_some());
    assert!(!left.audio_samples.is_empty());
    assert_eq!(left.audio_samples.len() % 2, 0);
    assert!(left.audio_samples.iter().any(|sample| sample.abs() > 0.05));
    assert_eq!(left.audio_semantic_frames.len(), 1);
    let tone_0 = left.audio_semantic_frames[0]
        .voices
        .iter()
        .find(|voice| voice.channel == AudioChannelId(0))
        .expect("semantic frame should contain PSG tone 0");
    assert!(tone_0.active);
    assert!(tone_0.pitch_hz.is_some_and(|pitch| pitch > 0.0));
    assert!(tone_0.level.is_some_and(|level| level > 0.0));
    assert!(left.audio_timeline_discontinuities.is_empty());

    let left_apu = left
        .ui_data
        .apu_debug
        .as_ref()
        .expect("active APU capture should publish debug data");
    let right_apu = right
        .ui_data
        .apu_debug
        .as_ref()
        .expect("active APU capture should publish debug data");
    assert_eq!(left_apu.master_lines, right_apu.master_lines);
    assert_f32_equal(
        "APU master waveform",
        &left_apu.master_waveform,
        &right_apu.master_waveform,
    );
    assert_eq!(left_apu.master_waveform.len(), 512);
    assert!(
        left_apu
            .master_waveform
            .iter()
            .any(|sample| sample.abs() > 0.05)
    );
    assert_eq!(left_apu.channels.len(), right_apu.channels.len());
    for (left_channel, right_channel) in left_apu.channels.iter().zip(&right_apu.channels) {
        assert_eq!(left_channel.name, right_channel.name);
        assert_eq!(left_channel.enabled, right_channel.enabled);
        assert_eq!(left_channel.muted, right_channel.muted);
        assert_eq!(left_channel.register_lines, right_channel.register_lines);
        assert_eq!(left_channel.detail_line, right_channel.detail_line);
        assert_f32_equal(
            "APU channel waveform",
            &left_channel.waveform,
            &right_channel.waveform,
        );
    }
    assert!(left_apu.channels[0].enabled);
    assert_eq!(left_apu.channels[0].waveform.len(), 512);
    assert!(
        left_apu.channels[0]
            .waveform
            .iter()
            .any(|sample| sample.abs() > 0.05)
    );
}

fn assert_gba_results_match(left: &FrameResult, right: &FrameResult) {
    assert_frame_results_match(left, right);
    assert_rich_ui_data_match(left, right);
    assert!(left.ui_data.input_debug.is_none());
    assert!(!left.audio_samples.is_empty());
    assert_eq!(left.audio_samples.len() % 2, 0);
    assert_eq!(left.audio_semantic_frames.len(), 1);
    assert!(left.audio_timeline_discontinuities.is_empty());

    let left_apu = left.ui_data.apu_debug.as_ref().expect("GBA APU data");
    let right_apu = right.ui_data.apu_debug.as_ref().expect("GBA APU data");
    assert_eq!(left_apu.master_lines, right_apu.master_lines);
    assert_f32_equal(
        "GBA APU master waveform",
        &left_apu.master_waveform,
        &right_apu.master_waveform,
    );
    assert_eq!(left_apu.master_waveform.len(), 512);
    assert_eq!(left_apu.channels.len(), right_apu.channels.len());
    assert_eq!(left_apu.channels.len(), 6);
    for (left_channel, right_channel) in left_apu.channels.iter().zip(&right_apu.channels) {
        assert_eq!(left_channel.name, right_channel.name);
        assert_eq!(left_channel.enabled, right_channel.enabled);
        assert_eq!(left_channel.muted, right_channel.muted);
        assert_eq!(left_channel.register_lines, right_channel.register_lines);
        assert_eq!(left_channel.detail_line, right_channel.detail_line);
        assert_f32_equal(
            "GBA APU channel waveform",
            &left_channel.waveform,
            &right_channel.waveform,
        );
        assert_eq!(left_channel.waveform.len(), 512);
    }
}

fn gba_battery_and_rtc(
    backend: &EmuBackend,
) -> (
    Vec<u8>,
    Option<zeff_gba_core::hardware::cartridge::RtcDateTime>,
) {
    match backend {
        EmuBackend::Gba(backend) => (
            backend.emu.dump_battery_sram().unwrap(),
            backend.emu.rtc_date_time(),
        ),
        _ => panic!("expected GBA backend"),
    }
}

#[test]
fn sega8_detached_stepframes_preserves_primary_and_host_results() {
    let (control, _control_responses) = audio_test_loop();
    let (subject, _subject_responses) = audio_test_loop();
    assert_sega8_detached_stepframes_match(control, subject);
}

fn assert_sega8_detached_stepframes_match(mut control: EmuLoop, mut subject: EmuLoop) {
    subject.speculation.force_frames_for_test(1);

    assert!(control.handle_command(EmuCommand::StepFrames(Box::new(active_audio_frame_input()))));
    assert!(subject.handle_command(EmuCommand::StepFrames(Box::new(active_audio_frame_input()))));
    assert_eq!(control.speculation.committed_frames_for_test(), 1);
    assert_eq!(subject.speculation.completed_runs_for_test(), 1);
    assert_eq!(subject.speculation.committed_frames_for_test(), 1);

    let control_result = control.drain_rx.recv().unwrap();
    let subject_result = subject.drain_rx.recv().unwrap();
    assert_active_audio_results_match(&control_result, &subject_result);

    assert_eq!(
        control.backend.encode_state_bytes().unwrap(),
        subject.backend.encode_state_bytes().unwrap()
    );
    assert_eq!(control.backend.framebuffer(), subject.backend.framebuffer());
    assert_eq!(
        control.backend.battery_component_hash(),
        subject.backend.battery_component_hash()
    );
    let after_dirty_deadline = Instant::now() + Duration::from_secs(60);
    assert_eq!(
        control.battery_flush.wait_timeout(after_dirty_deadline),
        Some(Duration::ZERO)
    );
    assert_eq!(
        subject.battery_flush.wait_timeout(after_dirty_deadline),
        Some(Duration::ZERO)
    );

    let mut control_audio = Vec::new();
    let mut subject_audio = Vec::new();
    control.backend.drain_audio_samples_into(&mut control_audio);
    subject.backend.drain_audio_samples_into(&mut subject_audio);
    assert_eq!(control_audio, subject_audio);
    assert!(control_audio.is_empty());

    let mut expected_detached = control
        .backend
        .fork_detached_for_speculation()
        .expect("control Sega8 backend should fork");
    expected_detached.disable_audio_output();
    assert!(expected_detached.step_frames(1));
    assert_eq!(
        subject.shared_framebuffer.load_full().unwrap().as_slice(),
        expected_detached.framebuffer()
    );
    assert_eq!(
        control.shared_framebuffer.load_full().unwrap().as_slice(),
        control.backend.framebuffer()
    );
}

#[test]
fn sega8_failed_tcp_link_start_leaves_detached_stepframes_eligible() {
    let (control, _control_responses) = audio_test_loop();
    let (mut subject, subject_responses) = audio_test_loop();

    assert!(
        subject.handle_command(EmuCommand::StartTcpLink(TcpLinkMode::Host {
            bind_addr: "127.0.0.1:0".to_string(),
        }))
    );
    assert!(matches!(
        subject_responses.try_recv(),
        Ok(EmuResponse::LinkFailed(error))
            if error == "TCP link currently supports GB/GBC and WonderSwan/WSC only"
    ));
    assert!(matches!(
        subject_responses.try_recv(),
        Err(crossbeam_channel::TryRecvError::Empty)
    ));
    assert!(subject.pending_tcp_link.is_none());
    assert!(subject.tcp_link.is_none());
    assert!(subject.game_boy_replay_link.is_none());
    assert!(subject.wonder_swan_replay_link.is_none());
    assert!(!subject.periodic_battery_flush_blocked());

    assert_sega8_detached_stepframes_match(control, subject);
}

fn assert_sega8_detached_fallback(wrong_framebuffer_len: bool) {
    let (mut control, _control_responses) = audio_test_loop();
    let (mut subject, _subject_responses) = audio_test_loop();
    subject.speculation.force_frames_for_test(1);
    if wrong_framebuffer_len {
        subject.speculation.force_wrong_framebuffer_len_for_test();
    } else {
        subject.speculation.force_operational_failure_for_test();
    }

    assert!(control.handle_command(EmuCommand::StepFrames(Box::new(active_audio_frame_input()))));
    assert!(subject.handle_command(EmuCommand::StepFrames(Box::new(active_audio_frame_input()))));
    assert_eq!(control.speculation.committed_frames_for_test(), 1);
    assert_eq!(subject.speculation.completed_runs_for_test(), 0);
    assert_eq!(subject.speculation.committed_frames_for_test(), 1);

    let control_result = control.drain_rx.recv().unwrap();
    let subject_result = subject.drain_rx.recv().unwrap();
    assert_active_audio_results_match(&control_result, &subject_result);
    assert_eq!(
        control.backend.encode_state_bytes().unwrap(),
        subject.backend.encode_state_bytes().unwrap()
    );
    assert_eq!(control.backend.framebuffer(), subject.backend.framebuffer());
    assert_eq!(
        control.backend.battery_component_hash(),
        subject.backend.battery_component_hash()
    );

    let after_dirty_deadline = Instant::now() + Duration::from_secs(60);
    assert_eq!(
        control.battery_flush.wait_timeout(after_dirty_deadline),
        Some(Duration::ZERO)
    );
    assert_eq!(
        subject.battery_flush.wait_timeout(after_dirty_deadline),
        Some(Duration::ZERO)
    );

    let mut control_audio = Vec::new();
    let mut subject_audio = Vec::new();
    control.backend.drain_audio_samples_into(&mut control_audio);
    subject.backend.drain_audio_samples_into(&mut subject_audio);
    assert_eq!(control_audio, subject_audio);
    assert!(control_audio.is_empty());
    assert_eq!(
        subject.shared_framebuffer.load_full().unwrap().as_slice(),
        subject.backend.framebuffer()
    );
}

#[test]
fn sega8_detached_operational_failure_preserves_primary_and_host_results() {
    assert_sega8_detached_fallback(false);
}

#[test]
fn sega8_detached_wrong_framebuffer_length_commits_the_primary_frame() {
    assert_sega8_detached_fallback(true);
}

#[test]
fn gba_detached_stepframes_preserves_primary_and_host_results() {
    let (mut control, _control_responses) = gba_test_loop();
    let (mut subject, _subject_responses) = gba_test_loop();
    subject.speculation.force_frames_for_test(1);

    assert!(control.handle_command(EmuCommand::StepFrames(Box::new(gba_frame_input()))));
    assert!(subject.handle_command(EmuCommand::StepFrames(Box::new(gba_frame_input()))));
    assert_eq!(control.speculation.committed_frames_for_test(), 1);
    assert_eq!(subject.speculation.completed_runs_for_test(), 1);
    assert_eq!(subject.speculation.committed_frames_for_test(), 1);

    let control_result = control.drain_rx.recv().unwrap();
    let subject_result = subject.drain_rx.recv().unwrap();
    assert_gba_results_match(&control_result, &subject_result);
    assert_eq!(
        control.backend.encode_state_bytes().unwrap(),
        subject.backend.encode_state_bytes().unwrap()
    );
    assert_eq!(control.backend.framebuffer(), subject.backend.framebuffer());
    let (control_sram, control_rtc) = gba_battery_and_rtc(&control.backend);
    let (subject_sram, subject_rtc) = gba_battery_and_rtc(&subject.backend);
    assert_eq!(control_sram, subject_sram);
    assert_eq!(control_sram, gba_sram_bytes(control_sram.len()));
    assert_eq!(control_rtc, subject_rtc);
    assert_eq!(control_rtc, Some(gba_rtc()));
    assert_eq!(subject_rtc, Some(gba_rtc()));

    let after_dirty_deadline = Instant::now() + Duration::from_secs(60);
    assert_eq!(
        control.battery_flush.wait_timeout(after_dirty_deadline),
        Some(Duration::ZERO)
    );
    assert_eq!(
        subject.battery_flush.wait_timeout(after_dirty_deadline),
        Some(Duration::ZERO)
    );
    let mut control_audio = Vec::new();
    let mut subject_audio = Vec::new();
    control.backend.drain_audio_samples_into(&mut control_audio);
    subject.backend.drain_audio_samples_into(&mut subject_audio);
    assert_eq!(control_audio, subject_audio);
    assert!(control_audio.is_empty());

    let mut expected_detached = control.backend.fork_detached_for_speculation().unwrap();
    expected_detached.disable_audio_output();
    assert!(expected_detached.step_frames(1));
    assert_eq!(
        subject.shared_framebuffer.load_full().unwrap().as_slice(),
        expected_detached.framebuffer()
    );
    assert_eq!(
        control.shared_framebuffer.load_full().unwrap().as_slice(),
        control.backend.framebuffer()
    );
}

fn assert_gba_detached_fallback(wrong_framebuffer_len: bool) {
    let (mut control, _control_responses) = gba_test_loop();
    let (mut subject, _subject_responses) = gba_test_loop();
    subject.speculation.force_frames_for_test(1);
    if wrong_framebuffer_len {
        subject.speculation.force_wrong_framebuffer_len_for_test();
    } else {
        subject.speculation.force_operational_failure_for_test();
    }

    assert!(control.handle_command(EmuCommand::StepFrames(Box::new(gba_frame_input()))));
    assert!(subject.handle_command(EmuCommand::StepFrames(Box::new(gba_frame_input()))));
    assert_eq!(control.speculation.committed_frames_for_test(), 1);
    assert_eq!(subject.speculation.completed_runs_for_test(), 0);
    assert_eq!(subject.speculation.committed_frames_for_test(), 1);
    let control_result = control.drain_rx.recv().unwrap();
    let subject_result = subject.drain_rx.recv().unwrap();
    assert_gba_results_match(&control_result, &subject_result);
    assert_eq!(
        control.backend.encode_state_bytes().unwrap(),
        subject.backend.encode_state_bytes().unwrap()
    );
    assert_eq!(control.backend.framebuffer(), subject.backend.framebuffer());
    let control_battery_and_rtc = gba_battery_and_rtc(&control.backend);
    let subject_battery_and_rtc = gba_battery_and_rtc(&subject.backend);
    assert_eq!(control_battery_and_rtc, subject_battery_and_rtc);
    assert_eq!(control_battery_and_rtc.1, Some(gba_rtc()));
    assert_eq!(subject_battery_and_rtc.1, Some(gba_rtc()));
    let after_dirty_deadline = Instant::now() + Duration::from_secs(60);
    assert_eq!(
        control.battery_flush.wait_timeout(after_dirty_deadline),
        Some(Duration::ZERO)
    );
    assert_eq!(
        subject.battery_flush.wait_timeout(after_dirty_deadline),
        Some(Duration::ZERO)
    );
    let mut control_audio = Vec::new();
    let mut subject_audio = Vec::new();
    control.backend.drain_audio_samples_into(&mut control_audio);
    subject.backend.drain_audio_samples_into(&mut subject_audio);
    assert_eq!(control_audio, subject_audio);
    assert!(control_audio.is_empty());
    assert_eq!(
        subject.shared_framebuffer.load_full().unwrap().as_slice(),
        subject.backend.framebuffer()
    );
}

#[test]
fn gba_detached_stepframes_falls_back_on_operational_or_size_failure() {
    assert_gba_detached_fallback(false);
    assert_gba_detached_fallback(true);
}

struct TestTempRoot(PathBuf);

impl Drop for TestTempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn test_temp_root(label: &str) -> TestTempRoot {
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "zeff-boy-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&path).unwrap();
    TestTempRoot(path)
}

fn terminal_test_loop(
    root: &TestTempRoot,
    fail_generation_write: bool,
) -> (
    EmuLoop,
    crossbeam_channel::Receiver<EmuResponse>,
    PathBuf,
    PathBuf,
) {
    let generation_path = root.0.join("battery-generation.bin");
    let state_path = root.0.join("last.smsstate");
    let (emu_loop, responses) = sega8_test_loop_with_recovery(
        SMS_AUDIO_ROM,
        root.0.join("fixture.sms"),
        true,
        Some(RecoveryTestConfig {
            generation_path: generation_path.clone(),
            state_path: state_path.clone(),
            fail_generation_write,
        }),
        true,
    );
    assert!(!emu_loop.backend.save_ram_kind().is_battery_backed());
    (emu_loop, responses, generation_path, state_path)
}

fn assert_terminal_success_responses(
    responses: &crossbeam_channel::Receiver<EmuResponse>,
    state_path: &std::path::Path,
) {
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::SramFlushed(None)
    ));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::RecoverySaved(path) if path.as_path() == state_path
    ));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::ShutdownComplete
    ));
    assert!(matches!(
        responses.try_recv(),
        Err(crossbeam_channel::TryRecvError::Empty)
    ));
}

fn assert_no_sav_files(root: &std::path::Path) {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                assert_ne!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("sav")
                );
            }
        }
    }
}

fn decode_terminal_files(
    emu_loop: &EmuLoop,
    generation_path: &std::path::Path,
    state_path: &std::path::Path,
) -> (
    Vec<u8>,
    Vec<u8>,
    crate::save_paths::recovery_state::BatteryGenerationRecord,
    crate::save_paths::recovery_state::RecoveryStateEnvelope,
) {
    let generation_bytes = std::fs::read(generation_path).unwrap();
    let state_bytes = std::fs::read(state_path).unwrap();
    let media_sha256 = emu_loop.backend.rom_hash();
    let record = crate::save_paths::recovery_state::decode_battery_generation(
        &generation_bytes,
        media_sha256,
    )
    .unwrap();
    let discriminator = emu_loop.backend.recovery_discriminator();
    let envelope = crate::save_paths::recovery_state::decode_recovery_state(
        &state_bytes,
        crate::save_paths::recovery_state::RecoveryStateIdentity {
            system: emu_loop.backend.system().storage_subdir(),
            discriminator: &discriminator,
            media_sha256,
        },
    )
    .unwrap();
    assert_eq!(
        record.component_sha256,
        emu_loop.backend.battery_component_hash()
    );
    assert_eq!(record.generation, 0);
    assert_eq!(
        record.component_sha256,
        crate::save_paths::recovery_state::canonical_battery_component_hash(&[])
    );
    assert_eq!(envelope.system, emu_loop.backend.system().storage_subdir());
    assert_eq!(envelope.discriminator, discriminator);
    assert_eq!(envelope.media_sha256, media_sha256);
    assert_eq!(
        envelope.battery,
        crate::save_paths::recovery_state::BatteryGenerationWitness::Committed {
            generation: record.generation,
            component_sha256: record.component_sha256,
        }
    );
    (generation_bytes, state_bytes, record, envelope)
}

#[test]
fn sega8_detached_terminal_recovery_matches_control_without_battery_files() {
    let control_root = test_temp_root("sms-terminal-control");
    let subject_root = test_temp_root("sms-terminal-subject");
    let (mut control, control_responses, control_generation, control_state) =
        terminal_test_loop(&control_root, false);
    let (mut subject, subject_responses, subject_generation, subject_state) =
        terminal_test_loop(&subject_root, false);
    subject.speculation.force_frames_for_test(1);

    assert!(control.handle_command(EmuCommand::StepFrames(Box::new(active_audio_frame_input()))));
    assert!(subject.handle_command(EmuCommand::StepFrames(Box::new(active_audio_frame_input()))));
    let control_result = control.drain_rx.recv().unwrap();
    let subject_result = subject.drain_rx.recv().unwrap();
    assert_active_audio_results_match(&control_result, &subject_result);
    let control_payload = control.backend.encode_state_bytes().unwrap();
    let subject_payload = subject.backend.encode_state_bytes().unwrap();
    assert_eq!(control_payload, subject_payload);
    assert_eq!(control.speculation.completed_runs_for_test(), 0);
    assert_eq!(subject.speculation.completed_runs_for_test(), 1);

    assert!(!control.handle_command(EmuCommand::Shutdown));
    assert!(!subject.handle_command(EmuCommand::Shutdown));
    assert_terminal_success_responses(&control_responses, &control_state);
    assert_terminal_success_responses(&subject_responses, &subject_state);
    assert_eq!(control.speculation.completed_runs_for_test(), 0);
    assert_eq!(subject.speculation.completed_runs_for_test(), 1);

    let (control_generation_bytes, control_state_bytes, control_record, control_envelope) =
        decode_terminal_files(&control, &control_generation, &control_state);
    let (subject_generation_bytes, subject_state_bytes, subject_record, subject_envelope) =
        decode_terminal_files(&subject, &subject_generation, &subject_state);
    assert_eq!(control_generation_bytes, subject_generation_bytes);
    assert_eq!(control_state_bytes, subject_state_bytes);
    assert_eq!(control_record, subject_record);
    assert_eq!(control_envelope, subject_envelope);
    assert_eq!(control_envelope.native_payload, control_payload);
    assert_eq!(subject_envelope.native_payload, subject_payload);
    assert_no_sav_files(&control_root.0);
    assert_no_sav_files(&subject_root.0);
}

fn gba_terminal_test_loop(
    root: &TestTempRoot,
) -> (
    EmuLoop,
    crossbeam_channel::Receiver<EmuResponse>,
    PathBuf,
    PathBuf,
    PathBuf,
) {
    let generation_path = root.0.join("battery-generation.bin");
    let state_path = root.0.join("last.gbastate");
    let rom_path = root.0.join("fixture.gba");
    let sav_path = rom_path.with_extension("sav");
    let (emu_loop, responses) = gba_test_loop_with_recovery(
        rom_path,
        true,
        Some(RecoveryTestConfig {
            generation_path: generation_path.clone(),
            state_path: state_path.clone(),
            fail_generation_write: false,
        }),
    );
    assert!(emu_loop.backend.save_ram_kind().is_battery_backed());
    (emu_loop, responses, sav_path, generation_path, state_path)
}

fn assert_gba_terminal_success_responses(
    responses: &crossbeam_channel::Receiver<EmuResponse>,
    sav_path: &std::path::Path,
    state_path: &std::path::Path,
) {
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::SramFlushed(Some(path)) if path == sav_path.display().to_string()
    ));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::RecoverySaved(path) if path.as_path() == state_path
    ));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::ShutdownComplete
    ));
    assert!(matches!(
        responses.try_recv(),
        Err(crossbeam_channel::TryRecvError::Empty)
    ));
}

#[test]
fn gba_detached_terminal_sram_and_recovery_match_control() {
    let control_root = test_temp_root("gba-terminal-control");
    let subject_root = test_temp_root("gba-terminal-subject");
    let (mut control, control_responses, control_sav, control_generation, control_state) =
        gba_terminal_test_loop(&control_root);
    let (mut subject, subject_responses, subject_sav, subject_generation, subject_state) =
        gba_terminal_test_loop(&subject_root);
    subject.speculation.force_frames_for_test(1);

    assert!(control.handle_command(EmuCommand::StepFrames(Box::new(gba_frame_input()))));
    assert!(subject.handle_command(EmuCommand::StepFrames(Box::new(gba_frame_input()))));
    let control_result = control.drain_rx.recv().unwrap();
    let subject_result = subject.drain_rx.recv().unwrap();
    assert_gba_results_match(&control_result, &subject_result);
    let control_payload = control.backend.encode_state_bytes().unwrap();
    let subject_payload = subject.backend.encode_state_bytes().unwrap();
    assert_eq!(control_payload, subject_payload);
    assert_eq!(control.speculation.completed_runs_for_test(), 0);
    assert_eq!(subject.speculation.completed_runs_for_test(), 1);

    assert!(!control.handle_command(EmuCommand::Shutdown));
    assert!(!subject.handle_command(EmuCommand::Shutdown));
    assert_gba_terminal_success_responses(&control_responses, &control_sav, &control_state);
    assert_gba_terminal_success_responses(&subject_responses, &subject_sav, &subject_state);
    assert_eq!(control.speculation.completed_runs_for_test(), 0);
    assert_eq!(subject.speculation.completed_runs_for_test(), 1);

    let control_sram = std::fs::read(&control_sav).unwrap();
    let subject_sram = std::fs::read(&subject_sav).unwrap();
    assert_eq!(control_sram, subject_sram);
    assert_eq!(control_sram, gba_sram_bytes(control_sram.len()));
    assert_eq!(control_sram, gba_battery_and_rtc(&control.backend).0);
    assert_eq!(subject_sram, gba_battery_and_rtc(&subject.backend).0);
    assert_eq!(gba_battery_and_rtc(&control.backend).1, Some(gba_rtc()));
    assert_eq!(gba_battery_and_rtc(&subject.backend).1, Some(gba_rtc()));

    let control_generation_bytes = std::fs::read(&control_generation).unwrap();
    let subject_generation_bytes = std::fs::read(&subject_generation).unwrap();
    let control_state_bytes = std::fs::read(&control_state).unwrap();
    let subject_state_bytes = std::fs::read(&subject_state).unwrap();
    assert_eq!(control_generation_bytes, subject_generation_bytes);
    assert_eq!(control_state_bytes, subject_state_bytes);
    let media_sha256 = control.backend.rom_hash();
    let record = crate::save_paths::recovery_state::decode_battery_generation(
        &control_generation_bytes,
        media_sha256,
    )
    .unwrap();
    assert_eq!(
        record.component_sha256,
        control.backend.battery_component_hash()
    );
    let discriminator = control.backend.recovery_discriminator();
    let envelope = crate::save_paths::recovery_state::decode_recovery_state(
        &control_state_bytes,
        crate::save_paths::recovery_state::RecoveryStateIdentity {
            system: control.backend.system().storage_subdir(),
            discriminator: &discriminator,
            media_sha256,
        },
    )
    .unwrap();
    assert_eq!(
        envelope.battery,
        crate::save_paths::recovery_state::BatteryGenerationWitness::Committed {
            generation: record.generation,
            component_sha256: record.component_sha256,
        }
    );
    assert_eq!(envelope.native_payload, control_payload);
    assert_eq!(envelope.native_payload, subject_payload);
}

#[test]
fn sega8_detached_terminal_generation_failure_preserves_prior_recovery_envelope() {
    let root = test_temp_root("sms-terminal-generation-failure");
    let (mut emu_loop, responses, generation_path, state_path) = terminal_test_loop(&root, true);
    let media_sha256 = emu_loop.backend.rom_hash();
    let discriminator = emu_loop.backend.recovery_discriminator();
    let prior_envelope = crate::save_paths::recovery_state::RecoveryStateEnvelope {
        system: emu_loop.backend.system().storage_subdir().to_owned(),
        discriminator: discriminator.clone(),
        media_sha256,
        battery: crate::save_paths::recovery_state::BatteryGenerationWitness::Unknown,
        native_payload: emu_loop.backend.encode_state_bytes().unwrap(),
    };
    let prior_bytes =
        crate::save_paths::recovery_state::encode_recovery_state(&prior_envelope).unwrap();
    std::fs::write(&state_path, &prior_bytes).unwrap();
    emu_loop.speculation.force_frames_for_test(1);

    assert!(emu_loop.handle_command(EmuCommand::StepFrames(Box::new(active_audio_frame_input()))));
    emu_loop.drain_rx.recv().unwrap();
    assert_eq!(emu_loop.speculation.completed_runs_for_test(), 1);
    assert!(!emu_loop.handle_command(EmuCommand::Shutdown));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::SramFlushFailed(error)
            if error == "injected battery generation write failure"
    ));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::RecoverySaveFailed(error)
            if error == "injected battery generation write failure"
    ));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::ShutdownComplete
    ));
    assert!(matches!(
        responses.try_recv(),
        Err(crossbeam_channel::TryRecvError::Empty)
    ));
    assert_eq!(emu_loop.speculation.completed_runs_for_test(), 1);
    assert!(!generation_path.exists());
    assert_eq!(std::fs::read(&state_path).unwrap(), prior_bytes);
    let decoded = crate::save_paths::recovery_state::decode_recovery_state(
        &prior_bytes,
        crate::save_paths::recovery_state::RecoveryStateIdentity {
            system: emu_loop.backend.system().storage_subdir(),
            discriminator: &discriminator,
            media_sha256,
        },
    )
    .unwrap();
    assert_eq!(decoded, prior_envelope);
    assert_no_sav_files(&root.0);
}

#[test]
fn sega8_detached_stepframes_rejects_nonlocal_or_mutating_requests() {
    let (mut replay_timeline, _responses) = test_loop();
    replay_timeline.speculation.force_frames_for_test(1);
    let mut replay_timeline_input = frame_input(1);
    replay_timeline_input.speculation_blockers =
        SpeculationBlockers::from_app_for_test(true, false);
    assert!(
        replay_timeline.handle_command(EmuCommand::StepFrames(Box::new(replay_timeline_input)))
    );
    assert_eq!(replay_timeline.speculation.completed_runs_for_test(), 0);

    let (mut live_control, _responses) = test_loop();
    live_control.speculation.force_frames_for_test(1);
    let mut live_control_input = frame_input(1);
    live_control_input.speculation_blockers = SpeculationBlockers::from_app_for_test(false, true);
    assert!(live_control.handle_command(EmuCommand::StepFrames(Box::new(live_control_input))));
    assert_eq!(live_control.speculation.completed_runs_for_test(), 0);

    let (mut replay, _responses) = test_loop();
    replay.speculation.force_frames_for_test(1);
    let mut replay_input = frame_input(1);
    replay_input.replay_joypad_frames = Some(vec![crate::emu_thread::ReplayJoypadFrame::default()]);
    assert!(replay.handle_command(EmuCommand::StepFrames(Box::new(replay_input))));
    assert_eq!(replay.speculation.completed_runs_for_test(), 0);

    let (mut debugger, _responses) = test_loop();
    debugger.speculation.force_frames_for_test(1);
    let mut debugger_input = frame_input(1);
    debugger_input
        .debug_actions
        .memory_writes
        .push((0xC000, 0x5A));
    assert!(debugger.handle_command(EmuCommand::StepFrames(Box::new(debugger_input))));
    assert_eq!(debugger.speculation.completed_runs_for_test(), 0);

    let (mut batch, _responses) = test_loop();
    batch.speculation.force_frames_for_test(1);
    assert!(batch.handle_command(EmuCommand::StepFrames(Box::new(frame_input(2)))));
    assert_eq!(batch.speculation.completed_runs_for_test(), 0);

    let (mut uncapped, _responses) = test_loop();
    uncapped.speculation.force_frames_for_test(1);
    uncapped.uncapped_mode = true;
    assert!(uncapped.handle_command(EmuCommand::StepFrames(Box::new(frame_input(1)))));
    assert_eq!(uncapped.speculation.completed_runs_for_test(), 0);
}

#[test]
fn gba_detached_stepframes_rejects_nonlocal_or_mutating_requests() {
    let (mut replay_timeline, _responses) = gba_test_loop();
    replay_timeline.speculation.force_frames_for_test(1);
    let mut replay_timeline_input = gba_frame_input();
    replay_timeline_input.speculation_blockers =
        SpeculationBlockers::from_app_for_test(true, false);
    assert!(
        replay_timeline.handle_command(EmuCommand::StepFrames(Box::new(replay_timeline_input)))
    );
    assert_eq!(replay_timeline.speculation.completed_runs_for_test(), 0);

    let (mut live_control, _responses) = gba_test_loop();
    live_control.speculation.force_frames_for_test(1);
    let mut live_control_input = gba_frame_input();
    live_control_input.speculation_blockers = SpeculationBlockers::from_app_for_test(false, true);
    assert!(live_control.handle_command(EmuCommand::StepFrames(Box::new(live_control_input))));
    assert_eq!(live_control.speculation.completed_runs_for_test(), 0);

    let (mut replay, _responses) = gba_test_loop();
    replay.speculation.force_frames_for_test(1);
    let mut replay_input = gba_frame_input();
    replay_input.replay_joypad_frames = Some(vec![crate::emu_thread::ReplayJoypadFrame::default()]);
    assert!(replay.handle_command(EmuCommand::StepFrames(Box::new(replay_input))));
    assert_eq!(replay.speculation.completed_runs_for_test(), 0);

    let (mut debugger, _responses) = gba_test_loop();
    debugger.speculation.force_frames_for_test(1);
    let mut debugger_input = gba_frame_input();
    debugger_input
        .debug_actions
        .memory_writes
        .push((0x0200_0000, 0x5A));
    assert!(debugger.handle_command(EmuCommand::StepFrames(Box::new(debugger_input))));
    assert_eq!(debugger.speculation.completed_runs_for_test(), 0);

    let (mut batch, _responses) = gba_test_loop();
    batch.speculation.force_frames_for_test(1);
    let mut batch_input = gba_frame_input();
    batch_input.frames = 2;
    assert!(batch.handle_command(EmuCommand::StepFrames(Box::new(batch_input))));
    assert_eq!(batch.speculation.completed_runs_for_test(), 0);

    let (mut uncapped, _responses) = gba_test_loop();
    uncapped.speculation.force_frames_for_test(1);
    uncapped.uncapped_mode = true;
    assert!(uncapped.handle_command(EmuCommand::StepFrames(Box::new(gba_frame_input()))));
    assert_eq!(uncapped.speculation.completed_runs_for_test(), 0);
}

#[test]
fn direct_step_commands_are_inert_after_runtime_fault() {
    let (mut emu_loop, _responses) = test_loop();
    let frame_before = emu_loop.backend.frame_count();
    emu_loop.runtime_fault.latch(Some("fault".to_string()));

    assert!(emu_loop.handle_command(EmuCommand::StepFrames(Box::new(frame_input(3)))));
    let first = emu_loop.drain_rx.recv().unwrap();
    assert_eq!(first.runtime_fault.as_deref(), Some("fault"));
    assert_eq!(first.advanced_frames, 0);
    assert_eq!(emu_loop.backend.frame_count(), frame_before);

    assert!(emu_loop.handle_command(EmuCommand::StepFrames(Box::new(frame_input(3)))));
    let second = emu_loop.drain_rx.recv().unwrap();
    assert_eq!(second.runtime_fault, None);
    assert_eq!(second.advanced_frames, 0);
    assert_eq!(emu_loop.backend.frame_count(), frame_before);

    assert!(emu_loop.handle_command(EmuCommand::SetUncapped(true)));
    assert!(!emu_loop.uncapped_mode);
}

#[test]
fn uncapped_batch_size_command_clamps_invalid_settings() {
    let (mut emu_loop, _responses) = test_loop();

    assert!(emu_loop.handle_command(EmuCommand::SetUncappedBatchSize(0)));
    assert_eq!(emu_loop.uncapped_batch_size, 1);

    assert!(emu_loop.handle_command(EmuCommand::SetUncappedBatchSize(17)));
    assert_eq!(emu_loop.uncapped_batch_size, 17);

    assert!(emu_loop.handle_command(EmuCommand::SetUncappedBatchSize(usize::MAX)));
    assert_eq!(
        emu_loop.uncapped_batch_size,
        super::super::MAX_UNCAPPED_BATCH_SIZE
    );
}

#[test]
fn paused_worker_wait_tracks_the_dirty_save_deadline() {
    let (mut emu_loop, _responses) = test_loop();
    let start = Instant::now();
    let interval = super::super::persistence::BATTERY_FLUSH_INTERVAL;
    emu_loop.battery_flush = super::super::persistence::BatteryFlushSchedule::new(start);

    assert_eq!(emu_loop.command_wait_timeout(start), None);
    emu_loop.battery_flush.mark_potentially_dirty();
    assert_eq!(emu_loop.command_wait_timeout(start), Some(interval));
    assert_eq!(
        emu_loop.command_wait_timeout(start + interval),
        Some(Duration::ZERO)
    );
}

#[test]
fn due_no_data_flush_clears_the_dirty_schedule() {
    let (mut emu_loop, _responses) = test_loop();
    let start = Instant::now();
    let deadline = start + super::super::persistence::BATTERY_FLUSH_INTERVAL;
    emu_loop.battery_flush = super::super::persistence::BatteryFlushSchedule::new(start);
    emu_loop.battery_flush.mark_potentially_dirty();

    emu_loop.flush_battery_sram_if_due(deadline);

    assert_eq!(emu_loop.command_wait_timeout(deadline), None);
}

#[test]
fn live_tcp_link_defers_periodic_flush_until_disconnect() {
    let (mut emu_loop, _responses, _peer) = test_gb_loop_with_tcp();
    let start = Instant::now();
    let now = start + super::super::persistence::BATTERY_FLUSH_INTERVAL;
    emu_loop.battery_flush = super::super::persistence::BatteryFlushSchedule::new(start);
    emu_loop.battery_flush.mark_potentially_dirty();

    assert_eq!(emu_loop.command_wait_timeout(now), None);
    emu_loop.disconnect_tcp_link();
    assert_eq!(emu_loop.command_wait_timeout(now), Some(Duration::ZERO));
}

#[test]
fn pal_sega8_loop_uses_pal_pacing_and_rewind_duration() {
    let (emu_loop, _responses) = test_pal_sega8_loop();

    assert_eq!(emu_loop.backend.nominal_frame_duration_ns(), 20_000_000);
    assert_eq!(emu_loop.rewind_buffer.capacity(), 125);
}

#[test]
fn native_scheduler_tracks_a_pal_sega8_state_load() {
    let emu = zeff_sega8_core::emulator::Emulator::new_with_hint(
        &[0x00],
        44_100,
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
    )
    .unwrap();
    let thread = EmuThread::spawn(
        EmuBackend::from_sega8(emu, PathBuf::from("test.sms")),
        false,
    );
    assert_eq!(thread.nominal_frame_duration_ns(), 16_666_667);

    let pal = zeff_sega8_core::emulator::Emulator::new_with_hint_and_video_standard(
        &[0x00],
        44_100,
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
        zeff_sega8_core::hardware::timing::Sega8VideoStandard::Pal,
    )
    .unwrap();
    thread.send(EmuCommand::LoadStateBytes {
        state_bytes: pal.encode_state().unwrap(),
        buttons_pressed: 0,
        dpad_pressed: 0,
        replay_events: None,
        game_boy_link_start_state: None,
        game_boy_link_coordinator_start_state: None,
        game_boy_link_start_tick: None,
        wonder_swan_link_start_tick: None,
    });

    assert!(matches!(
        thread.recv(),
        Some(EmuResponse::LoadStateOk { .. })
    ));
    assert_eq!(thread.nominal_frame_duration_ns(), 20_000_000);
}

fn replay_link_state(
    pending_master_byte: Option<u8>,
    pending_master_response: Option<u8>,
    queued_master_action: Option<zeff_emu_common::replay::ReplayGameBoyLinkAction>,
) -> zeff_emu_common::replay::ReplayGameBoyLinkState {
    zeff_emu_common::replay::ReplayGameBoyLinkState {
        peer_present: true,
        pending_master_byte,
        pending_master_response,
        pending_master_completion_ready: false,
        queued_master_action,
        pending_passive_completion: None,
        serial_generation: 7,
    }
}

fn replay_coordinator(
    owner: zeff_emu_common::replay::ReplayGameBoyLinkCoordinatorOwner,
    reply: Option<zeff_emu_common::replay::ReplayGameBoyLinkReply>,
) -> zeff_emu_common::replay::ReplayGameBoyLinkCoordinatorState {
    zeff_emu_common::replay::ReplayGameBoyLinkCoordinatorState {
        transfer_id: 17,
        action: zeff_emu_common::replay::ReplayGameBoyLinkAction {
            out_byte: 0x12,
            clock_period_t_cycles: 4096,
            serial_generation: 7,
        },
        owner,
        reply,
    }
}

#[test]
fn replay_start_capture_accepts_replay_owned_master_transfer() {
    let state = replay_link_state(Some(0x12), None, None);
    let coordinator = replay_coordinator(
        zeff_emu_common::replay::ReplayGameBoyLinkCoordinatorOwner::ReplayAwaitingReply,
        None,
    );

    assert_eq!(
        game_boy_replay_start_capture_blocker(Some(coordinator), Some(state)),
        None
    );
}

#[test]
fn replay_start_capture_rejects_consumed_core_master_without_reply() {
    let state = replay_link_state(Some(0x12), None, None);

    assert!(game_boy_replay_start_capture_blocker(None, Some(state)).is_some());
}

#[test]
fn replay_start_capture_allows_queued_core_master_but_rejects_unowned_reply() {
    let queued = replay_link_state(
        Some(0x12),
        None,
        Some(zeff_emu_common::replay::ReplayGameBoyLinkAction {
            out_byte: 0x12,
            clock_period_t_cycles: 4096,
            serial_generation: 7,
        }),
    );
    let replied = replay_link_state(Some(0x12), Some(0x34), None);

    assert_eq!(
        game_boy_replay_start_capture_blocker(None, Some(queued)),
        None
    );
    assert!(game_boy_replay_start_capture_blocker(None, Some(replied)).is_some());
}

#[test]
fn replay_start_capture_accepts_core_owned_applied_reply() {
    let state = replay_link_state(Some(0x12), Some(0x34), None);
    let reply = zeff_emu_common::replay::ReplayGameBoyLinkReply {
        out_byte: 0x34,
        passive: true,
        serial_generation: 8,
    };
    let coordinator = replay_coordinator(
        zeff_emu_common::replay::ReplayGameBoyLinkCoordinatorOwner::CoreHasReply,
        Some(reply),
    );

    assert_eq!(
        game_boy_replay_start_capture_blocker(Some(coordinator), Some(state)),
        None
    );
}

#[test]
fn replay_start_capture_allows_passive_in_flight_completion() {
    let state = zeff_emu_common::replay::ReplayGameBoyLinkState {
        pending_passive_completion: Some(zeff_emu_common::replay::ReplayGameBoyPassiveCompletion {
            peer_byte: 0xAB,
            remaining_t_cycles: 2048,
        }),
        ..replay_link_state(None, None, None)
    };

    assert_eq!(
        game_boy_replay_start_capture_blocker(None, Some(state)),
        None
    );
}

#[test]
fn replay_load_disconnects_tcp_before_restoring_core_owned_master() {
    use zeff_emu_common::replay::{
        ReplayGameBoyLinkCoordinatorOwner, ReplayGameBoyLinkCoordinatorState,
        ReplayGameBoyLinkReply,
    };
    use zeff_gb_core::hardware::types::constants::{SERIAL_SB, SERIAL_SC};

    let (mut emu_loop, responses, _peer) = test_gb_loop_with_tcp();
    let EmuBackend::Gb(gb) = &mut emu_loop.backend else {
        unreachable!();
    };
    gb.emu.write_byte(SERIAL_SB, 0xAB);
    gb.emu.write_byte(SERIAL_SC, 0x81);
    gb.emu.set_game_boy_link_peer_present(true);
    let action = gb
        .emu
        .game_boy_link_replay_state()
        .queued_master_action
        .unwrap();
    let state = zeff_emu_common::replay::ReplayGameBoyLinkState {
        peer_present: true,
        pending_master_byte: Some(action.out_byte),
        pending_master_response: Some(0x34),
        pending_master_completion_ready: false,
        queued_master_action: None,
        pending_passive_completion: None,
        serial_generation: action.serial_generation,
    };
    let reply = ReplayGameBoyLinkReply {
        out_byte: 0x34,
        passive: true,
        serial_generation: 9,
    };
    let coordinator = ReplayGameBoyLinkCoordinatorState {
        transfer_id: 0x0100_0000_0000_0001,
        action,
        owner: ReplayGameBoyLinkCoordinatorOwner::CoreHasReply,
        reply: Some(reply),
    };
    let state_bytes = emu_loop.backend.encode_state_bytes().unwrap();
    let start_tick = emu_loop.backend.game_boy_cpu_cycles();

    assert!(emu_loop.handle_command(EmuCommand::LoadStateBytes {
        state_bytes,
        buttons_pressed: 0,
        dpad_pressed: 0,
        replay_events: Some(Vec::new()),
        game_boy_link_start_state: Some(state),
        game_boy_link_coordinator_start_state: Some(coordinator),
        game_boy_link_start_tick: start_tick,
        wonder_swan_link_start_tick: None,
    }));
    assert!(matches!(
        responses.recv().unwrap(),
        crate::emu_thread::EmuResponse::LoadStateOk { .. }
    ));
    assert_eq!(emu_loop.backend.game_boy_link_replay_state(), Some(state));
    assert!(emu_loop.tcp_link.is_none());
    assert!(emu_loop.game_boy_replay_link.is_none());
}

#[test]
fn failed_replay_tick_validation_preserves_tcp_and_core_state() {
    let (mut emu_loop, responses, _peer) = test_gb_loop_with_tcp();
    let before = emu_loop.backend.encode_state_bytes().unwrap();
    let tick = emu_loop.backend.game_boy_cpu_cycles().unwrap();

    assert!(emu_loop.handle_command(EmuCommand::LoadStateBytes {
        state_bytes: before.clone(),
        buttons_pressed: 0x0F,
        dpad_pressed: 0x0F,
        replay_events: Some(Vec::new()),
        game_boy_link_start_state: None,
        game_boy_link_coordinator_start_state: None,
        game_boy_link_start_tick: Some(tick.wrapping_add(1)),
        wonder_swan_link_start_tick: None,
    }));
    assert!(matches!(
        responses.recv().unwrap(),
        crate::emu_thread::EmuResponse::LoadStateFailed(_)
    ));
    assert!(emu_loop.tcp_link.is_some());
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), before);
    assert!(emu_loop.pending_audio_discontinuities.is_empty());
}

#[test]
fn media_ack_uses_apply_boundary_after_already_advanced_frames() {
    use crate::emu_thread::EmuResponse;
    use zeff_emu_common::media::MediaEvent;

    let (mut emu_loop, responses) = test_fds_loop();
    emu_loop.backend.step_frame();
    let apply_frame = emu_loop.backend.frame_count();
    let snapshot = emu_loop.backend.media_slot_snapshot().unwrap();

    assert!(
        emu_loop.handle_command(EmuCommand::ApplyMediaEvent(MediaEvent::SetWriteProtected {
            slot: snapshot.state.slot,
            write_protected: true,
        }))
    );
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::MediaEventApplied {
            frame_count,
            snapshot,
            ..
        } if frame_count == apply_frame && snapshot.state.write_protected
    ));
}

#[test]
fn state_load_discontinuity_is_not_started_after_validation_failure() {
    let (mut emu_loop, _responses) = test_loop();
    emu_loop.audio_recording_capture = AudioRecordingCapture {
        active: true,
        semantic: true,
    };
    let state_bytes = emu_loop.backend.encode_state_bytes().unwrap();

    assert!(emu_loop.handle_command(EmuCommand::LoadStateBytes {
        state_bytes,
        buttons_pressed: 0,
        dpad_pressed: 0,
        replay_events: None,
        game_boy_link_start_state: None,
        game_boy_link_coordinator_start_state: None,
        game_boy_link_start_tick: Some(1),
        wonder_swan_link_start_tick: None,
    }));
    assert!(emu_loop.pending_audio_discontinuities.is_empty());

    let mut pre_mutation_result = semantic_result();
    pre_mutation_result.audio_semantic_frames.clear();
    emu_loop.attach_audio_discontinuities(&mut pre_mutation_result);
    assert!(
        pre_mutation_result
            .audio_timeline_discontinuities
            .is_empty()
    );
    assert!(emu_loop.pending_audio_discontinuities.is_empty());

    let mut post_mutation_result = semantic_result();
    emu_loop.attach_audio_discontinuities(&mut post_mutation_result);
    assert!(
        post_mutation_result
            .audio_timeline_discontinuities
            .is_empty()
    );
    assert!(emu_loop.pending_audio_discontinuities.is_empty());
}

#[test]
fn failed_state_decode_does_not_start_a_semantic_epoch() {
    let (mut emu_loop, _responses) = test_loop();
    emu_loop.audio_recording_capture = AudioRecordingCapture {
        active: true,
        semantic: true,
    };

    assert!(emu_loop.handle_command(EmuCommand::LoadStateBytes {
        state_bytes: vec![0xFF],
        buttons_pressed: 0,
        dpad_pressed: 0,
        replay_events: None,
        game_boy_link_start_state: None,
        game_boy_link_coordinator_start_state: None,
        game_boy_link_start_tick: None,
        wonder_swan_link_start_tick: None,
    }));
    assert!(emu_loop.pending_audio_discontinuities.is_empty());
}
