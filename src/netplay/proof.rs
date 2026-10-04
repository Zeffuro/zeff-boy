use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use zeff_netplay::lockstep::Player;
use zeff_netplay::wire::Message;

use super::{Response, Start, identity};
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use crate::emu_thread::{EmuCommand, EmuResponse, EmuResponsePoll, EmuThread};

mod media;
mod pause;

#[cfg(test)]
pub(crate) use media::fixture_rom;

pub(crate) fn run_if_requested() -> Result<bool> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments
        .first()
        .is_none_or(|arg| arg != "--netplay-worker-proof")
    {
        return Ok(false);
    }
    let media::Options {
        root,
        frames,
        cartridge,
        timing,
    } = media::Options::parse(&arguments[1..])?;
    std::fs::create_dir_all(&root)?;
    let config_root = PathBuf::from(
        std::env::var_os("ZEFF_CONFIG_DIR")
            .context("set ZEFF_CONFIG_DIR to a directory inside OUTPUT_DIRECTORY for this proof")?,
    );
    std::fs::create_dir_all(&config_root)?;
    ensure!(
        std::fs::canonicalize(&config_root)?.starts_with(std::fs::canonicalize(&root)?),
        "ZEFF_CONFIG_DIR must be inside OUTPUT_DIRECTORY"
    );
    let build = Sha256::digest(std::fs::read(std::env::current_exe()?)?).into();
    let report = match cartridge {
        Some(path) => run_media(&root, frames, build, &media::Media::cartridge(&path)?)?,
        None if timing == zeff_nes_core::hardware::cartridge::TimingMode::Ntsc => {
            run(&root, frames, build)?
        }
        None => run_media(&root, frames, build, &media::Media::fixture_timing(timing))?,
    };
    println!("{}", serde_json::to_string(&report)?);
    Ok(true)
}

#[cfg(test)]
fn loaded(root: &Path, name: &str) -> Result<(PathBuf, EmuBackend)> {
    media::Media::fixture().load(root, name)
}

fn sockets() -> Result<(TcpStream, TcpStream)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let one = TcpStream::connect(listener.local_addr()?)?;
    let (two, _) = listener.accept()?;
    Ok((one, two))
}

fn next_any(worker: &EmuThread) -> Result<Response> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match worker.poll_response() {
            EmuResponsePoll::Response(response) => {
                if let EmuResponse::Netplay(response) = *response {
                    return Ok(response);
                }
                bail!("unexpected worker response");
            }
            EmuResponsePoll::Disconnected => bail!("worker disconnected"),
            EmuResponsePoll::Empty => {}
        }
        ensure!(Instant::now() < deadline, "worker response deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn next(worker: &EmuThread) -> Result<Response> {
    loop {
        let response = next_any(worker)?;
        if !matches!(response, Response::Presented { .. }) {
            return Ok(response);
        }
    }
}

fn stepped_frame(worker: &EmuThread) -> Result<Response> {
    let mut frame = None;
    let mut complete = false;
    while frame.is_none() || !complete {
        match next_any(worker)? {
            response @ Response::Frame { .. } => {
                ensure!(frame.is_none(), "multiple frames in one proof step");
                frame = Some(response);
            }
            Response::Presented {
                step_complete: true,
                ..
            } => complete = true,
            Response::Presented { .. } => {}
            Response::Paused { .. } => {}
            response => bail!("unexpected proof step: {}", response_summary(&Ok(response))),
        }
    }
    Ok(frame.unwrap())
}

fn sample(frame: u64, player: Player) -> u8 {
    match player {
        Player::One => ((frame * 17 + 3) ^ (frame >> 2)) as u8,
        Player::Two => ((frame * 29 + 11) ^ (frame >> 1)) as u8,
    }
}

fn response_summary(response: &Result<Response>) -> String {
    match response {
        Ok(Response::Ready) => "ready".into(),
        Ok(Response::Chat { .. }) => "chat".into(),
        Ok(Response::ChatError(_)) => "chat error".into(),
        Ok(Response::Frame { .. }) => "confirmed frame".into(),
        Ok(Response::Audio { .. }) => "confirmed audio".into(),
        Ok(Response::Presented { .. }) => "presented frame".into(),
        Ok(Response::Paused { frame, local, peer }) => {
            format!("paused at {frame}, local={local}, peer={peer}")
        }
        Ok(Response::Stopped { reason, restored }) => {
            format!("stopped: {reason}; restored={restored}")
        }
        Ok(Response::Rejected(reason)) => format!("rejected: {reason}"),
        Err(error) => format!("response error: {error:#}"),
    }
}

fn reference_frame(
    backend: &mut EmuBackend,
    ports: [u8; 2],
    config: [u8; 32],
) -> Result<(Message, Vec<f32>)> {
    let EmuBackend::Nes(nes) = backend else {
        bail!("lost NES")
    };
    nes.emu.set_input_p1_raw(ports[0]);
    nes.emu.set_input_p2_raw(ports[1]);
    backend.step_frame();
    let mut audio = Vec::new();
    backend.drain_audio_samples_into(&mut audio);
    Ok((
        identity::checkpoint(backend, backend.frame_count(), &audio, config)?,
        audio,
    ))
}

fn stopped(worker: &EmuThread) -> Result<String> {
    loop {
        match next(worker)? {
            Response::Stopped {
                reason,
                restored: true,
            } => return Ok(reason),
            Response::Stopped {
                reason,
                restored: false,
            } => bail!("failed restoration: {reason}"),
            Response::Frame { .. } | Response::Audio { .. } | Response::Paused { .. } => {}
            _ => bail!("expected stopped response"),
        }
    }
}

fn capture(worker: &EmuThread) -> Result<Vec<u8>> {
    worker.send(EmuCommand::CaptureStateBytes);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match worker.poll_response() {
            EmuResponsePoll::Response(response) => match *response {
                EmuResponse::StateCaptured(state) => return Ok(state),
                EmuResponse::Netplay(Response::Rejected(reason))
                    if reason == "no netplay session" => {}
                EmuResponse::Netplay(Response::Stopped { restored: true, .. }) => {}
                _ => bail!("unexpected restoration capture response"),
            },
            EmuResponsePoll::Empty => {}
            EmuResponsePoll::Disconnected => bail!("worker disconnected during capture"),
        }
        ensure!(Instant::now() < deadline, "capture deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
}

pub(crate) fn run(root: &Path, frames: u64, build: [u8; 32]) -> Result<serde_json::Value> {
    run_media(root, frames, build, &media::Media::fixture())
}

fn run_media(
    root: &Path,
    frames: u64,
    build: [u8; 32],
    media: &media::Media,
) -> Result<serde_json::Value> {
    ensure!((8..=100_000).contains(&frames), "frames must be 8..100000");
    let started = Instant::now();
    let (path_one, one) = media.load(root, "one")?;
    let (path_two, two) = media.load(root, "two")?;
    let (_, mut reference) = media.load(root, "reference")?;
    let identity = identity::identity(&reference, build)?;
    let build_info = super::compatibility::describe(&reference, false);
    let config = identity.config;
    let battery = reference.nes().unwrap().emu.has_battery();
    let timing = reference.nes().unwrap().emu.resolved_timing_mode();
    let resolved_timing = match timing {
        zeff_nes_core::hardware::cartridge::TimingMode::Ntsc => "ntsc",
        zeff_nes_core::hardware::cartridge::TimingMode::Pal => "pal",
        zeff_nes_core::hardware::cartridge::TimingMode::Dendy => "dendy",
        _ => bail!("proof requires resolved NES timing"),
    };
    let mapper = reference
        .nes()
        .unwrap()
        .emu
        .cartridge_effective_mapper_label();
    let initial_one = one.encode_state_bytes()?;
    let initial_two = two.encode_state_bytes()?;
    let persistent = identity::identity(&one, build)?.persistent;
    let (stream_one, stream_two) = sockets()?;
    let mut one = EmuThread::spawn(one, false);
    let mut two = EmuThread::spawn(two, false);
    // Both peers use the same newly generated capability, retained only in memory.
    let mut secret = [0; 32];
    getrandom::fill(&mut secret).context("creating session capability")?;
    one.send(EmuCommand::StartNetplay(Box::new(Start {
        allow_different_versions: false,
        verify_every_frame: true,
        input_delay: zeff_netplay::rollback::InputDelay::default(),
        scope: zeff_netplay::endpoint::ConnectionScope::Loopback,
        stream: stream_one.into(),
        player: Player::One,
        build,
        secret,
    })));
    two.send(EmuCommand::StartNetplay(Box::new(Start {
        allow_different_versions: false,
        verify_every_frame: true,
        input_delay: zeff_netplay::rollback::InputDelay::default(),
        scope: zeff_netplay::endpoint::ConnectionScope::Loopback,
        stream: stream_two.into(),
        player: Player::Two,
        build,
        secret,
    })));
    ensure!(
        matches!(next(&one)?, Response::Ready),
        "P1 admission failed"
    );
    ensure!(
        matches!(next(&two)?, Response::Ready),
        "P2 admission failed"
    );
    let mut last = None;
    let forbidden = root.join("forbidden.state");
    for frame in 0..frames {
        if frame == 5 {
            for command in [
                EmuCommand::Reset,
                EmuCommand::SetSampleRate(44_100),
                EmuCommand::SaveStateToPath(forbidden.clone()),
                EmuCommand::CaptureStateBytes,
                EmuCommand::AcquireTasControl {
                    request_id: 77,
                    profile: crate::emu_thread::TasExecutionProfile::DirectNesCartridge,
                },
            ] {
                one.send(command);
                ensure!(
                    matches!(next(&one)?, Response::Rejected(reason) if reason.contains("owns worker")),
                    "mutation escaped session owner"
                );
            }
        }
        one.send(EmuCommand::StepNetplay(sample(frame, Player::One)));
        two.send(EmuCommand::StepNetplay(sample(frame, Player::Two)));
        let ports = if frame < 2 {
            [0, 0]
        } else {
            [
                sample(frame - 2, Player::One),
                sample(frame - 2, Player::Two),
            ]
        };
        let (expected, expected_audio) = reference_frame(&mut reference, ports, config)?;
        let responses = [stepped_frame(&one), stepped_frame(&two)];
        ensure!(
            responses
                .iter()
                .all(|response| matches!(response, Ok(Response::Frame { .. }))),
            "worker responses at frame {frame}: P1 {}; P2 {}",
            response_summary(&responses[0]),
            response_summary(&responses[1])
        );
        for (worker, response) in [&one, &two].into_iter().zip(responses) {
            let Response::Frame {
                checkpoint,
                ports: actual_ports,
                audio,
            } = response?
            else {
                unreachable!()
            };
            ensure!(
                checkpoint == expected && actual_ports == ports,
                "worker/reference mismatch at {frame}"
            );
            ensure!(
                audio
                    .iter()
                    .map(|x| x.to_bits())
                    .eq(expected_audio.iter().map(|x| x.to_bits())),
                "worker/reference audio differs at {frame}"
            );
            ensure!(
                worker.shared_framebuffer().load_full().unwrap().as_slice()
                    == reference.framebuffer(),
                "published worker/reference pixels differ at {frame}"
            );
        }
        ensure!(
            !path_one.with_extension("sav").exists() && !path_two.with_extension("sav").exists(),
            "session SRAM was published"
        );
        last = Some(expected);
    }
    ensure!(!forbidden.exists(), "state save escaped session owner");
    let final_persistent = <[u8; 32]>::from(Sha256::digest(
        reference
            .nes()
            .unwrap()
            .emu
            .dump_persistent_data()
            .unwrap_or_default(),
    ));
    if media.synthetic {
        ensure!(
            final_persistent != persistent,
            "fixture did not mutate SRAM"
        );
    }
    let pause_rounds = pause::hold([&one, &two], frames, &mut reference, config)?;
    one.send(EmuCommand::StopNetplay);
    two.send(EmuCommand::StopNetplay);
    stopped(&one)?;
    stopped(&two)?;
    for (worker, expected) in [(&one, initial_one), (&two, initial_two)] {
        ensure!(
            capture(worker)? == expected,
            "worker did not restore exact initial state"
        );
    }
    one.shutdown();
    two.shutdown();
    for path in [path_one, path_two] {
        if media.synthetic {
            ensure!(
                <[u8; 32]>::from(Sha256::digest(std::fs::read(path.with_extension("sav"))?))
                    == persistent,
                "shutdown published session SRAM"
            );
        } else {
            ensure!(
                !path.with_extension("sav").exists(),
                "cartridge proof published SRAM"
            );
        }
    }
    let Message::Checkpoint {
        frame,
        logical,
        video,
        audio,
        persistent,
    } = last.unwrap()
    else {
        unreachable!()
    };
    Ok(
        serde_json::json!({"frames": frame, "elapsed_ms": started.elapsed().as_millis(),
        "logical": const_hex::encode(logical), "video": const_hex::encode(video),
        "audio": const_hex::encode(audio), "persistent": const_hex::encode(persistent),
        "source_sha256": const_hex::encode(identity.source), "media_bytes": identity.media_len,
        "build_sha256": const_hex::encode(build), "app_version": build_info.version,
        "compatibility_contract": const_hex::encode(build_info.contract),
        "build_platform": build_info.platform,
        "allow_different_versions": build_info.allow_different_versions,
        "compiled_target": env!("ZEFF_NETPLAY_BUILD_TARGET"),
        "compiled_profile": env!("ZEFF_NETPLAY_BUILD_PROFILE"),
        "compiled_opt_level": env!("ZEFF_NETPLAY_BUILD_OPT_LEVEL"),
        "compiled_debug": env!("ZEFF_NETPLAY_BUILD_DEBUG"), "test": cfg!(test),
        "frontend_fp_controls": super::compatibility::floating_point_controls(),
        "compiled_full_source": env!("ZEFF_NETPLAY_FULL_SOURCE"),
        "compiled_qualification_source": env!("ZEFF_NETPLAY_QUALIFICATION_SOURCE"),
        "observed_build_receipt": option_env!("ZEFF_NETPLAY_OBSERVED_RECEIPT_V1"),
        "admission_build_receipt": option_env!("ZEFF_NETPLAY_BUILD_RECEIPT_V1"),
        "mapper": mapper, "battery": battery, "resolved_timing": resolved_timing,
        "pause_rounds": pause_rounds, "pause_approach_frames": 12, "core_frames_before_restore": reference.frame_count(),
        "persistent_changed": final_persistent != identity.persistent,
        "save_policy": if media.synthetic { "restored_initial" } else { "disabled" },
        "delayed_reference": true, "exact_restore": true, "save_protection": true}),
    )
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod pause_tests;

#[cfg(test)]
mod fault_tests;

#[cfg(test)]
mod regional_tests;
