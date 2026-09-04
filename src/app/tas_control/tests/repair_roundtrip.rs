use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::harness::{app_with_worker, live_ok};
use crate::app::App;
use crate::app::tas_control::TasControlState;
use crate::app::tas_control::repair::{TasRepairResolution, TasRepairState};
use crate::emu_backend::loader::{DirectGbTasExecutionLoader, DirectNesTasExecutionLoader};
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use crate::emu_thread::{
    EmuCommand, EmuResponse, EmuThread, TasExecutionProfile, TasLoadedProfileObservation,
};
use crate::input::HostButton;
use crate::live_control::{LiveCommand, LiveReply, TasRecordMode};
use crate::tas_project::{TasDigest, TasProject};
use crate::test_support::write_zip;

const ORIGINAL_GENERATION: u64 = 40;

struct ExpectedSession {
    state: Vec<u8>,
    framebuffer: Vec<u8>,
    frame_count: u64,
    profile: TasLoadedProfileObservation,
}

struct RepairHarness {
    app: App,
    _root: crate::test_support::TestDirectory,
    original: ExpectedSession,
    repaired: ExpectedSession,
}

impl RepairHarness {
    fn new(label: &str) -> Self {
        let root = crate::test_support::test_directory(label).unwrap();
        let rom_path = root.path().join("repair-roundtrip.nes");
        std::fs::write(&rom_path, crate::test_support::build_nes_test_rom()).unwrap();

        let project_path = root.path().join("repair-roundtrip.ztas");
        DirectNesTasExecutionLoader::new(rom_path.clone(), Vec::new())
            .create_project_file(&project_path)
            .unwrap();

        let mut original_backend = load_backend(&rom_path);
        for _ in 0..3 {
            original_backend.step_frame();
        }
        original_backend.set_sample_rate(44_100);
        let original = expected_session(&original_backend);
        let worker = EmuThread::spawn(original_backend, false);
        let mut app = app_with_worker(
            worker,
            ORIGINAL_GENERATION,
            ActiveSystem::Nes,
            rom_path.clone(),
        );

        let opened = live_ok(&mut app, LiveCommand::TasOpenProject { path: project_path });
        assert_eq!(opened["project"]["frame_count"], 1);
        wait_for_readiness(&mut app, "reload_required");
        let selected = live_ok(&mut app, LiveCommand::TasSelectBoundary { boundary: 1 });
        assert_eq!(selected["project"]["cursor"], 1);

        let mut repaired_backend = load_backend(&rom_path);
        repaired_backend.step_frame();
        let repaired = expected_session(&repaired_backend);
        assert_ne!(original.state, repaired.state);

        Self {
            app,
            _root: root,
            original,
            repaired,
        }
    }

    fn reload_and_link(&mut self) {
        let reply = live_ok(&mut self.app, LiveCommand::TasReloadGame);
        assert_eq!(reply["repair_activated"], true);
        let status = &reply["tas"];
        assert_eq!(status["repair"]["state"], "active");
        assert_eq!(status["repair"]["original_generation"], ORIGINAL_GENERATION);
        assert_eq!(
            status["repair"]["repaired_generation"],
            ORIGINAL_GENERATION + 1
        );
        assert_eq!(status["repair"]["parked_frame_count"], 3);
        assert_eq!(
            status["repair"]["parked_state_sha256"],
            TasDigest::from_bytes(&self.original.state).to_hex()
        );
        assert_eq!(
            status["repair"]["parked_framebuffer_sha256"],
            TasDigest::from_bytes(&self.original.framebuffer).to_hex()
        );
        assert_eq!(status["live"]["state"], "unavailable");

        let TasRepairState::RepairedDetached {
            original_generation,
            repaired_generation,
            original_proof,
            ..
        } = self.app.tas_repair_state()
        else {
            panic!("reload should retain the parked original worker");
        };
        assert_eq!(original_generation, ORIGINAL_GENERATION);
        assert_eq!(repaired_generation, ORIGINAL_GENERATION + 1);
        assert_eq!(original_proof.frame_count, self.original.frame_count);
        assert_eq!(original_proof.loaded_profile, self.original.profile);
        assert_eq!(
            original_proof.framebuffer_len,
            self.original.framebuffer.len()
        );

        wait_for_live_state(&mut self.app, "acquiring");
        let linked = wait_for_live_state(&mut self.app, "linked");
        assert_eq!(linked["live"]["execution_boundary"], 1);
        assert_eq!(linked["repair"]["state"], "active");
        assert_eq!(self.app.emu_worker_generation, ORIGINAL_GENERATION + 1);
    }
}

#[test]
fn live_reload_connect_and_restore_round_trip_restores_exact_parked_session_once() {
    let mut harness = RepairHarness::new("tas-repair-live-restore");
    harness.reload_and_link();

    let returning = live_ok(&mut harness.app, LiveCommand::TasDisconnect { keep: false });
    assert_eq!(returning["live"]["state"], "returning");
    assert_eq!(returning["repair"]["state"], "active");

    let restored = wait_for_readiness(&mut harness.app, "reload_required");
    assert_eq!(restored["repair"]["state"], "detached");
    assert_eq!(harness.app.emu_worker_generation, ORIGINAL_GENERATION + 2);
    assert_eq!(capture_state(&harness.app), harness.original.state);
    assert_eq!(
        current_framebuffer(&harness.app),
        harness.original.framebuffer
    );
    assert_eq!(observe_profile(&harness.app), harness.original.profile);
    assert_eq!(harness.app.tas_repair_state(), TasRepairState::Detached);

    let state = capture_state(&harness.app);
    let generation = harness.app.emu_worker_generation;
    let repeated = live_reply(&mut harness.app, LiveCommand::TasDisconnect { keep: false });
    assert!(matches!(repeated, LiveReply::Error(_)));
    assert_eq!(capture_state(&harness.app), state);
    assert_eq!(harness.app.emu_worker_generation, generation);
    assert!(
        !harness
            .app
            .request_tas_repair_resolution(TasRepairResolution::Restore)
    );
}

#[test]
fn live_reload_connect_and_keep_round_trip_retains_repaired_position_once() {
    let mut harness = RepairHarness::new("tas-repair-live-keep");
    harness.reload_and_link();

    let keeping = live_ok(&mut harness.app, LiveCommand::TasDisconnect { keep: true });
    assert_eq!(keeping["live"]["state"], "keeping");
    assert_eq!(keeping["repair"]["state"], "active");

    let kept = wait_for_status(&mut harness.app, |status| {
        status["repair"]["state"] == "detached" && status["readiness"]["state"] == "ready"
    });
    assert_eq!(kept["repair"]["state"], "detached");
    assert_eq!(harness.app.emu_worker_generation, ORIGINAL_GENERATION + 1);
    assert_eq!(capture_state(&harness.app), harness.repaired.state);
    assert_eq!(
        current_framebuffer(&harness.app),
        harness.repaired.framebuffer
    );
    assert_eq!(observe_profile(&harness.app), harness.repaired.profile);
    assert_ne!(capture_state(&harness.app), harness.original.state);
    assert_eq!(harness.app.tas_repair_state(), TasRepairState::Detached);

    let state = capture_state(&harness.app);
    let generation = harness.app.emu_worker_generation;
    let repeated = live_reply(&mut harness.app, LiveCommand::TasDisconnect { keep: true });
    assert!(matches!(repeated, LiveReply::Error(_)));
    assert_eq!(capture_state(&harness.app), state);
    assert_eq!(harness.app.emu_worker_generation, generation);
    assert!(
        !harness
            .app
            .request_tas_repair_resolution(TasRepairResolution::Keep)
    );
}

fn load_backend(path: &Path) -> EmuBackend {
    load_backend_from_rom_source(
        ActiveSystem::Nes,
        path,
        path,
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

fn expected_session(backend: &EmuBackend) -> ExpectedSession {
    ExpectedSession {
        state: backend.encode_state_bytes().unwrap(),
        framebuffer: backend.framebuffer().to_vec(),
        frame_count: backend.frame_count(),
        profile: crate::emu_thread::observe_tas_repair_profile(
            backend,
            TasExecutionProfile::DirectNesCartridge,
        ),
    }
}

fn live_reply(app: &mut App, command: LiveCommand) -> LiveReply {
    app.handle_live_command_for_test(command)
}

fn live_status(app: &mut App) -> Value {
    live_ok(app, LiveCommand::TasStatus)
}

fn pump(app: &mut App) {
    app.drain_emu_responses();
    app.begin_queued_tas_control_acquire();
}

fn wait_for_live_state(app: &mut App, expected: &str) -> Value {
    wait_for_status(app, |status| status["live"]["state"] == expected)
}

fn wait_for_readiness(app: &mut App, expected: &str) -> Value {
    wait_for_status(app, |status| status["readiness"]["state"] == expected)
}

fn wait_for_status(app: &mut App, ready: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last = live_status(app);
    while !ready(&last) && Instant::now() < deadline {
        pump(app);
        last = live_status(app);
        std::thread::yield_now();
    }
    assert!(ready(&last), "timed out waiting for TAS state: {last}");
    last
}

fn capture_state(app: &App) -> Vec<u8> {
    let worker = app.emu_thread.as_ref().unwrap();
    assert!(worker.send_checked(EmuCommand::CaptureStateBytes));
    match worker.recv_checked().unwrap() {
        EmuResponse::StateCaptured(state) => state,
        _ => panic!("unexpected state-capture response"),
    }
}

fn observe_profile(app: &App) -> TasLoadedProfileObservation {
    let worker = app.emu_thread.as_ref().unwrap();
    assert!(worker.send_checked(EmuCommand::InspectTasReadiness {
        request_id: 900,
        profile: TasExecutionProfile::DirectNesCartridge,
    }));
    match worker.recv_checked().unwrap() {
        EmuResponse::TasReadinessObserved {
            request_id: 900,
            observation,
        } => *observation,
        _ => panic!("unexpected readiness response"),
    }
}

fn current_framebuffer(app: &App) -> Vec<u8> {
    app.emu_thread
        .as_ref()
        .unwrap()
        .shared_framebuffer()
        .load_full()
        .unwrap()
        .as_ref()
        .clone()
}

struct BatteryRepairHarness {
    app: App,
    _root: crate::test_support::TestDirectory,
    save_path: std::path::PathBuf,
    project_sram: Vec<u8>,
    original_state: Vec<u8>,
    rom_sha256: [u8; 32],
    generation_path: std::path::PathBuf,
    recovery_state_path: std::path::PathBuf,
    _game_gear_catalog: Option<crate::emu_backend::loader::TestGameGearBoardCatalogGuard>,
}

struct BatteryRepairPaths {
    source: std::path::PathBuf,
    rom: std::path::PathBuf,
    project: std::path::PathBuf,
    save: std::path::PathBuf,
}

impl BatteryRepairHarness {
    fn direct(label: &str) -> Self {
        let root = crate::test_support::test_directory(label).unwrap();
        let source_path = root.path().join("battery.nes");
        let save_path = source_path.with_extension("sav");
        let project_path = root.path().join("battery.ztas");
        let mut rom = crate::test_support::build_nes_battery_test_rom();
        *rom.last_mut().unwrap() ^= 0x01;
        let project_sram = crate::test_support::nes_battery_test_bytes(&rom, 0xA5);
        std::fs::write(&source_path, &rom).unwrap();
        std::fs::write(&save_path, &project_sram).unwrap();
        DirectNesTasExecutionLoader::new(source_path.clone(), Vec::new())
            .create_project_file(&project_path)
            .unwrap();
        let original_sram = crate::test_support::nes_battery_test_bytes(&rom, 0x3C);
        std::fs::write(&save_path, original_sram).unwrap();
        Self::finish(
            root,
            BatteryRepairPaths {
                source: source_path.clone(),
                rom: source_path,
                project: project_path,
                save: save_path,
            },
            None,
            project_sram,
        )
    }

    fn zip(label: &str) -> Self {
        let root = crate::test_support::test_directory(label).unwrap();
        let source_path = root.path().join("battery.zip");
        let rom_path = source_path.join("folder/battery.nes");
        let save_path = source_path.with_extension("sav");
        let project_path = root.path().join("battery.ztas");
        let rom = crate::test_support::build_nes_battery_test_rom();
        let project_sram = crate::test_support::nes_battery_test_bytes(&rom, 0x6D);
        write_zip(&source_path, &[("folder/battery.nes", &rom)]).unwrap();
        std::fs::write(&save_path, &project_sram).unwrap();
        DirectNesTasExecutionLoader::new_zip(
            source_path.clone(),
            Some(rom_path.clone()),
            Vec::new(),
        )
        .create_project_file(&project_path)
        .unwrap();
        let original_sram = crate::test_support::nes_battery_test_bytes(&rom, 0xC7);
        std::fs::write(&save_path, original_sram).unwrap();
        Self::finish(
            root,
            BatteryRepairPaths {
                source: source_path,
                rom: rom_path,
                project: project_path,
                save: save_path,
            },
            Some(rom),
            project_sram,
        )
    }

    fn gb_direct(label: &str) -> Self {
        let root = crate::test_support::test_directory(label).unwrap();
        let source_path = root.path().join("battery.gb");
        let save_path = source_path.with_extension("sav");
        let project_path = root.path().join("battery.ztas");
        let rom = gb_battery_test_rom();
        let project_sram = vec![0x5A; 8 * 1024];
        std::fs::write(&source_path, &rom).unwrap();
        std::fs::write(&save_path, &project_sram).unwrap();
        DirectGbTasExecutionLoader::new(source_path.clone(), Vec::new())
            .create_project_file(&project_path)
            .unwrap();
        std::fs::write(&save_path, vec![0xC3; project_sram.len()]).unwrap();
        Self::finish_gb(
            root,
            BatteryRepairPaths {
                source: source_path.clone(),
                rom: source_path,
                project: project_path,
                save: save_path,
            },
            None,
            project_sram,
            None,
            zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::ForceDmg,
        )
    }

    fn gb_zip(label: &str) -> Self {
        let root = crate::test_support::test_directory(label).unwrap();
        let source_path = root.path().join("battery.zip");
        let member_name = "folder/battery.gb";
        let rom_path = source_path.join(member_name);
        let save_path = source_path.with_extension("sav");
        let project_path = root.path().join("battery.ztas");
        let rom = gb_battery_test_rom();
        let project_sram = vec![0x96; 8 * 1024];
        write_zip(&source_path, &[(member_name, &rom)]).unwrap();
        std::fs::write(&save_path, &project_sram).unwrap();
        DirectGbTasExecutionLoader::new_zip(
            source_path.clone(),
            Some(rom_path.clone()),
            Vec::new(),
        )
        .create_project_file(&project_path)
        .unwrap();
        std::fs::write(&save_path, vec![0x2D; project_sram.len()]).unwrap();
        let archive = std::fs::read(&source_path).unwrap();
        Self::finish_gb(
            root,
            BatteryRepairPaths {
                source: source_path,
                rom: rom_path,
                project: project_path,
                save: save_path,
            },
            Some(rom),
            project_sram,
            Some((
                TasDigest::from_bytes(&archive).0,
                archive.len(),
                crate::emu_backend::loader::zip_gb_battery_tas_sync_config_sha256(member_name).0,
            )),
            zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::ForceDmg,
        )
    }

    fn finish(
        root: crate::test_support::TestDirectory,
        paths: BatteryRepairPaths,
        preloaded_rom: Option<Vec<u8>>,
        project_sram: Vec<u8>,
    ) -> Self {
        let original_backend = load_backend_from_rom_source(
            ActiveSystem::Nes,
            &paths.source,
            &paths.rom,
            preloaded_rom,
            BackendLoadConfig {
                apply_mods: false,
                nes_load_battery_sram: true,
                ..BackendLoadConfig::default()
            },
        )
        .unwrap()
        .backend;
        let original_state = original_backend.encode_state_bytes().unwrap();
        let rom_sha256 = original_backend.rom_hash();
        let worker = EmuThread::spawn(original_backend, false);
        let mut app = app_with_worker(
            worker,
            ORIGINAL_GENERATION,
            ActiveSystem::Nes,
            paths.rom.clone(),
        );
        app.rom_info.source_path = Some(paths.source);
        app.rom_info.rom_path = Some(paths.rom);
        let generation_path = root.path().join("battery-generation.bin");
        let recovery_state_path = root.path().join("recovery-state.zst");
        app.tas_repair
            .set_repaired_recovery_for_test(crate::emu_thread::RecoveryTestConfig {
                generation_path: generation_path.clone(),
                state_path: recovery_state_path.clone(),
                fail_generation_write: false,
            });
        let opened = live_ok(
            &mut app,
            LiveCommand::TasOpenProject {
                path: paths.project.clone(),
            },
        );
        assert_eq!(opened["project"]["frame_count"], 1);
        wait_for_readiness(&mut app, "reload_required");
        live_ok(&mut app, LiveCommand::TasSelectBoundary { boundary: 1 });
        Self {
            app,
            _root: root,
            save_path: paths.save,
            project_sram,
            original_state,
            rom_sha256,
            generation_path,
            recovery_state_path,
            _game_gear_catalog: None,
        }
    }

    fn finish_gb(
        root: crate::test_support::TestDirectory,
        paths: BatteryRepairPaths,
        preloaded_rom: Option<Vec<u8>>,
        project_sram: Vec<u8>,
        tas_source_media: Option<([u8; 32], usize, [u8; 32])>,
        hardware_mode_preference:
            zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference,
    ) -> Self {
        let original_backend = load_backend_from_rom_source(
            ActiveSystem::GameBoy,
            &paths.source,
            &paths.rom,
            preloaded_rom,
            BackendLoadConfig {
                apply_mods: false,
                gb_hardware_mode_preference: hardware_mode_preference,
                gb_load_battery_sram: true,
                gb_tas_source_media: tas_source_media,
                ..BackendLoadConfig::default()
            },
        )
        .unwrap()
        .backend;
        let original_state = original_backend.encode_state_bytes().unwrap();
        let rom_sha256 = original_backend.rom_hash();
        let worker = EmuThread::spawn(original_backend, false);
        let mut app = app_with_worker(
            worker,
            ORIGINAL_GENERATION,
            ActiveSystem::GameBoy,
            paths.rom.clone(),
        );
        app.rom_info.source_path = Some(paths.source);
        app.rom_info.rom_path = Some(paths.rom);
        let generation_path = root.path().join("battery-generation.bin");
        let recovery_state_path = root.path().join("recovery-state.zst");
        app.tas_repair
            .set_repaired_recovery_for_test(crate::emu_thread::RecoveryTestConfig {
                generation_path: generation_path.clone(),
                state_path: recovery_state_path.clone(),
                fail_generation_write: false,
            });
        let opened = live_ok(
            &mut app,
            LiveCommand::TasOpenProject {
                path: paths.project.clone(),
            },
        );
        assert_eq!(opened["project"]["frame_count"], 1);
        wait_for_readiness(&mut app, "reload_required");
        live_ok(&mut app, LiveCommand::TasSelectBoundary { boundary: 1 });
        Self {
            app,
            _root: root,
            save_path: paths.save,
            project_sram,
            original_state,
            rom_sha256,
            generation_path,
            recovery_state_path,
            _game_gear_catalog: None,
        }
    }

    fn reload_and_link(&mut self) {
        let reply = live_ok(&mut self.app, LiveCommand::TasReloadGame);
        assert_eq!(reply["repair_activated"], true);
        wait_for_live_state(&mut self.app, "acquiring");
        wait_for_live_state(&mut self.app, "linked");
    }
}

#[test]
fn direct_battery_restore_keeps_the_sidecar_and_resumes_the_original() {
    assert_battery_restore(BatteryRepairHarness::direct(
        "tas-repair-battery-direct-restore",
    ));
}

#[test]
fn zip_battery_restore_keeps_the_archive_sidecar_and_resumes_the_original() {
    assert_battery_restore(BatteryRepairHarness::zip("tas-repair-battery-zip-restore"));
}

#[test]
fn direct_gb_battery_restore_keeps_the_sidecar_and_resumes_the_original() {
    assert_battery_restore(BatteryRepairHarness::gb_direct(
        "tas-repair-gb-battery-direct-restore",
    ));
}

#[test]
fn zip_gb_battery_restore_keeps_the_archive_sidecar_and_resumes_the_original() {
    assert_battery_restore(BatteryRepairHarness::gb_zip(
        "tas-repair-gb-battery-zip-restore",
    ));
}

fn assert_battery_restore(mut harness: BatteryRepairHarness) {
    let before = std::fs::read(&harness.save_path).unwrap();
    harness.reload_and_link();

    live_ok(&mut harness.app, LiveCommand::TasDisconnect { keep: false });
    wait_for_readiness(&mut harness.app, "reload_required");

    assert_eq!(std::fs::read(&harness.save_path).unwrap(), before);
    assert_eq!(capture_state(&harness.app), harness.original_state);
}

#[test]
fn zip_battery_keep_publishes_the_project_candidate_to_the_archive_sidecar() {
    assert_battery_keep(
        BatteryRepairHarness::zip("tas-repair-battery-zip-keep"),
        true,
    );
}

#[test]
fn direct_battery_keep_publishes_the_project_candidate() {
    assert_battery_keep(
        BatteryRepairHarness::direct("tas-repair-battery-direct-keep"),
        false,
    );
}

#[test]
fn direct_gb_battery_keep_publishes_the_project_candidate_and_generation() {
    assert_battery_keep(
        BatteryRepairHarness::gb_direct("tas-repair-gb-battery-direct-keep"),
        true,
    );
}

#[test]
fn zip_gb_battery_keep_publishes_to_the_archive_sidecar() {
    assert_battery_keep(
        BatteryRepairHarness::gb_zip("tas-repair-gb-battery-zip-keep"),
        false,
    );
}

#[test]
fn direct_gb_rom_ram_battery_repaired_session_records_input() {
    record_battery_frame(
        BatteryRepairHarness::gb_direct("tas-repair-gb-rom-ram-battery-record"),
        true,
    );
}

#[test]
fn zip_cgb_rom_ram_battery_repaired_session_records_input() {
    record_battery_frame(
        BatteryRepairHarness::cgb_zip("tas-repair-cgb-rom-ram-battery-record"),
        false,
    );
}

fn record_battery_frame(mut harness: BatteryRepairHarness, keep: bool) {
    harness.reload_and_link();
    live_ok(
        &mut harness.app,
        LiveCommand::Button {
            player: 1,
            key: HostButton::A,
            pressed: true,
        },
    );
    live_ok(
        &mut harness.app,
        LiveCommand::TasSetRealtimeRecording { active: true },
    );
    live_ok(
        &mut harness.app,
        LiveCommand::TasRecordFrame {
            mode: TasRecordMode::Replace,
        },
    );
    live_ok(
        &mut harness.app,
        LiveCommand::TasSetRealtimeRecording { active: false },
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(
        harness.app.tas_control.state,
        TasControlState::AwaitingDecision {
            candidate_executed_project_frames,
            candidate_frame_count,
            ..
        } if candidate_executed_project_frames == 2 && candidate_frame_count == 2
    ) && Instant::now() < deadline
    {
        harness.app.drain_emu_responses();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        matches!(
            harness.app.tas_control.state,
            TasControlState::AwaitingDecision {
                candidate_executed_project_frames,
                candidate_frame_count,
                ..
            } if candidate_executed_project_frames == 2 && candidate_frame_count == 2
        ),
        "unexpected TAS recording state: {:?}",
        harness.app.tas_control.state
    );
    let session = harness
        .app
        .debug_windows
        .tas_editor
        .active_session()
        .unwrap();
    assert_eq!(session.selected_branch().frame_count(), 2);
    assert_eq!(
        session.selected_branch().input_at(1).players[0].buttons,
        0x01
    );
    assert_eq!(session.selected_branch().input_at(1).players[0].dpad, 0);

    live_ok(&mut harness.app, LiveCommand::TasDisconnect { keep });
    wait_for_readiness(&mut harness.app, "reload_required");
    if keep {
        let (project_path, project) = {
            let session = harness
                .app
                .debug_windows
                .tas_editor
                .active_session()
                .unwrap();
            (
                session.manual_path().to_path_buf(),
                session.project().clone(),
            )
        };
        project.save_atomic(&project_path).unwrap();
        let reopened = TasProject::load(&project_path).unwrap();
        assert_eq!(reopened.branch("main").unwrap().frame_count(), 2);
        assert_eq!(
            reopened.branch("main").unwrap().input_at(1).players[0].buttons,
            0x01
        );
    }
}

fn assert_battery_keep(mut harness: BatteryRepairHarness, verify_generation: bool) {
    harness.reload_and_link();

    live_ok(&mut harness.app, LiveCommand::TasDisconnect { keep: true });
    wait_for_readiness(&mut harness.app, "reload_required");

    assert_eq!(
        std::fs::read(&harness.save_path).unwrap(),
        harness.project_sram
    );
    if !verify_generation {
        return;
    }
    let generation_bytes = crate::platform::read_save_data(&harness.generation_path)
        .unwrap()
        .expect("Keep should publish a battery generation record");
    let generation = crate::save_paths::recovery_state::decode_battery_generation(
        &generation_bytes,
        harness.rom_sha256,
    )
    .expect("Keep generation record should be valid");
    assert_eq!(
        generation.component_sha256,
        crate::save_paths::recovery_state::canonical_battery_component_hash(&[(
            crate::save_paths::SRAM_COMPONENT,
            harness.project_sram.as_slice(),
        )])
    );
}

fn assert_uncertain_keep(mut harness: BatteryRepairHarness) {
    harness
        .app
        .tas_repair
        .set_repaired_recovery_for_test(crate::emu_thread::RecoveryTestConfig {
            generation_path: harness.generation_path.clone(),
            state_path: harness.recovery_state_path.clone(),
            fail_generation_write: true,
        });
    harness.reload_and_link();

    live_ok(&mut harness.app, LiveCommand::TasDisconnect { keep: true });
    wait_for_readiness(&mut harness.app, "reload_required");

    assert_eq!(
        std::fs::read(&harness.save_path).unwrap(),
        harness.project_sram
    );
    assert!(!harness.generation_path.exists());
    assert_eq!(harness.app.tas_repair_state(), TasRepairState::Detached);
    assert!(
        !harness
            .app
            .request_tas_repair_resolution(TasRepairResolution::Restore)
    );
    assert_ne!(capture_state(&harness.app), harness.original_state);
}

fn assert_cas_conflict(mut harness: BatteryRepairHarness, fill: u8) {
    harness.reload_and_link();
    let conflict = vec![fill; harness.project_sram.len()];
    std::fs::write(&harness.save_path, &conflict).unwrap();

    live_ok(&mut harness.app, LiveCommand::TasDisconnect { keep: true });
    let failed = wait_for_status(&mut harness.app, |status| {
        status["live"]["state"] == "ready" && status["repair"]["state"] == "active"
    });
    assert_eq!(failed["repair"]["state"], "active");
    assert_eq!(std::fs::read(&harness.save_path).unwrap(), conflict);

    assert!(
        harness
            .app
            .request_tas_repair_resolution(TasRepairResolution::Restore)
    );
    harness.app.pump_tas_repair_resolution();
    wait_for_readiness(&mut harness.app, "reload_required");
    assert_eq!(std::fs::read(&harness.save_path).unwrap(), conflict);
    assert_eq!(capture_state(&harness.app), harness.original_state);
}

#[test]
fn uncertain_generation_publication_keeps_repaired_ownership_and_discards_original() {
    assert_uncertain_keep(BatteryRepairHarness::direct("tas-repair-battery-uncertain"));
}

#[test]
fn uncertain_gb_generation_publication_keeps_repaired_ownership() {
    assert_uncertain_keep(BatteryRepairHarness::gb_direct(
        "tas-repair-gb-battery-uncertain",
    ));
}

#[test]
fn battery_keep_conflict_is_not_published_and_restore_remains_available() {
    assert_cas_conflict(
        BatteryRepairHarness::direct("tas-repair-battery-conflict"),
        0x7E,
    );
}

#[test]
fn gb_battery_keep_conflict_preserves_restore_authority() {
    assert_cas_conflict(
        BatteryRepairHarness::gb_direct("tas-repair-gb-battery-conflict"),
        0xE1,
    );
}

fn gb_battery_test_rom() -> Vec<u8> {
    let mut rom = crate::test_support::build_gb_test_rom();
    rom[0x147] = 0x09;
    rom[0x148] = 0x00;
    rom[0x149] = 0x02;
    rom
}

mod cgb_battery;
mod game_gear_battery;
mod gba_battery;
mod ws_battery;
