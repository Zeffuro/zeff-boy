use super::*;
use crate::emu_backend::EmuBackend;
use crate::netplay::identity;
use crossbeam_channel::{Receiver, Sender};
use zeff_netplay::wire::{self, Connection, Message};

const WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const RESPONSE_DEADLINE: Duration = Duration::from_secs(2);

struct Fixture {
    directory: crate::test_support::TestDirectory,
    worker: EmuLoop,
    reference: EmuBackend,
    cmd_tx: Sender<EmuCommand>,
    resp_rx: Receiver<EmuResponse>,
    initial: Vec<u8>,
    initial_save: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        let directory = crate::test_support::test_directory("netplay-selected-wake").unwrap();
        let path = directory.path().join("fixture.nes");
        std::fs::write(&path, zeff_netplay::fixture::rom()).unwrap();
        let load = |nes_load_battery_sram| {
            load_backend_from_rom_source(
                ActiveSystem::Nes,
                &path,
                &path,
                None,
                BackendLoadConfig {
                    nes_load_battery_sram,
                    ..BackendLoadConfig::default()
                },
            )
            .unwrap()
            .backend
        };
        let seed = load(false);
        let initial_save = vec![
            0xa5;
            seed.nes()
                .unwrap()
                .emu
                .dump_persistent_data()
                .unwrap()
                .len()
        ];
        drop(seed);
        std::fs::write(path.with_extension("sav"), &initial_save).unwrap();
        let backend = load(true);
        let reference = load(true);
        assert_eq!(
            backend.nes().unwrap().emu.dump_persistent_data().unwrap(),
            initial_save
        );
        let initial = backend.encode_state_bytes().unwrap();
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        let (frame_tx, frame_rx) = crossbeam_channel::bounded(2);
        let (resp_tx, resp_rx) = crossbeam_channel::unbounded();
        let worker = EmuLoop::new(
            backend,
            cmd_rx,
            frame_tx,
            frame_rx,
            resp_tx,
            EmuLoopConfig {
                shared_framebuffer: crate::emu_thread::types::new_shared_framebuffer(),
                save_recovery_on_shutdown: false,
                recovery: None,
            },
        );
        crate::emu_thread::types::publish_backend_framebuffer(
            &worker.shared_framebuffer,
            &worker.backend,
        );
        Self {
            directory,
            worker,
            reference,
            cmd_tx,
            resp_rx,
            initial,
            initial_save,
        }
    }

    fn wait_event(&mut self) {
        let started = Instant::now();
        assert!(
            self.worker
                .wait_netplay_command(WAIT_TIMEOUT)
                .unwrap()
                .is_none()
        );
        assert!(
            started.elapsed() < RESPONSE_DEADLINE,
            "network event waited for timeout"
        );
    }

    fn wait_queued(&self) {
        let deadline = Instant::now() + RESPONSE_DEADLINE;
        while self.worker.netplay.as_ref().unwrap().events().is_empty() {
            assert!(Instant::now() < deadline, "network event was never queued");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn admit(&mut self, prequeued: bool) -> Connection {
        let admission = identity::identity(&self.worker.backend, [9; 32]).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let local = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (peer, _) = listener.accept().unwrap();
        assert!(
            self.worker
                .handle_command(EmuCommand::StartNetplay(Box::new(crate::netplay::Start {
                    allow_different_versions: false,
                    verify_every_frame: true,
                    input_delay: zeff_netplay::rollback::InputDelay::default(),
                    scope: zeff_netplay::endpoint::ConnectionScope::Loopback,
                    stream: local.into(),
                    player: Player::One,
                    build: [9; 32],
                    secret: [5; 32],
                },)))
        );
        let peer = std::thread::spawn(move || {
            if !prequeued {
                std::thread::sleep(Duration::from_millis(30));
            }
            wire::admit(peer, Player::Two, &admission, &[5; 32]).unwrap()
        });
        let peer = if prequeued {
            let peer = peer.join().unwrap();
            self.wait_queued();
            self.wait_event();
            peer
        } else {
            assert!(self.worker.netplay.as_ref().unwrap().events().is_empty());
            self.wait_event();
            peer.join().unwrap()
        };
        assert!(matches!(
            self.resp_rx.try_recv().unwrap(),
            EmuResponse::Netplay(Response::Ready)
        ));
        assert!(
            !self
                .worker
                .backend
                .nes()
                .unwrap()
                .host_persistence_enabled()
        );
        self.assert_save_preserved();
        peer
    }

    fn send_event(&mut self, peer: &mut Connection, message: Message, prequeued: bool) {
        if prequeued {
            peer.send(&message).unwrap();
            self.wait_queued();
            self.wait_event();
        } else {
            assert!(self.worker.netplay.as_ref().unwrap().events().is_empty());
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    std::thread::sleep(Duration::from_millis(30));
                    peer.send(&message).unwrap();
                });
                self.wait_event();
            });
        }
        self.assert_save_preserved();
    }

    fn assert_save_preserved(&self) {
        assert_eq!(
            std::fs::read(self.directory.path().join("fixture.sav")).unwrap(),
            self.initial_save,
        );
    }

    fn assert_restored(&self) {
        assert!(self.worker.netplay.is_none());
        assert_eq!(
            self.worker.backend.encode_state_bytes().unwrap(),
            self.initial
        );
        assert!(
            self.worker
                .backend
                .nes()
                .unwrap()
                .host_persistence_enabled()
        );
        assert_eq!(
            self.worker
                .shared_framebuffer
                .load_full()
                .unwrap()
                .as_slice(),
            self.worker.backend.framebuffer(),
        );
        assert_eq!(
            self.worker
                .backend
                .nes()
                .unwrap()
                .emu
                .dump_persistent_data()
                .unwrap(),
            self.initial_save
        );
        self.assert_save_preserved();
    }
}

#[test]
fn selected_network_events_wake_prequeued_admission_and_late_input_correction() {
    selected_correction(true);
}

#[test]
fn selected_network_events_wake_admission_and_late_input_correction_during_wait() {
    selected_correction(false);
}

fn selected_correction(prequeued: bool) {
    let mut fixture = Fixture::new();
    let config = identity::identity(&fixture.reference, [9; 32])
        .unwrap()
        .config;
    let mut peer = fixture.admit(prequeued);
    let inputs = [[1, 0x80], [2, 0x40], [4, 0x20], [8, 0x10]];
    let mut frames = Vec::new();
    for (frame, buttons) in inputs.into_iter().enumerate() {
        assert!(
            fixture
                .worker
                .handle_command(EmuCommand::StepNetplay(buttons[0]))
        );
        fixture.worker.poll_netplay();
        let responses: Vec<_> = fixture.resp_rx.try_iter().collect();
        assert!(responses.iter().any(|response| matches!(response, EmuResponse::Netplay(Response::Presented {frame:actual,step_complete:true,..}) if *actual==frame as u64+1)));
        frames.extend(responses.into_iter().filter_map(|response| match response {
            EmuResponse::Netplay(Response::Frame {
                checkpoint,
                ports,
                audio,
            }) => Some((checkpoint, ports, audio)),
            _ => None,
        }));
        assert_eq!(fixture.worker.backend.frame_count(), frame as u64 + 1);
        let deadline = Instant::now() + RESPONSE_DEADLINE;
        loop {
            assert!(Instant::now() < deadline, "input message deadline");
            match peer.receive().unwrap() {
                Message::Progress {
                    frame: progress,
                    confirmed,
                } => {
                    assert!(progress <= frame as u64 && confirmed <= progress);
                }
                message => {
                    assert_eq!(
                        message,
                        Message::Input {
                            player: Player::One,
                            frame: frame as u64 + 2,
                            buttons: buttons[0]
                        }
                    );
                    break;
                }
            }
        }
        assert!(matches!(peer.receive().unwrap(), Message::Progress { .. }));
    }
    assert_eq!(frames.len(), 2, "speculative PCM escaped confirmation");
    for (frame, buttons) in inputs.into_iter().enumerate() {
        fixture.send_event(
            &mut peer,
            Message::Input {
                player: Player::Two,
                frame: frame as u64 + 2,
                buttons: buttons[1],
            },
            prequeued,
        );
        fixture.worker.poll_netplay();
        frames.extend(
            fixture
                .resp_rx
                .try_iter()
                .filter_map(|response| match response {
                    EmuResponse::Netplay(Response::Frame {
                        checkpoint,
                        ports,
                        audio,
                    }) => Some((checkpoint, ports, audio)),
                    _ => None,
                }),
        );
    }
    assert_eq!(frames.len(), 4);
    for (frame, (checkpoint, actual, audio)) in frames.into_iter().enumerate() {
        let ports = if frame < 2 { [0, 0] } else { inputs[frame - 2] };
        let EmuBackend::Nes(nes) = &mut fixture.reference else {
            unreachable!()
        };
        nes.emu.set_input_p1_raw(ports[0]);
        nes.emu.set_input_p2_raw(ports[1]);
        fixture.reference.step_frame();
        let mut expected_audio = Vec::new();
        fixture
            .reference
            .drain_audio_samples_into(&mut expected_audio);
        let expected = identity::checkpoint(
            &fixture.reference,
            frame as u64 + 1,
            &expected_audio,
            config,
        )
        .unwrap();
        assert_eq!(checkpoint, expected);
        assert_eq!(actual, ports);
        assert!(
            audio
                .iter()
                .map(|v| v.to_bits())
                .eq(expected_audio.iter().map(|v| v.to_bits()))
        );
    }
    assert_eq!(
        fixture
            .worker
            .shared_framebuffer
            .load_full()
            .unwrap()
            .as_slice(),
        fixture.reference.framebuffer()
    );
    assert!(
        !fixture
            .worker
            .backend
            .nes()
            .unwrap()
            .host_persistence_enabled()
    );
    assert!(
        fixture
            .worker
            .backend
            .flush_battery_sram()
            .unwrap()
            .is_none()
    );
    fixture.assert_save_preserved();
    assert!(fixture.worker.handle_command(EmuCommand::StopNetplay));
    assert!(fixture.resp_rx.try_iter().any(|response| matches!(
        response,
        EmuResponse::Netplay(Response::Stopped { restored: true, .. })
    )));
    fixture.assert_restored();
}

#[test]
fn selected_commands_stop_and_shutdown_without_peer_input() {
    for shutdown in [false, true] {
        let mut fixture = Fixture::new();
        let mut peer = fixture.admit(true);
        assert!(fixture.worker.handle_command(EmuCommand::StepNetplay(0xff)));
        fixture.worker.poll_netplay();
        fixture.resp_rx.try_iter().for_each(drop);
        assert!(matches!(
            peer.receive().unwrap(),
            Message::Input { frame: 2, .. }
        ));
        assert!(matches!(peer.receive().unwrap(), Message::Progress { .. }));
        let cmd_tx = fixture.cmd_tx.clone();
        let started = Instant::now();
        let command = std::thread::scope(|scope| {
            scope.spawn(move || {
                std::thread::sleep(Duration::from_millis(30));
                cmd_tx
                    .send(if shutdown {
                        EmuCommand::Shutdown
                    } else {
                        EmuCommand::StopNetplay
                    })
                    .unwrap();
            });
            fixture
                .worker
                .wait_netplay_command(WAIT_TIMEOUT)
                .unwrap()
                .unwrap()
        });
        assert!(started.elapsed() < RESPONSE_DEADLINE);
        assert_eq!(fixture.worker.handle_command(command), !shutdown);
        let responses: Vec<_> = fixture.resp_rx.try_iter().collect();
        assert!(responses.iter().any(|response| matches!(
            response,
            EmuResponse::Netplay(Response::Stopped { restored: true, .. })
        )));
        if shutdown {
            assert!(
                responses
                    .iter()
                    .any(|response| matches!(response, EmuResponse::ShutdownComplete))
            );
        }
        fixture.assert_restored();
    }
}
