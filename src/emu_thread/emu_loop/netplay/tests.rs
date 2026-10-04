use super::*;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_thread::emu_loop::EmuLoopConfig;
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};
use zeff_netplay::lockstep::Player;

mod wake_tests;

#[test]
fn active_and_failed_restore_leases_block_all_persistence_paths() {
    let directory = crate::test_support::test_directory("netplay-worker-poison").unwrap();
    let path = directory.path().join("game.nes");
    std::fs::write(&path, zeff_netplay::fixture::rom()).unwrap();
    let backend = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &path,
        &path,
        None,
        BackendLoadConfig {
            nes_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend;
    let generation = directory.path().join("generation.json");
    let recovery = directory.path().join("recovery.state");
    let (_cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
    let (frame_tx, frame_rx) = crossbeam_channel::bounded(2);
    let (resp_tx, resp_rx) = crossbeam_channel::unbounded();
    let mut worker = EmuLoop::new(
        backend,
        cmd_rx,
        frame_tx,
        frame_rx,
        resp_tx,
        EmuLoopConfig {
            shared_framebuffer: crate::emu_thread::types::new_shared_framebuffer(),
            save_recovery_on_shutdown: true,
            recovery: Some(crate::emu_thread::RecoveryTestConfig {
                generation_path: generation.clone(),
                state_path: recovery.clone(),
                fail_generation_write: false,
            }),
        },
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let local = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (_silent_peer, _) = listener.accept().unwrap();
    assert!(
        worker.handle_command(EmuCommand::StartNetplay(Box::new(crate::netplay::Start {
            allow_different_versions: false,
            verify_every_frame: true,
            input_delay: zeff_netplay::rollback::InputDelay::default(),
            scope: zeff_netplay::endpoint::ConnectionScope::Loopback,
            stream: local,
            player: Player::One,
            build: [9; 32],
            secret: [5; 32],
        })))
    );
    assert!(worker.netplay.is_some());
    assert!(!worker.backend.nes().unwrap().host_persistence_enabled());
    for _ in 0..8 {
        worker.backend.step_frame();
    }
    worker.battery_flush.mark_potentially_dirty();
    worker.flush_battery_sram_if_due(Instant::now() + Duration::from_secs(31));
    assert!(!path.with_extension("sav").exists());
    worker
        .netplay
        .as_mut()
        .unwrap()
        .corrupt_restore_checkpoint_for_test();
    assert!(worker.handle_command(EmuCommand::StopNetplay));
    assert!(matches!(
        resp_rx.recv().unwrap(),
        EmuResponse::Netplay(Response::Stopped {
            restored: false,
            ..
        })
    ));
    assert!(worker.netplay_restore_failed);
    assert!(!worker.backend.nes().unwrap().host_persistence_enabled());
    let forbidden = directory.path().join("forbidden.state");
    for command in [
        EmuCommand::Reset,
        EmuCommand::SaveStateToPath(forbidden.clone()),
        EmuCommand::CaptureStateBytes,
        EmuCommand::StepNetplay(1),
        EmuCommand::SetNetplayPaused(true),
    ] {
        assert!(worker.handle_command(command));
        assert!(
            matches!(resp_rx.recv().unwrap(), EmuResponse::Netplay(Response::Rejected(reason))
            if reason.contains("requires replacement"))
        );
    }
    worker.flush_battery_sram_if_due(Instant::now() + Duration::from_secs(32));
    assert!(!worker.handle_command(EmuCommand::Shutdown));
    assert!(matches!(
        resp_rx.recv().unwrap(),
        EmuResponse::ShutdownComplete
    ));
    for output in [path.with_extension("sav"), forbidden, generation, recovery] {
        assert!(
            !output.exists(),
            "unexpected publication: {}",
            output.display()
        );
    }
}
