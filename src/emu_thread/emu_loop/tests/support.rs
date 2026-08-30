use super::super::{EmuLoop, EmuLoopConfig};
use crate::audio_tooling::{
    AudioChannelId, AudioSemanticFrame, AudioVoiceClass, AudioVoiceState, NTSC_60_TEMPO_US_PER_BEAT,
};
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use crate::emu_thread::contract_tests::{SMS_AUDIO_ROM, gba_rtc, gba_sram_bytes, gba_test_rom};
use crate::emu_thread::recovery::RecoveryTestConfig;
use crate::emu_thread::{
    AudioConfig, AudioRecordingCapture, FrameInput, FrameResult, JoypadInput, MemorySearchRequest,
    RenderSettings, ReusableBuffers, SnapshotRequest, SpeculationBlockers, ZapperInput,
};
use crate::link::transport::TcpLinkTransport;
use crate::link::{LinkEndpointId, LinkSession, LinkSystemType, RemoteLink};
use std::net::TcpListener;
use std::path::PathBuf;

pub(super) fn test_loop() -> (
    EmuLoop,
    crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
) {
    sega8_test_loop(&[0x00], PathBuf::from("test.sms"))
}

pub(super) fn tas_nes_test_backend() -> EmuBackend {
    tas_nes_test_backend_from_rom(
        "tas-control-direct-nes",
        crate::test_support::build_nes_test_rom(),
    )
}

pub(super) fn tas_nes_test_backend_from_rom(label: &str, rom: Vec<u8>) -> EmuBackend {
    let root = crate::test_support::test_directory(label).unwrap();
    let path = root.path().join("control.nes");
    std::fs::write(&path, rom).unwrap();
    load_backend_from_rom_source(
        ActiveSystem::Nes,
        &path,
        &path,
        None,
        BackendLoadConfig {
            apply_mods: false,
            nes_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend
}

pub(super) fn tas_nes_test_loop() -> (
    EmuLoop,
    crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
) {
    tas_nes_test_loop_from_backend(tas_nes_test_backend())
}

pub(super) fn tas_nes_test_loop_from_backend(
    backend: EmuBackend,
) -> (
    EmuLoop,
    crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
) {
    tas_nes_test_loop_from_backend_with_recovery(backend, false, None)
}

pub(super) fn tas_nes_test_loop_with_recovery(
    root: &crate::test_support::TestDirectory,
) -> (
    EmuLoop,
    crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
    PathBuf,
    PathBuf,
) {
    let generation_path = root.path().join("battery-generation.bin");
    let state_path = root.path().join("last.nesstate");
    let loop_and_responses = tas_nes_test_loop_from_backend_with_recovery(
        tas_nes_test_backend(),
        true,
        Some(RecoveryTestConfig {
            generation_path: generation_path.clone(),
            state_path: state_path.clone(),
            fail_generation_write: false,
        }),
    );
    (
        loop_and_responses.0,
        loop_and_responses.1,
        generation_path,
        state_path,
    )
}

fn tas_nes_test_loop_from_backend_with_recovery(
    backend: EmuBackend,
    save_recovery_on_shutdown: bool,
    recovery: Option<RecoveryTestConfig>,
) -> (
    EmuLoop,
    crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
) {
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

pub(super) fn audio_test_loop() -> (
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

pub(super) fn gba_test_loop_with_recovery(
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

pub(super) fn gba_test_loop() -> (
    EmuLoop,
    crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
) {
    gba_test_loop_with_recovery(PathBuf::from("emerald-sram.gba"), false, None)
}

pub(super) fn sega8_test_loop(
    rom: &[u8],
    path: PathBuf,
) -> (
    EmuLoop,
    crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
) {
    sega8_test_loop_with_recovery(rom, path, false, None, false)
}

pub(super) fn sega8_test_loop_with_recovery(
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

pub(super) fn test_pal_sega8_loop() -> (
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

pub(super) fn test_fds_loop() -> (
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

pub(super) fn test_gb_loop_with_tcp() -> (
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

pub(super) fn semantic_result() -> FrameResult {
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

pub(super) fn frame_input(frames: usize) -> FrameInput {
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

pub(super) fn active_audio_frame_input() -> FrameInput {
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

pub(super) fn gba_frame_input() -> FrameInput {
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

pub(super) fn gba_battery_and_rtc(
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
