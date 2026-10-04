use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;

use super::*;
use crate::emu_backend::{ActiveSystem, EmuBackend};
use crate::emu_thread::EmuCommand;
use crate::emu_thread::emu_loop::EmuLoopConfig;

fn worker(system: ActiveSystem) -> (EmuLoop, Receiver<EmuResponse>) {
    let backend = match system {
        ActiveSystem::GameBoy => EmuBackend::from_gb(
            zeff_gb_core::emulator::Emulator::from_rom_data(
                &crate::test_support::build_gb_test_rom(),
                zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::Auto,
            )
            .unwrap(),
            PathBuf::from("pending-link.gb"),
        ),
        ActiveSystem::WonderSwan => {
            let mut rom = vec![0xff; 0x10000];
            rom[0] = 0xf4;
            let reset = rom.len() - 16;
            rom[reset..reset + 5].copy_from_slice(&[0xea, 0, 0, 0, 0xf0]);
            let footer = rom.len() - 10;
            rom[footer + 4] = 1;
            let checksum = zeff_ws_core::hardware::cartridge::compute_footer_checksum(&rom);
            rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
            EmuBackend::from_ws(
                zeff_ws_core::emulator::Emulator::from_rom_data(&rom).unwrap(),
                PathBuf::from("pending-link.ws"),
            )
        }
        _ => unreachable!(),
    };
    let (_commands, command_rx) = crossbeam_channel::unbounded();
    let (frame_tx, frame_rx) = crossbeam_channel::bounded(2);
    let (response_tx, response_rx) = crossbeam_channel::unbounded();
    (
        EmuLoop::new(
            backend,
            command_rx,
            frame_tx,
            frame_rx,
            response_tx,
            EmuLoopConfig {
                shared_framebuffer: crate::emu_thread::types::new_shared_framebuffer(),
                save_recovery_on_shutdown: false,
                recovery: None,
            },
        ),
        response_rx,
    )
}

fn unused_address() -> SocketAddr {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

fn host(worker: &mut EmuLoop, responses: &Receiver<EmuResponse>, address: SocketAddr) {
    assert!(
        worker.handle_command(EmuCommand::StartTcpLink(TcpLinkMode::Host {
            bind_addr: address.to_string(),
        }))
    );
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::LinkPending(_)
    ));
    assert!(TcpListener::bind(address).is_err());
}

#[test]
fn pending_host_cancel_rebind_and_replacement_preserve_both_console_states() {
    for system in [ActiveSystem::GameBoy, ActiveSystem::WonderSwan] {
        let (mut worker, responses) = worker(system);
        let initial = worker.backend.encode_state_bytes().unwrap();
        let address = unused_address();
        host(&mut worker, &responses, address);
        host(&mut worker, &responses, address);
        let began = Instant::now();
        assert!(worker.handle_command(EmuCommand::DisconnectLink));
        assert!(began.elapsed() < Duration::from_secs(2));
        assert!(matches!(
            responses.recv().unwrap(),
            EmuResponse::LinkDisconnected { .. }
        ));
        let rebound = TcpListener::bind(address).unwrap();
        worker.poll_tcp_link_connection();
        assert!(responses.try_recv().is_err());
        assert!(worker.pending_tcp_link.is_none() && worker.tcp_link.is_none());
        assert_eq!(worker.backend.encode_state_bytes().unwrap(), initial);
        drop(rebound);
        host(&mut worker, &responses, address);
        drop(worker);
        assert!(TcpListener::bind(address).is_ok());
    }
}

#[test]
fn pending_join_cancel_and_worker_shutdown_release_owned_connection() {
    for system in [ActiveSystem::GameBoy, ActiveSystem::WonderSwan] {
        let (mut worker, responses) = worker(system);
        let initial = worker.backend.encode_state_bytes().unwrap();
        assert!(
            worker.handle_command(EmuCommand::StartTcpLink(TcpLinkMode::Join {
                connect_addr: unused_address().to_string(),
            }))
        );
        assert!(matches!(
            responses.recv().unwrap(),
            EmuResponse::LinkPending(_)
        ));
        let began = Instant::now();
        assert!(worker.handle_command(EmuCommand::DisconnectLink));
        assert!(began.elapsed() < Duration::from_secs(2));
        assert!(matches!(
            responses.recv().unwrap(),
            EmuResponse::LinkDisconnected { .. }
        ));
        assert_eq!(worker.backend.encode_state_bytes().unwrap(), initial);
        let address = unused_address();
        host(&mut worker, &responses, address);
        assert!(!worker.handle_command(EmuCommand::Shutdown));
        assert!(worker.pending_tcp_link.is_none() && worker.tcp_link.is_none());
        assert!(TcpListener::bind(address).is_ok());
        assert!(
            responses
                .try_iter()
                .any(|response| matches!(response, EmuResponse::ShutdownComplete))
        );
    }
}

#[test]
fn both_console_workers_connect_and_disconnect_without_late_completion() {
    for system in [ActiveSystem::GameBoy, ActiveSystem::WonderSwan] {
        let (mut host_worker, host_responses) = worker(system);
        let (mut join_worker, join_responses) = worker(system);
        let address = unused_address();
        host(&mut host_worker, &host_responses, address);
        assert!(
            join_worker.handle_command(EmuCommand::StartTcpLink(TcpLinkMode::Join {
                connect_addr: address.to_string(),
            }))
        );
        assert!(matches!(
            join_responses.recv().unwrap(),
            EmuResponse::LinkPending(_)
        ));
        let deadline = Instant::now() + Duration::from_secs(3);
        while host_worker.tcp_link.is_none() || join_worker.tcp_link.is_none() {
            host_worker.poll_tcp_link_connection();
            join_worker.poll_tcp_link_connection();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        for responses in [&host_responses, &join_responses] {
            assert!(matches!(
                responses.recv().unwrap(),
                EmuResponse::LinkConnected { .. }
            ));
        }
        host_worker.disconnect_tcp_link();
        join_worker.disconnect_tcp_link();
        assert!(host_worker.pending_tcp_link.is_none() && join_worker.pending_tcp_link.is_none());
        host_worker.poll_tcp_link_connection();
        join_worker.poll_tcp_link_connection();
        assert!(host_responses.try_recv().is_err() && join_responses.try_recv().is_err());
    }
}

#[test]
fn invalid_host_address_fails_without_starting_or_mutating_either_console() {
    for system in [ActiveSystem::GameBoy, ActiveSystem::WonderSwan] {
        let (mut worker, responses) = worker(system);
        let initial = worker.backend.encode_state_bytes().unwrap();
        assert!(
            worker.handle_command(EmuCommand::StartTcpLink(TcpLinkMode::Host {
                bind_addr: "remote.invalid:8765".into(),
            }))
        );
        assert!(matches!(
            responses.recv().unwrap(),
            EmuResponse::LinkFailed(_)
        ));
        assert!(worker.pending_tcp_link.is_none() && worker.tcp_link.is_none());
        assert_eq!(worker.backend.encode_state_bytes().unwrap(), initial);
    }
}
