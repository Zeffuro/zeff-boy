use super::*;
use std::net::Shutdown;
use zeff_netplay::wire;

#[derive(Clone, Copy, Debug)]
pub(super) enum Fault {
    Jitter,
    Duplicate,
    Stale,
    Conflict,
    Future,
    Malformed,
    Disconnect,
    Timeout,
    Desync,
    NativeDivergence,
    Flood,
    PauseMismatch,
}

fn owned_input(connection: &mut wire::Connection) -> Result<(u64, u8)> {
    loop {
        match connection.receive()? {
            Message::Input {
                player: Player::One,
                frame,
                buttons,
            } => return Ok((frame, buttons)),
            Message::Progress { .. } | Message::Checkpoint { .. } => {}
            message => bail!("unexpected peer input packet {message:?}"),
        }
    }
}

fn await_refusal(connection: &mut wire::Connection) {
    while connection.receive().is_ok() {}
}

fn peer(
    stream: TcpStream,
    mut backend: EmuBackend,
    fault: Fault,
    completed: std::sync::mpsc::Sender<()>,
) -> Result<()> {
    let admission = identity::identity(&backend, [9; 32])?;
    let mut connection = wire::admit(stream.try_clone()?, Player::Two, &admission, &[5; 32])?;
    let mut prior = Vec::new();
    let frames = if matches!(fault, Fault::Desync | Fault::NativeDivergence) {
        64
    } else {
        24
    };
    for frame in 0..frames {
        let (scheduled, buttons) = owned_input(&mut connection)?;
        ensure!(scheduled == frame + 2, "unexpected scheduled input");
        prior.push(buttons);
        if matches!(fault, Fault::Jitter) {
            std::thread::sleep(Duration::from_millis([0, 1, 4, 2, 8][frame as usize % 5]));
        }
        if frame == 5 {
            match fault {
                Fault::Future => {
                    connection.send(&Message::Input {
                        player: Player::Two,
                        frame: u64::MAX,
                        buttons: 0,
                    })?;
                    await_refusal(&mut connection);
                    return Ok(());
                }
                Fault::Malformed => {
                    connection.send_invalid_length_for_test()?;
                    await_refusal(&mut connection);
                    return Ok(());
                }
                Fault::Disconnect => {
                    stream.shutdown(Shutdown::Both)?;
                    return Ok(());
                }
                Fault::Timeout => {
                    std::thread::sleep(Duration::from_secs(3));
                    return Ok(());
                }
                Fault::PauseMismatch => {
                    connection.send(&Message::PauseChange {
                        request: 1,
                        frame,
                        paused: false,
                    })?;
                    await_refusal(&mut connection);
                    return Ok(());
                }
                Fault::Flood => {
                    for _ in 0..4096 {
                        if connection
                            .send(&Message::Progress {
                                frame: 0,
                                confirmed: 0,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    return Ok(());
                }
                Fault::NativeDivergence => {
                    let EmuBackend::Nes(nes) = &mut backend else {
                        unreachable!()
                    };
                    nes.emu.cpu_write8(0x07f0, 0x80);
                }
                _ => {}
            }
        }
        let input = Message::Input {
            player: Player::Two,
            frame: scheduled,
            buttons: sample(frame, Player::Two),
        };
        connection.send(&input)?;
        if frame == 5 {
            match fault {
                Fault::Duplicate => connection.send(&input)?,
                Fault::Stale => connection.send(&Message::Input {
                    player: Player::Two,
                    frame: 0,
                    buttons: 0,
                })?,
                Fault::Conflict => {
                    connection.send(&Message::Input {
                        player: Player::Two,
                        frame: scheduled,
                        buttons: sample(frame, Player::Two) ^ 1,
                    })?;
                    await_refusal(&mut connection);
                    return Ok(());
                }
                _ => {}
            }
        }
        let ports = if frame < 2 {
            [0, 0]
        } else {
            [prior[(frame - 2) as usize], sample(frame - 2, Player::Two)]
        };
        let (mut expected, _) = reference_frame(&mut backend, ports, admission.config)?;
        if (frame + 1) % 60 == 0 {
            if matches!(fault, Fault::Desync) {
                let Message::Checkpoint { logical, .. } = &mut expected else {
                    unreachable!()
                };
                logical[0] ^= 1;
            }
            connection.send(&expected)?;
            if matches!(fault, Fault::Desync | Fault::NativeDivergence) {
                await_refusal(&mut connection);
                return Ok(());
            }
        }
    }
    completed.send(())?;
    let _ = connection.receive();
    Ok(())
}

#[test]
fn native_worker_fault_matrix_restores_exact_state_and_protects_saves() {
    for fault in [
        Fault::Jitter,
        Fault::Duplicate,
        Fault::Stale,
        Fault::Conflict,
        Fault::Future,
        Fault::Malformed,
        Fault::Disconnect,
        Fault::Timeout,
        Fault::Desync,
        Fault::Flood,
        Fault::PauseMismatch,
    ] {
        run_fault(fault, &media::Media::fixture(), false);
    }
}

pub(super) fn run_fault(fault: Fault, media: &media::Media, check_restored_publication: bool) {
    let directory = crate::test_support::test_directory("netplay-worker-fault").unwrap();
    let (path, backend) = media.load(directory.path(), "worker").unwrap();
    let (_, reference) = media.load(directory.path(), "peer").unwrap();
    let initial = backend.encode_state_bytes().unwrap();
    let pixels = backend.framebuffer().to_vec();
    let persistent = identity::identity(&backend, [9; 32]).unwrap().persistent;
    let (local, remote) = sockets().unwrap();
    let (completed, completion) = std::sync::mpsc::channel();
    let peer = std::thread::spawn(move || peer(remote, reference, fault, completed));
    let mut worker = EmuThread::spawn(backend, false);
    worker.send(EmuCommand::StartNetplay(Box::new(Start {
        allow_different_versions: false,
        verify_every_frame: true,
        input_delay: zeff_netplay::rollback::InputDelay::default(),
        scope: zeff_netplay::endpoint::ConnectionScope::Loopback,
        stream: local.into(),
        player: Player::One,
        build: [9; 32],
        secret: [5; 32],
    })));
    assert!(
        matches!(next(&worker).unwrap(), Response::Ready),
        "{fault:?}"
    );
    let success = matches!(fault, Fault::Jitter | Fault::Duplicate | Fault::Stale);
    let mut confirmed = 0;
    let mut pending_step = false;
    let mut issued = 1;
    worker.send(EmuCommand::StepNetplay(sample(0, Player::One)));
    let reason = loop {
        match next_any(&worker).unwrap() {
            Response::Frame {
                checkpoint: Message::Checkpoint { frame, .. },
                ..
            } => {
                assert_eq!(frame, confirmed + 1);
                confirmed = frame;
                assert!(!path.with_extension("sav").exists());
            }
            Response::Presented {
                step_complete: true,
                ..
            } => pending_step = true,
            Response::Presented { .. } => {}
            Response::Stopped {
                reason,
                restored: true,
            } => break reason,
            Response::Stopped { reason, .. } => panic!("failed restoration {fault:?}: {reason}"),
            Response::Rejected(reason) => panic!("unexpected rejection {fault:?}: {reason}"),
            _ => panic!("unexpected fault response {fault:?}"),
        }
        if pending_step && confirmed == issued {
            if success && confirmed == 24 {
                completion
                    .recv_timeout(Duration::from_secs(2))
                    .expect("peer must finish its supplied input writes before shutdown");
                worker.send(EmuCommand::StopNetplay);
                break stopped(&worker).unwrap();
            }
            // Verification pacing waits only in the harness; production presentation is independent.
            if confirmed <= 64 {
                worker.send(EmuCommand::StepNetplay(sample(confirmed, Player::One)));
                pending_step = false;
                issued += 1;
            }
        }
    };
    if success {
        assert_eq!(confirmed, 24);
    } else {
        let confirmed_limit = if matches!(fault, Fault::Desync | Fault::NativeDivergence) {
            60 + zeff_netplay::lockstep::INPUT_DELAY
        } else {
            6 + zeff_netplay::lockstep::INPUT_DELAY
        };
        assert!(
            confirmed <= confirmed_limit,
            "fault confirmed beyond supplied inputs: {fault:?}: {confirmed}"
        );
        match fault {
            Fault::Conflict => assert!(reason.contains("rewritten"), "{fault:?}: {reason}"),
            Fault::Future => assert!(reason.contains("window"), "{fault:?}: {reason}"),
            Fault::Desync | Fault::NativeDivergence => {
                assert!(reason.contains("checkpoint"), "{fault:?}: {reason}")
            }
            Fault::PauseMismatch => assert!(reason.contains("pause"), "{fault:?}: {reason}"),
            Fault::Malformed => assert!(reason.contains("length"), "{fault:?}: {reason}"),
            _ => assert!(!reason.is_empty()),
        }
    }
    assert_eq!(capture(&worker).unwrap(), initial, "{fault:?}");
    if check_restored_publication {
        assert_eq!(
            worker.shared_framebuffer().load_full().unwrap().as_slice(),
            pixels
        );
    }
    worker.shutdown();
    assert_eq!(
        <[u8; 32]>::from(Sha256::digest(
            std::fs::read(path.with_extension("sav")).unwrap()
        )),
        persistent
    );
    let peer_result = peer.join().unwrap();
    if success {
        peer_result.unwrap_or_else(|error| panic!("{fault:?}: {error:#}"));
    }
}
