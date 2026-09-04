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

fn fixture(
    label: &str,
) -> (
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
    crate::emu_backend::pce_profiles::TestControllerCatalogGuard,
    &'static [u8],
) {
    let root = crate::test_support::test_directory(label).unwrap();
    let source_path = root.path().join("disc.cue");
    std::fs::write(
        root.path().join("disc.bin"),
        vec![label.bytes().fold(0xD3, u8::wrapping_add); 4 * 2048],
    )
    .unwrap();
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
    let source_disc_sha256 = base
        .load_fresh_backend()
        .unwrap()
        .pce()
        .unwrap()
        .normalized_disc_hash()
        .unwrap();
    let catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        source_disc_sha256,
        zeff_pce_core::hardware::PceControllerMode::Multitap,
    );
    let stack = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &source_path,
        vec![("tas.ppf".to_owned(), super::super::ppf1(0, &[0xA5]))],
    )
    .unwrap();
    let loader = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_and_ppf_stack(
        source_path,
        system_card,
        firmware_sha256,
        stack,
    );
    (root, loader, catalog, system_card)
}

#[test]
fn linked_app_records_ppf_multitap_and_supports_restore_and_keep() {
    for keep in [false, true] {
        let (root, loader, _catalog, _) = fixture(if keep {
            "tas-pce-cd-ppf-multitap-live-keep"
        } else {
            "tas-pce-cd-ppf-multitap-live-restore"
        });
        let project = loader.create_project().unwrap();
        let backend = loader.load_editor_engine(&project).unwrap().into_backend();
        let path = root.path().join("movie.ztas");
        let autosaves =
            TasAutosaveStore::beside_manual_save(&path, TasAutosaveConfig::default()).unwrap();
        let cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
        let session = TasEditorSession::new(project, path, autosaves, cache).unwrap();
        let mut app = app_with_worker(
            EmuThread::spawn(backend, false),
            if keep { 131 } else { 130 },
            ActiveSystem::Pce,
            root.path().join("disc.cue"),
        );
        app.debug_windows
            .tas_editor
            .install_verified_export_session(session);
        let snapshot = TasEditorControlSnapshot::capture(
            app.debug_windows.tas_editor.active_session().unwrap(),
        )
        .unwrap();
        app.tas_control
            .queue_acquire(
                if keep { 131 } else { 130 },
                snapshot,
                TasControlStartMode::Preview,
            )
            .unwrap();
        wait_for_linked(&mut app);
        for (player, key) in [
            (1, HostButton::A),
            (3, HostButton::Select),
            (5, HostButton::Left),
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
            [(1, 0), (0, 0), (4, 0), (0, 0), (0, 2)]
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
fn ppf_multitap_repair_reloads_and_restores_running_worker() {
    let (root, loader, _catalog, system_card) = fixture("tas-pce-cd-ppf-multitap-repair");
    let source_path = root.path().join("disc.cue");
    let project = loader.create_project().unwrap();
    let mut original = loader.load_editor_engine(&project).unwrap().into_backend();
    original.set_input_p5(1, 2);
    original.step_frame();
    let firmware_sha256 = zeff_firmware::sha256_bytes(system_card);
    let _firmware =
        crate::emu_backend::loader::register_test_pce_cd_system_card(firmware_sha256, system_card);
    let _stack = crate::emu_backend::loader::register_test_pce_cd_ppf_stack(
        source_path.clone(),
        crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
            &source_path,
            vec![("tas.ppf".to_owned(), super::super::ppf1(0, &[0xA5]))],
        )
        .unwrap(),
    );
    let path = root.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&path, TasAutosaveConfig::default()).unwrap();
    let cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, path, autosaves, cache).unwrap();
    let mut app = app_with_worker(
        EmuThread::spawn(original, false),
        132,
        ActiveSystem::Pce,
        source_path,
    );
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session);
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
    assert!(
        matches!(
            app.tas_control.state,
            TasControlState::AwaitingDecision { .. }
        ),
        "state={:?} repair={:?}",
        app.tas_control.state,
        app.tas_repair_state()
    );
    live_ok(&mut app, LiveCommand::TasDisconnect { keep: false });
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.tas_control.state != TasControlState::Detached && Instant::now() < deadline {
        app.drain_emu_responses();
        app.pump_tas_repair_resolution();
        std::thread::yield_now();
    }
    assert_eq!(app.tas_control.state, TasControlState::Detached);
}
