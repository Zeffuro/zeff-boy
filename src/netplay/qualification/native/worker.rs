use super::*;
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use crate::emu_thread::{EmuCommand, EmuResponse, EmuResponsePoll, EmuThread};
use crate::netplay::qualification::{Frame, MAX_FRAMES, input, media, ports};
use crate::netplay::{
    Response as NetResponse,
    connect::{Connector, executable_build, lobby as lobby_options},
    identity,
};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Instant;
use zeff_netplay::rollback::InputDelay;
use zeff_netplay_connect::protocol::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Options {
    timing: u8,
    delay: u64,
    host: bool,
    room: String,
    frames: usize,
    #[serde(default)]
    rom: Option<Vec<u8>>,
}

impl Options {
    pub(super) fn parse(text: &str) -> Result<Self> {
        let value: Self = serde_json::from_str(text)?;
        ensure!(
            [0, 1, 3].contains(&value.timing)
                && [0, 2].contains(&value.delay)
                && (72..=1000).contains(&value.frames),
            "unsupported qualification case"
        );
        ensure!(
            (value.host && value.room.is_empty()) || (!value.host && !value.room.is_empty()),
            "invitation role differs"
        );
        ensure!(value.room.len() <= 1200, "invitation exceeds limit");
        if let Some(rom) = &value.rom {
            ensure!(
                (16..=1024 * 1024).contains(&rom.len()) && &rom[..4] == b"NES\x1a",
                "invalid qualification ROM"
            );
        }
        Ok(value)
    }
}

struct Publication {
    save: PathBuf,
    media: [u8; 32],
    component: [u8; 32],
    battery: bool,
}

impl Publication {
    fn check_save(&self) -> Result<()> {
        if self.battery {
            ensure!(
                std::fs::read(&self.save)? == vec![0x53; 8192],
                "shutdown published session SRAM"
            );
        } else {
            ensure!(
                !self.save.exists(),
                "no-battery qualification published SRAM"
            );
        }
        Ok(())
    }

    fn check_acknowledgment(&self, path: Option<&str>) -> Result<()> {
        ensure!(
            if self.battery {
                path.is_some_and(|path| Path::new(path) == self.save.as_path())
            } else {
                path.is_none()
            },
            "native publication path differs"
        );
        self.check_save()?;
        let generation =
            crate::save_paths::recovery_state::battery_generation_path("nes", self.media)?;
        let record = crate::save_paths::recovery_state::decode_battery_generation(
            &std::fs::read(generation)?,
            self.media,
        )
        .context("native generation commit witness absent")?;
        ensure!(
            record.component_sha256 == self.component,
            "native generation components differ"
        );
        Ok(())
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
    root: &Path,
    control: &Control,
    ready: tokio::sync::oneshot::Sender<String>,
) -> Result<()> {
    let directory = tempfile::tempdir_in(root)?;
    let path = directory.path().join("fixture.nes");
    let save = path.with_extension("sav");
    std::fs::write(
        &path,
        options.rom.clone().unwrap_or_else(|| media(options.timing)),
    )?;
    let initial = load(&path)?;
    let battery = initial.capabilities().has_battery;
    let persistent = initial
        .nes()
        .context("NES required")?
        .emu
        .dump_persistent_data();
    if battery {
        ensure!(
            persistent.is_some_and(|bytes| bytes.len() == 8192),
            "qualification requires 8 KiB battery SRAM"
        );
    } else {
        ensure!(
            options.rom.is_some() && persistent.is_none(),
            "generated fixtures require battery SRAM"
        );
        ensure!(!save.exists(), "no-battery qualification has an SRAM file");
    }
    drop(initial);
    if battery {
        std::fs::write(&save, vec![0x53; 8192])?;
    }
    let backend = load(&path)?;
    ensure!(
        backend
            .nes()
            .context("NES required")?
            .emu
            .dump_persistent_data()
            == battery.then(|| vec![0x53; 8192]),
        "loaded native persistent seed differs"
    );
    let publication = Publication {
        save,
        media: backend.rom_hash(),
        component: backend.battery_generation_receipt()?.component_sha256,
        battery,
    };
    let original = backend.encode_state_bytes()?;
    let delay = InputDelay::new(options.delay)?;
    let build = executable_build()?;
    let id = identity::identity_with_delay(&backend, build, delay)?;
    ensure!(
        id.build_info.certificate != [0; 32],
        "production qualification needs a signed artifact pair"
    );
    let mut facts = super::super::facts(build);
    facts["game_source"] = serde_json::json!(const_hex::encode(id.source));
    facts["certificate"] =
        serde_json::to_value(crate::netplay::compatibility::certificate_status()?)?;
    control.progress.lock().unwrap().facts = facts;
    let mut reference = load(&path)?;
    ensure!(
        reference.encode_state_bytes()? == original,
        "reference initial state differs"
    );
    let lease = core(&mut reference).begin_rollback_session()?;
    let mut expected = Vec::new();
    for frame in 0..(options.frames + 24) as u64 {
        let ports = ports(frame, options.delay);
        let pcm = lease.advance_frame(core(&mut reference), ports.map(|p| p as u8))?;
        let checkpoint = identity::checkpoint(&reference, frame + 1, &pcm, id.config)?;
        expected.push((
            Frame::new(&checkpoint, ports, &pcm)?,
            pcm.iter().map(|s| s.to_bits()).collect::<Vec<_>>(),
        ));
    }
    let session = SessionIdentity {
        core: "nes".into(),
        content_hash: const_hex::encode(id.effective),
        compatibility_hash: const_hex::encode(id.config),
        mode: SessionMode::SharedConsole,
    };
    let connection_options = lobby_options::Options {
        url: url.into(),
        access_token: String::new(),
        input_delay: delay,
    };
    let mut connector = if options.host {
        Connector::lobby_host(connection_options, session)?
    } else {
        Connector::lobby_join(connection_options, &options.room, session)?
    };
    let mut ready = Some(ready);
    let connection_started = Instant::now();
    let mut start = loop {
        ensure!(
            connection_started.elapsed() < Duration::from_secs(35)
                && !control.cancel.load(Ordering::Acquire),
            "qualification connection canceled or timed out"
        );
        if let Some(invitation) = connector.take_invitation()
            && let Some(sender) = ready.take()
        {
            let _ = sender.send(invitation);
        }
        if let Some(start) = connector.poll()? {
            break start;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    ensure!(
        lobby.room_count() == 0,
        "production signaling room retained"
    );
    if let crate::netplay::Transport::Direct(peer) = &start.stream {
        ensure!(
            !peer
                .runtime
                .block_on(peer.connection.connection_path())?
                .relay,
            "unexpected relay"
        );
    } else {
        anyhow::bail!("production qualification requires RTC");
    }
    if let Some(sender) = ready.take() {
        let _ = sender.send(options.room.clone());
    }
    start.verify_every_frame = true;
    let mut worker = EmuThread::spawn(backend, false);
    worker.send(EmuCommand::StartNetplay(Box::new(start)));
    let result = execute(
        &worker,
        options,
        control,
        &expected,
        &original,
        &publication,
    );
    worker.shutdown();
    result?;
    publication.check_save()?;
    Ok(())
}

fn execute(
    worker: &EmuThread,
    options: &Options,
    control: &Control,
    expected: &[(Frame, Vec<u32>)],
    original: &[u8],
    publication: &Publication,
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
            start.elapsed() < Duration::from_secs(120) && !control.cancel.load(Ordering::Acquire),
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
                        restored && frames.len() >= options.frames,
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
                EmuResponse::SramFlushed(path) => {
                    ensure!(captured, "native publication preceded restoration");
                    publication.check_acknowledgment(path.as_deref())?;
                    progress.published = true;
                }
                EmuResponse::ShutdownComplete => {
                    ensure!(
                        captured && progress.published,
                        "native shutdown fence differs"
                    );
                    progress.frames = frames.into_iter().take(options.frames).collect();
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
            ensure!(
                progress.verified >= options.frames,
                "early native stop request"
            );
            control.progress.lock().unwrap().closed_at = progress.presented;
            worker.send(EmuCommand::StopNetplay);
            stop_sent = true;
        }
        if progress.ready
            && !pending
            && !stop_sent
            && !stopped
            && progress.verified < options.frames
            && (progress.presented < 6 || control.release.load(Ordering::Acquire))
        {
            ensure!(
                progress.presented < (options.frames + 24) as u64,
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
