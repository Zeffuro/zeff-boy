use std::time::Duration;

use super::super::super::harness::{
    app_with_worker, live_ok, wait_for_linked, wait_for_recorded_frame,
};
use super::super::super::*;
use crate::emu_backend::ActiveSystem;
use crate::emu_backend::loader::DirectPceCdTasExecutionLoader;
use crate::emu_thread::EmuThread;
use crate::input::HostButton;
use crate::live_control::{LiveCommand, TasRecordMode};
use crate::platform::Instant;
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasEditorSession, TasSeekStateCache,
};

struct Fixture {
    root: crate::test_support::TestDirectory,
    loader: DirectPceCdTasExecutionLoader,
    system_card: &'static [u8],
    memory_base_catalog: crate::emu_backend::pce_profiles::TestMemoryBaseCatalogGuard,
    controller_catalog: crate::emu_backend::pce_profiles::TestControllerCatalogGuard,
}

fn fixture(label: &str) -> Fixture {
    let root = crate::test_support::test_directory(label).unwrap();
    let source_path = root.path().join("disc.cue");
    let fill = label.bytes().fold(0xE3, u8::wrapping_add);
    let mut disc = vec![fill; 4 * 2048];
    disc[0..4].copy_from_slice(&[0x4D, 0x42, fill, fill.rotate_left(1)]);
    std::fs::write(root.path().join("disc.bin"), disc).unwrap();
    std::fs::write(
        &source_path,
        b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
    )
    .unwrap();
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let firmware_sha256 = zeff_firmware::sha256_bytes(system_card);
    let base = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        source_path.clone(),
        system_card,
        firmware_sha256,
    );
    let disc_sha256 = base
        .load_fresh_backend()
        .unwrap()
        .pce()
        .unwrap()
        .normalized_disc_hash()
        .unwrap();
    let memory_base_catalog =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(disc_sha256);
    let controller_catalog =
        crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
            disc_sha256,
            zeff_pce_core::hardware::PceControllerMode::Multitap,
        );
    let loader = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
        source_path,
        system_card,
        firmware_sha256,
    );
    Fixture {
        root,
        loader,
        system_card,
        memory_base_catalog,
        controller_catalog,
    }
}

fn app_for_fixture(fixture: &Fixture, generation: u64) -> crate::app::App {
    let project = fixture.loader.create_project().unwrap();
    let backend = fixture
        .loader
        .load_editor_engine(&project)
        .unwrap()
        .into_backend();
    let pce = backend.pce().unwrap();
    assert_eq!(
        pce.memory_base_mode(),
        zeff_pce_core::hardware::PceMemoryBaseMode::Enabled
    );
    assert_eq!(
        pce.arcade_card_mode(),
        zeff_pce_core::hardware::PceArcadeCardMode::Disabled
    );
    let path = fixture.root.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&path, TasAutosaveConfig::default()).unwrap();
    let cache = TasSeekStateCache::open(fixture.root.path().join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, path, autosaves, cache).unwrap();
    let mut app = app_with_worker(
        EmuThread::spawn(backend, false),
        generation,
        ActiveSystem::Pce,
        fixture.root.path().join("disc.cue"),
    );
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session);
    app
}

#[test]
fn linked_app_records_memory_base_multitap_and_supports_restore_and_keep() {
    for keep in [false, true] {
        let fixture = fixture(if keep {
            "tas-pce-cd-memory-base-multitap-live-keep"
        } else {
            "tas-pce-cd-memory-base-multitap-live-restore"
        });
        let _catalogs = (&fixture.memory_base_catalog, &fixture.controller_catalog);
        let generation = if keep { 139 } else { 138 };
        let mut app = app_for_fixture(&fixture, generation);
        let snapshot = TasEditorControlSnapshot::capture(
            app.debug_windows.tas_editor.active_session().unwrap(),
        )
        .unwrap();
        app.tas_control
            .queue_acquire(generation, snapshot, TasControlStartMode::Preview)
            .unwrap();
        wait_for_linked(&mut app);
        for (player, key) in [
            (1, HostButton::A),
            (2, HostButton::B),
            (3, HostButton::Select),
            (4, HostButton::Start),
            (5, HostButton::Right),
        ] {
            live_ok(
                &mut app,
                LiveCommand::Button {
                    player,
                    key,
                    pressed: true,
                },
            );
        }
        live_ok(
            &mut app,
            LiveCommand::TasRecordFrame {
                mode: TasRecordMode::Replace,
            },
        );
        wait_for_recorded_frame(&mut app);
        assert_eq!(
            app.debug_windows
                .tas_editor
                .active_session()
                .unwrap()
                .selected_branch()
                .input_at(0)
                .players
                .map(|player| (player.buttons, player.dpad)),
            [(1, 0), (2, 0), (4, 0), (8, 0), (0, 1)]
        );
        live_ok(&mut app, LiveCommand::TasDisconnect { keep });
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.tas_control.state != TasControlState::Detached && Instant::now() < deadline {
            app.drain_emu_responses();
            std::thread::yield_now();
        }
        assert_eq!(app.tas_control.state, TasControlState::Detached);
    }
}

#[test]
fn memory_base_multitap_repair_reloads_and_restores_running_worker() {
    let fixture = fixture("tas-pce-cd-memory-base-multitap-repair");
    let _catalogs = (&fixture.memory_base_catalog, &fixture.controller_catalog);
    let firmware_sha256 = zeff_firmware::sha256_bytes(fixture.system_card);
    let _firmware = crate::emu_backend::loader::register_test_pce_cd_system_card(
        firmware_sha256,
        fixture.system_card,
    );
    let mut app = app_for_fixture(&fixture, 140);
    assert_eq!(
        live_ok(&mut app, LiveCommand::TasReloadGame)["repair_activated"],
        true
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(
        app.tas_control.state,
        TasControlState::AwaitingDecision { .. }
    ) && Instant::now() < deadline
    {
        app.drain_emu_responses();
        app.begin_queued_tas_control_acquire();
        let _ = live_ok(&mut app, LiveCommand::TasStatus);
        std::thread::yield_now();
    }
    assert!(matches!(
        app.tas_control.state,
        TasControlState::AwaitingDecision { .. }
    ));
    live_ok(&mut app, LiveCommand::TasDisconnect { keep: false });
    let deadline = Instant::now() + Duration::from_secs(5);
    while (app.tas_control.state != TasControlState::Detached
        || app.tas_repair_state() != crate::app::tas_control::repair::TasRepairState::Detached)
        && Instant::now() < deadline
    {
        app.drain_emu_responses();
        app.pump_tas_repair_resolution();
        std::thread::yield_now();
    }
    assert_eq!(app.tas_control.state, TasControlState::Detached);
    assert_eq!(
        app.tas_repair_state(),
        crate::app::tas_control::repair::TasRepairState::Detached
    );
}
