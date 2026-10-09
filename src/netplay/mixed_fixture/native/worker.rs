use super::*;
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use crate::emu_thread::{EmuCommand, EmuResponse, EmuResponsePoll, EmuThread};
use crate::netplay::mixed_fixture::{FRAMES, Frame, MAX_FRAMES, input, media, ports};
use crate::netplay::{DirectPeer, Response as NetResponse, Start, Transport, identity};
use std::path::Path;
use std::time::Instant;
use zeff_netplay::{
    endpoint::ConnectionScope, lockstep::Player, rollback::InputDelay, wire::BuildInfo,
};
use zeff_netplay_connect::{LobbyConnection, protocol::*};

pub(super) struct Options {
    timing: u8,
    delay: u64,
    host: bool,
    room: String,
}

impl Options {
    pub(super) fn parse(text: &str) -> Result<Self> {
        let values: Vec<_> = text.split(':').collect();
        ensure!(
            values.len() == 4 && text.len() <= 96,
            "invalid mixed options"
        );
        let timing = values[0].parse()?;
        let delay = values[1].parse()?;
        let host = values[2].parse::<u8>()?;
        ensure!(
            [0, 1, 3].contains(&timing) && [0, 2].contains(&delay) && host <= 1,
            "unsupported case"
        );
        ensure!(
            (host == 1 && values[3].is_empty()) || (host == 0 && valid_hex(values[3], 24)),
            "room role differs"
        );
        Ok(Self {
            timing,
            delay,
            host: host == 1,
            room: values[3].into(),
        })
    }
}

fn load(path: &Path) -> Result<EmuBackend> {
    Ok(load_backend_from_rom_source(
        ActiveSystem::Nes,
        path,
        path,
        None,
        BackendLoadConfig {
            sample_rate: Some(48_000),
            ..Default::default()
        },
    )?
    .backend)
}

fn next(worker: &EmuThread) -> Result<Option<EmuResponse>> {
    while worker.try_recv_frame().is_some() {}
    match worker.poll_response() {
        EmuResponsePoll::Response(response) => Ok(Some(*response)),
        EmuResponsePoll::Empty => Ok(None),
        EmuResponsePoll::Disconnected => anyhow::bail!("native worker disconnected"),
    }
}

pub(super) fn run(
    options: &Options,
    url: &str,
    lobby: &Lobby,
    control: &Control,
    ready: tokio::sync::oneshot::Sender<String>,
) -> Result<()> {
    let directory = crate::test_support::test_directory("mixed-native-gameplay")?;
    let path = directory.path().join("fixture.nes");
    let save = path.with_extension("sav");
    std::fs::write(&path, media(options.timing))?;
    std::fs::write(&save, vec![0x53; 8192])?;
    let backend = load(&path)?;
    let original = backend.encode_state_bytes()?;
    let delay = InputDelay::new(options.delay)?;
    let id = identity::identity_with_delay(&backend, [7; 32], delay)?;
    ensure!(
        id.build_info
            == BuildInfo {
                version: env!("CARGO_PKG_VERSION").into(),
                ..BuildInfo::default()
            },
        "fixture requires neutral native build metadata"
    );
    let mut reference = load(&path)?;
    ensure!(
        reference.encode_state_bytes()? == original,
        "reference initial state differs"
    );
    let lease = core(&mut reference).begin_rollback_session()?;
    let mut expected = Vec::new();
    for frame in 0..MAX_FRAMES as u64 {
        let ports = ports(frame, options.delay);
        let pcm = lease.advance_frame(core(&mut reference), ports.map(|p| p as u8))?;
        let checkpoint = identity::checkpoint(&reference, frame + 1, &pcm, id.config)?;
        expected.push((
            Frame::new(&checkpoint, ports, &pcm)?,
            pcm.iter().map(|s| s.to_bits()).collect::<Vec<_>>(),
        ));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let connection = runtime.block_on(async {
        let session = SessionIdentity {
            core: "nes".into(),
            content_hash: const_hex::encode(id.effective),
            compatibility_hash: const_hex::encode(id.config),
            mode: SessionMode::SharedConsole,
        };
        let auth = if options.host {
            ClientMessage::Create {
                version: VERSION,
                access_token: String::new(),
                identity: session,
            }
        } else {
            ClientMessage::Join {
                version: VERSION,
                access_token: String::new(),
                identity: session,
                room: options.room.clone(),
            }
        };
        let connection = LobbyConnection::open(url, &auth).await?;
        let _ = ready.send(connection.room().into());
        let peer = connection.establish().await?;
        ensure!(
            !peer.connection_path().await?.relay && lobby.room_count() == 0,
            "direct signaling cleanup differs"
        );
        Ok::<_, anyhow::Error>(peer)
    })?;
    let mut worker = EmuThread::spawn(backend, false);
    worker.send(EmuCommand::StartNetplay(Box::new(Start {
        stream: Transport::Direct(Box::new(DirectPeer {
            connection,
            runtime,
        })),
        player: if options.host {
            Player::One
        } else {
            Player::Two
        },
        build: [7; 32],
        secret: [9; 32],
        scope: ConnectionScope::TrustedPrivate,
        allow_different_versions: false,
        verify_every_frame: true,
        input_delay: delay,
    })));
    let result = execute(&worker, options, control, &expected, &original, &save);
    worker.shutdown();
    result?;
    ensure!(
        std::fs::read(&save)? == vec![0x53; 8192],
        "shutdown published session SRAM"
    );
    Ok(())
}

fn execute(
    worker: &EmuThread,
    options: &Options,
    control: &Control,
    expected: &[(Frame, Vec<u32>)],
    original: &[u8],
    save: &Path,
) -> Result<()> {
    let start = Instant::now();
    let mut pending = false;
    let mut stop_sent = false;
    let mut stopped = false;
    let mut capture_sent = false;
    let mut captured = false;
    let mut frames = Vec::new();
    loop {
        ensure!(
            start.elapsed() < Duration::from_secs(30),
            "native gameplay job deadline"
        );
        while let Some(response) = next(worker)? {
            let mut progress = control.progress.lock().unwrap();
            match response {
                EmuResponse::Netplay(NetResponse::Ready) => progress.ready = true,
                EmuResponse::Netplay(NetResponse::Presented {
                    frame,
                    step_complete,
                    rollback_frames,
                    ..
                }) => {
                    progress.presented = frame;
                    progress.replayed += rollback_frames;
                    if step_complete {
                        pending = false;
                        progress.pending = false;
                    }
                }
                EmuResponse::Netplay(NetResponse::Frame {
                    checkpoint,
                    ports,
                    audio,
                }) => {
                    ensure!(frames.len() < MAX_FRAMES, "native frame ledger overflow");
                    let frame = Frame::new(&checkpoint, ports, &audio)?;
                    ensure!(
                        frame == expected[frames.len()].0
                            && audio
                                .iter()
                                .map(|s| s.to_bits())
                                .eq(expected[frames.len()].1.iter().copied()),
                        "native confirmed frame differs"
                    );
                    frames.push(frame);
                    progress.verified = frames.len();
                }
                EmuResponse::Netplay(NetResponse::Stopped { restored, reason }) => {
                    ensure!(
                        restored && frames.len() >= FRAMES,
                        "unexpected native stop: {reason}"
                    );
                    stopped = true;
                    progress.stop_reason = Some(reason);
                }
                EmuResponse::StateCaptured(bytes) => {
                    ensure!(bytes == original, "native exact restoration differs");
                    captured = true;
                    progress.restored = true;
                    worker.send(EmuCommand::Shutdown);
                }
                EmuResponse::SramFlushed(Some(path)) => {
                    ensure!(
                        captured && Path::new(&path) == save,
                        "native publication fence differs"
                    );
                    progress.published = true;
                }
                EmuResponse::ShutdownComplete => {
                    ensure!(
                        captured && progress.published,
                        "native shutdown fence differs"
                    );
                    progress.frames = frames.into_iter().take(FRAMES).collect();
                    return Ok(());
                }
                EmuResponse::Netplay(NetResponse::NetworkStats(_)) => {}
                EmuResponse::Netplay(NetResponse::Paused { local, peer, .. }) => {
                    ensure!(!local && !peer, "unexpected native pause");
                }
                EmuResponse::Netplay(NetResponse::Rejected(reason)) => {
                    anyhow::bail!("native gameplay rejected: {reason}");
                }
                _ => anyhow::bail!("unexpected native gameplay response"),
            }
        }
        let progress = control.progress.lock().unwrap().clone();
        if stopped && !capture_sent {
            worker.send(EmuCommand::CaptureStateBytes);
            capture_sent = true;
        }
        if control.stop.load(Ordering::Acquire) && !pending && !stop_sent && !stopped {
            ensure!(progress.verified >= FRAMES, "early native stop request");
            control.progress.lock().unwrap().closed_at = progress.presented;
            worker.send(EmuCommand::StopNetplay);
            stop_sent = true;
        }
        if progress.ready
            && !pending
            && !stop_sent
            && !stopped
            && progress.verified < FRAMES
            && (progress.presented < 6 || control.release.load(Ordering::Acquire))
        {
            ensure!(
                progress.presented < MAX_FRAMES as u64,
                "native prediction ledger overflow"
            );
            pending = true;
            control.progress.lock().unwrap().pending = true;
            worker.send(EmuCommand::StepNetplay(input(
                progress.presented,
                usize::from(!options.host),
            )));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn core(backend: &mut EmuBackend) -> &mut zeff_nes_core::emulator::Emulator {
    let EmuBackend::Nes(nes) = backend else {
        unreachable!()
    };
    &mut nes.emu
}
