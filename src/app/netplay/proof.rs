use std::io::Write;
use std::net::SocketAddr;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use zeff_netplay::{endpoint::ConnectionScope, wire::Message};

use super::{App, Phase};
use crate::emu_backend::{ActiveSystem, EmuBackend};
use crate::emu_thread::{EmuCommand, EmuResponse, EmuResponsePoll};
use crate::netplay::{connect::executable_build, identity};

mod cadence;
mod chat;
mod fault;
mod media;
mod pause;
mod route;
mod timing;
use media::Options;

#[cfg(test)]
mod lobby_tests;

#[derive(Default)]
pub(super) struct Observation {
    pub(super) last: Option<(Message, [u8; 2], Vec<f32>)>,
    pub(super) frames: u64,
    pub(super) admitted: bool,
    pub(super) connection: Option<(SocketAddr, SocketAddr, ConnectionScope)>,
    pub(super) pause_rounds: u64,
    pub(super) last_pause: Option<(u64, bool, bool)>,
    pub(super) cadence: bool,
    pub(super) presented: Vec<(u64, Instant)>,
    pub(super) depth_max: u64,
    pub(super) rollback_frames: u64,
    pub(super) stalls: u64,
    pub(super) retained_payload_max: usize,
    pub(super) pcm_hash: Sha256,
}

impl Observation {
    pub(super) fn record_pcm(&mut self, audio: &[f32]) {
        for sample in audio {
            self.pcm_hash.update(sample.to_bits().to_le_bytes());
        }
    }
}

pub(crate) fn run_if_requested() -> Result<bool> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments
        .first()
        .is_none_or(|arg| arg != "--netplay-app-proof")
    {
        return Ok(false);
    }
    let options = Options::parse(&arguments[1..])?;
    options.prepare_root()?;
    let path = options.copy_media()?;
    let report = execute(&options, &path)?;
    write_new(
        &options.root.join("report.json"),
        &serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", json!({"report": report}));
    Ok(true)
}

fn execute(options: &Options, path: &Path) -> Result<Value> {
    let mut app = setup(path)?;
    let result = run(&mut app, options, path);
    if result.is_err() {
        app.request_netplay_stop();
        let _ = wait(&mut app, Duration::from_secs(5), |app| {
            !app.netplay.fenced()
        });
    }
    app.stop_emu_thread();
    let mut report = result?;
    options.check_save(path)?;
    report["save_protection_after_shutdown"] = json!(true);
    Ok(report)
}

fn setup(path: &Path) -> Result<App> {
    let backend = media::load(path)?;
    let mut settings = crate::settings::Settings::default();
    settings.ui.check_for_updates = false;
    settings.audio.output_sample_rate = 48_000;
    settings.emulation.save_recovery_state = false;
    settings.emulation.resume_recovery_state = false;
    let mut app = super::super::construct::create(Some(backend), settings, None, false);
    ensure!(
        !app.live_control.is_enabled(),
        "disable live control for the proof"
    );
    let backend = app
        .initial_backend
        .take()
        .context("missing initial backend")?;
    app.finalize_rom_load(
        &backend,
        ActiveSystem::Nes,
        path.to_path_buf(),
        path.to_path_buf(),
    );
    app.spawn_emu_thread(backend);
    app.set_user_paused(true);
    app.netplay.proof = Some(Observation::default());
    Ok(app)
}

fn run(app: &mut App, options: &Options, path: &Path) -> Result<Value> {
    let started = Instant::now();
    let build = executable_build()?;
    let initial = capture(app)?;
    app.netplay
        .proof
        .as_mut()
        .context("proof observer lost")?
        .cadence = options.cadence;
    app.debug_windows.netplay.private_network = true;
    options.route.configure(app);
    app.debug_windows.netplay.input_delay = options.input_delay.frames();
    let invitation = if options.role == 0 {
        app.debug_windows.netplay.host_address = options.address.ip().to_string();
        app.debug_windows.netplay.host_port = options.address.port();
        None
    } else {
        Some(
            options
                .invitation
                .clone()
                .context("join invitation required")?,
        )
    };
    app.begin_netplay(invitation)?;
    app.pump_netplay();
    ensure!(app.netplay.fenced(), "{}", app.debug_windows.netplay.status);
    let save = media::optional_save(path)?;
    options.record_save(&save)?;
    if options.role == 0 {
        wait(app, Duration::from_secs(35), |app| {
            !app.debug_windows.netplay.invitation.is_empty() || !app.netplay.fenced()
        })?;
        ensure!(app.netplay.fenced(), "{}", app.debug_windows.netplay.status);
        ensure!(
            !app.debug_windows.netplay.invitation.is_empty(),
            "host invitation unavailable"
        );
        write_new(
            &options.root.join("invitation.txt"),
            app.debug_windows.netplay.invitation.as_bytes(),
        )?;
        println!("{}", json!({"listening": true}));
        std::io::stdout().flush()?;
    }
    wait(app, Duration::from_secs(40), |app| {
        app.netplay.running() || !app.netplay.fenced()
    })?;
    hold(app);
    let observation = app.netplay.proof.as_ref().context("proof observer lost")?;
    let (local, peer, scope) = options.route.endpoints(observation.connection)?;
    let mut reference = media::load(path)?;
    ensure!(
        reference.encode_state_bytes()? == initial,
        "fresh reference initial state differs"
    );
    let delay = zeff_netplay::rollback::InputDelay::new(app.debug_windows.netplay.input_delay)?;
    ensure!(
        delay == options.input_delay,
        "negotiated proof delay differs"
    );
    let reference_identity = identity::identity_with_delay(&reference, build, delay)?;
    let config = reference_identity.config;
    let info = crate::netplay::compatibility::describe(&reference, false);
    let mut report = json!({
        "production_app": true, "player": options.role + 1,
        "build_sha256": const_hex::encode(build), "app_version": info.version,
        "build_platform": info.platform, "compatibility_contract": const_hex::encode(info.contract),
        "compiled_target": env!("ZEFF_NETPLAY_BUILD_TARGET"),
        "compiled_profile": env!("ZEFF_NETPLAY_BUILD_PROFILE"),
        "compiled_opt_level": env!("ZEFF_NETPLAY_BUILD_OPT_LEVEL"),
        "compiled_debug": env!("ZEFF_NETPLAY_BUILD_DEBUG"), "test": cfg!(test),
        "compiled_full_source": env!("ZEFF_NETPLAY_FULL_SOURCE"),
        "compiled_qualification_source": env!("ZEFF_NETPLAY_QUALIFICATION_SOURCE"),
        "frontend_fp_controls": crate::netplay::compatibility::floating_point_controls(),
        "certificate": crate::netplay::compatibility::certificate_status()
            .map(|summary| json!(summary)).unwrap_or_else(|error| json!({"error": error.to_string()})),
        "source_sha256": const_hex::encode(reference_identity.source),
        "media_bytes": reference_identity.media_len,
        "local_endpoint": local, "peer_endpoint": peer,
        "scope": scope, "frames": observation.frames,
        "transport": if options.route.is_lobby() { "webrtc-data-channel" } else { "tcp" },
        "admitted": observation.admitted, "reference_checked_frames": 0,
        "jitter_ms": options.jitter_ms, "requested_fault": options.fault,
        "input_delay": delay.frames(),
    });
    if options.reject_build {
        ensure!(
            app.netplay.phase == Phase::Idle,
            "{}",
            app.debug_windows.netplay.status
        );
        ensure!(
            !observation.admitted && observation.frames == 0 && observation.last.is_none(),
            "rejected peer executed frames"
        );
        ensure!(
            app.debug_windows
                .netplay
                .status
                .contains("build compatibility contract mismatch"),
            "unexpected rejection: {}",
            app.debug_windows.netplay.status
        );
        report["outcome"] = json!(app.debug_windows.netplay.status);
    } else {
        ensure!(
            app.netplay.running() && observation.admitted,
            "{}",
            app.debug_windows.netplay.status
        );
        chat::exchange(app, options, "ready")?;
        if options.cadence {
            report["cadence"] = cadence::play(app, options)?;
        } else {
            let timings = play(app, options, path, &save, &mut reference, config)?;
            report["timing"] = timings.report();
        }
        report["paced"] = json!(options.paced);
        let observation = app.netplay.proof.as_ref().unwrap();
        report["frames"] = json!(observation.frames);
        report["reference_checked_frames"] = json!(if options.cadence {
            0
        } else if options.fault.is_some() {
            8
        } else {
            observation.frames
        });
        report["pause_rounds"] = json!(observation.pause_rounds);
        report["chat_messages"] = json!(app.debug_windows.netplay.chat.messages().count());
        if let Some((last, _, _)) = &observation.last {
            report["checkpoint"] = checkpoint_json(last)?;
        }
        report["final_video_sha256"] = json!(const_hex::encode(Sha256::digest(
            app.latest_frame
                .as_ref()
                .context("missing final frame")?
                .as_slice()
        )));
        report["confirmed_pcm_sha256"] =
            json!(const_hex::encode(observation.pcm_hash.clone().finalize()));
        report["focus_neutral_and_local_p2_ignored"] = json!(observation.frames >= 17);
        report["mutation_fences"] = json!(["reset", "sample-rate"]);
        report["outcome"] = json!(if options.fault.is_some() {
            "expected-fault-restored"
        } else {
            "complete"
        });
        if options.fault.is_some() {
            report["fault_reason"] = json!(app.debug_windows.netplay.status);
            report["speculative_video_allowed"] = json!(true);
        }
        write_new(
            &options.root.join("ready.json"),
            &serde_json::to_vec_pretty(&report)?,
        )?;
        println!(
            "{}",
            json!({"awaiting_stop": true, "verified_frames": observation.frames})
        );
        std::io::stdout().flush()?;
        let deadline = Instant::now() + Duration::from_secs(60);
        while !options.root.join("finish").exists() {
            ensure!(
                Instant::now() < deadline,
                "proof completion barrier timed out"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        app.request_netplay_stop();
        wait(app, Duration::from_secs(5), |app| {
            app.netplay.phase == Phase::Idle
        })?;
    }
    ensure!(
        app.speed.paused && app.netplay.phase == Phase::Idle,
        "proof did not return idle and paused"
    );
    ensure!(
        capture(app)? == initial,
        "exact fresh-load restoration differs"
    );
    ensure!(
        media::optional_save(path)? == save,
        "restoration changed save bytes"
    );
    report["exact_restore"] = json!(true);
    report["save_protection"] = json!(true);
    report["initial_native_sha256"] = json!(const_hex::encode(Sha256::digest(&initial)));
    report["save_sha256"] = json!(
        save.as_ref()
            .map(|bytes| const_hex::encode(Sha256::digest(bytes)))
    );
    report["elapsed_ms"] = json!(started.elapsed().as_millis());
    Ok(report)
}

fn play(
    app: &mut App,
    options: &Options,
    path: &Path,
    save: &Option<Vec<u8>>,
    reference: &mut EmuBackend,
    config: [u8; 32],
) -> Result<timing::Timings> {
    let mut timings = timing::Timings::default();
    if options.paced {
        app.netplay.next_frame = None;
    }
    for frame in 0..options.frames {
        if options.fault.is_some() && frame == 8 {
            fault::run(app, options, frame)?;
            return Ok(timings);
        }
        set_input(app, sample(frame, options.role));
        app.game_window_focused = options.role != 0 || !(5..8).contains(&frame);
        app.game_view_focused = options.role != 1 || !(9..12).contains(&frame);
        app.egui_wants_keyboard = options.role == 1 && (12..15).contains(&frame);
        if frame == 5 {
            ensure!(
                app.send_emu_command_checked(EmuCommand::Reset).is_err(),
                "reset escaped App fence"
            );
            ensure!(
                app.send_emu_command_checked(EmuCommand::SetSampleRate(44_100))
                    .is_err(),
                "sample rate escaped App fence"
            );
        }
        if options.jitter_ms > 0 {
            let delay = (frame * 13 + options.role as u64 * 7) % (options.jitter_ms + 1);
            std::thread::sleep(Duration::from_millis(delay));
        }
        let started = Instant::now();
        if options.paced {
            timing::paced_round(app)?;
        } else {
            round(app)?;
        }
        timings.rounds.push(started.elapsed());
        wait_confirmed_image(app, frame + 1)?;
        ensure!(app.netplay.confirmed == frame + 1, "confirmed frame drift");
        let delay = options.input_delay.frames();
        let ports = if frame < delay {
            [0, 0]
        } else {
            [input(frame - delay, 0), input(frame - delay, 1)]
        };
        let started = Instant::now();
        let EmuBackend::Nes(nes) = reference else {
            bail!("lost NES reference")
        };
        nes.emu.set_input_p1_raw(ports[0]);
        nes.emu.set_input_p2_raw(ports[1]);
        reference.step_frame();
        let mut audio = Vec::new();
        reference.drain_audio_samples_into(&mut audio);
        let expected = identity::checkpoint(reference, frame + 1, &audio, config)?;
        timings.references.push(started.elapsed());
        let (actual, actual_ports, actual_audio) = app
            .netplay
            .proof
            .as_ref()
            .unwrap()
            .last
            .as_ref()
            .context("confirmed frame observation missing")?;
        ensure!(
            actual == &expected && *actual_ports == ports,
            "checkpoint or delayed ports differ at {frame}"
        );
        ensure!(
            actual_audio
                .iter()
                .map(|x| x.to_bits())
                .eq(audio.iter().map(|x| x.to_bits())),
            "raw stereo audio bits differ at {frame}"
        );
        ensure!(
            app.latest_frame
                .as_ref()
                .context("published pixels missing")?
                .as_slice()
                == reference.framebuffer(),
            "published pixels differ at {frame}"
        );
        ensure!(
            media::optional_save(path)? == *save,
            "session published save bytes at {frame}"
        );
    }
    pause::after_play(app, options, reference, config)?;
    Ok(timings)
}

fn sample(frame: u64, role: usize) -> u8 {
    if role == 0 {
        ((frame * 17 + 3) ^ (frame >> 2)) as u8
    } else {
        ((frame * 29 + 11) ^ (frame >> 1)) as u8
    }
}

fn input(frame: u64, role: usize) -> u8 {
    if (role == 0 && (5..8).contains(&frame)) || (role == 1 && (9..15).contains(&frame)) {
        0
    } else {
        sample(frame, role)
    }
}

fn set_input(app: &mut App, raw: u8) {
    use crate::input::HostButton;
    app.host_input.clear_keyboard();
    for (bit, button) in [
        HostButton::A,
        HostButton::B,
        HostButton::Select,
        HostButton::Start,
        HostButton::Up,
        HostButton::Down,
        HostButton::Left,
        HostButton::Right,
    ]
    .into_iter()
    .enumerate()
    {
        app.host_input.set_keyboard(button, raw & (1 << bit) != 0);
        app.host_input.set_keyboard_p2(button, true);
    }
}

fn hold(app: &mut App) {
    app.netplay.next_frame = Some(Instant::now() + Duration::from_secs(60));
}

fn wait_confirmed_image(app: &mut App, frame: u64) -> Result<()> {
    wait(app, Duration::from_secs(10), |app| {
        app.netplay.confirmed == frame && app.netplay.published_confirmed >= frame
    })
}

fn round(app: &mut App) -> Result<()> {
    app.netplay.next_frame = None;
    app.pump_netplay();
    ensure!(app.netplay.in_flight, "App did not submit a round");
    hold(app);
    wait(app, Duration::from_secs(10), |app| !app.netplay.in_flight)?;
    ensure!(
        app.netplay.running(),
        "{}",
        app.debug_windows.netplay.status
    );
    hold(app);
    Ok(())
}

fn wait(app: &mut App, budget: Duration, predicate: impl Fn(&App) -> bool) -> Result<()> {
    let deadline = Instant::now() + budget;
    while !predicate(app) {
        // Only connection phases are pumped here; gameplay advances solely through round().
        if matches!(app.netplay.phase, Phase::Preparing(_) | Phase::Connecting) {
            app.pump_netplay();
        }
        app.drain_emu_responses();
        ensure!(
            Instant::now() < deadline,
            "App response deadline: {}",
            app.debug_windows.netplay.status
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

fn capture(app: &mut App) -> Result<Vec<u8>> {
    app.emu_thread
        .as_ref()
        .context("missing emulator worker")?
        .send(EmuCommand::CaptureStateBytes);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match app.emu_thread.as_ref().unwrap().poll_response() {
            EmuResponsePoll::Response(response) => match *response {
                EmuResponse::StateCaptured(bytes) => return Ok(bytes),
                other => {
                    app.consume_netplay_response(other);
                }
            },
            EmuResponsePoll::Disconnected => bail!("worker disconnected during capture"),
            EmuResponsePoll::Empty => {}
        }
        ensure!(Instant::now() < deadline, "state capture deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn checkpoint_json(message: &Message) -> Result<Value> {
    let Message::Checkpoint {
        frame,
        logical,
        video,
        audio,
        persistent,
    } = message
    else {
        bail!("expected checkpoint")
    };
    Ok(
        json!({"frame": frame, "logical": const_hex::encode(logical), "video": const_hex::encode(video), "audio": const_hex::encode(audio), "persistent": const_hex::encode(persistent)}),
    )
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(bytes)?;
    Ok(())
}
