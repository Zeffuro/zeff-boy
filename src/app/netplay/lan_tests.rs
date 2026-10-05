use super::tests::{app, capture, load, set_input, wait};
use super::*;
use crate::netplay::{connect::executable_build, identity};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;

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

fn hold(app: &mut App) {
    app.netplay.next_frame = Some(Instant::now() + Duration::from_secs(60));
}

fn round(app: &mut App) {
    app.netplay.next_frame = None;
    app.pump_netplay();
    assert!(app.netplay.in_flight);
    hold(app);
    wait(app, |app| !app.netplay.in_flight);
    assert!(
        app.netplay.running(),
        "{}",
        app.debug_windows.netplay.status
    );
    hold(app);
}

fn checkpoint_json(message: &Message) -> Value {
    let Message::Checkpoint {
        frame,
        logical,
        video,
        audio,
        persistent,
    } = message
    else {
        panic!("expected checkpoint")
    };
    json!({"frame": frame, "logical": logical, "video": video, "audio": audio, "persistent": persistent})
}

#[test]
#[ignore = "requires two explicitly configured private-network App processes"]
fn private_network_app_process_matches_reference_and_restores() {
    let root = PathBuf::from(
        std::env::var_os("ZEFF_NETPLAY_APP_LAN_ROOT").expect("set fresh output root"),
    );
    let role = match std::env::var("ZEFF_NETPLAY_APP_LAN_ROLE").as_deref() {
        Ok("host") => 0,
        Ok("join") => 1,
        _ => panic!("set host or join role"),
    };
    let frames: u64 = std::env::var("ZEFF_NETPLAY_APP_LAN_FRAMES")
        .unwrap_or_else(|_| "24".into())
        .parse()
        .unwrap();
    assert!((8..=1000).contains(&frames));
    let reject_build =
        std::env::var("ZEFF_NETPLAY_APP_LAN_EXPECT").as_deref() == Ok("reject-build");
    std::fs::create_dir_all(root.parent().expect("output needs parent directory")).unwrap();
    std::fs::create_dir(&root).expect("output root must be new");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut app = app(&root, "one");
    let initial = capture(&mut app);
    let build = executable_build().unwrap();
    app.debug_windows.netplay.private_network = true;
    let invitation = if role == 0 {
        let address: SocketAddr = std::env::var("ZEFF_NETPLAY_APP_LAN_ADDRESS")
            .expect("set specific host address")
            .parse()
            .unwrap();
        app.debug_windows.netplay.host_address = address.ip().to_string();
        app.debug_windows.netplay.host_port = address.port();
        None
    } else {
        Some(
            std::env::var("ZEFF_NETPLAY_APP_LAN_INVITATION")
                .expect("set invitation in environment"),
        )
    };
    app.begin_netplay(invitation).unwrap();
    app.pump_netplay();
    assert!(app.netplay.fenced(), "{}", app.debug_windows.netplay.status);
    let path = app.rom_info.rom_path.clone().unwrap();
    let save = std::fs::read(path.with_extension("sav")).unwrap();
    if role == 0 {
        let invitation = &app.debug_windows.netplay.invitation;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("invitation.txt"))
            .unwrap();
        file.write_all(invitation.as_bytes()).unwrap();
        println!(
            "{}",
            json!({"listening": invitation.split_once('/').unwrap().0})
        );
        std::io::stdout().flush().unwrap();
    }
    let mut connection = app.netplay.observed_connection;
    let deadline = Instant::now() + Duration::from_secs(35);
    while !app.netplay.running() && app.netplay.fenced() {
        app.pump_netplay();
        connection = app.netplay.observed_connection.or(connection);
        app.drain_emu_responses();
        assert!(Instant::now() < deadline, "App admission deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
    let (local, peer, scope) = connection.expect("connector must return an owned socket");
    assert_eq!(scope, ConnectionScope::TrustedPrivate);
    hold(&mut app);
    let mut report = json!({
        "player": role + 1, "build_sha256": const_hex::encode(build),
        "local_endpoint": local.to_string(), "peer_endpoint": peer.to_string(),
        "scope": "trusted-private-plaintext", "frames": 0,
        "admitted": app.netplay.admitted, "reference_checked_frames": 0,
    });
    if reject_build {
        assert!(
            app.netplay.phase == Phase::Idle,
            "{}",
            app.debug_windows.netplay.status
        );
        assert!(!app.netplay.admitted && app.netplay.confirmed == 0);
        assert!(app.netplay.observed_frames.is_empty());
        assert!(
            app.debug_windows
                .netplay
                .status
                .contains("build compatibility contract mismatch")
        );
        report["outcome"] = json!(app.debug_windows.netplay.status);
    } else {
        assert!(
            app.netplay.running(),
            "{}",
            app.debug_windows.netplay.status
        );
        let mut reference = load(&path);
        assert_eq!(reference.encode_state_bytes().unwrap(), initial);
        let config = identity::identity(&reference, build).unwrap().config;
        let mut pause_rounds = 0;
        for frame in 0..frames {
            if frame == 4 || frame == frames / 2 {
                let picture = app
                    .latest_frame
                    .as_ref()
                    .map(|pixels| pixels.as_slice().to_vec());
                for paused in [[true, false], [true, true], [false, true]] {
                    app.set_netplay_paused(paused[role]);
                    round(&mut app);
                    assert!(app.netplay.paused);
                    assert_eq!(app.netplay.confirmed, frame);
                    assert_eq!(app.netplay.observed_frames.len(), frame as usize);
                    assert_eq!(
                        app.latest_frame
                            .as_ref()
                            .map(|pixels| pixels.as_slice().to_vec()),
                        picture
                    );
                    pause_rounds += 1;
                }
                app.set_netplay_paused(false);
            }
            set_input(&mut app, sample(frame, role));
            app.game_window_focused = role != 0 || !(5..8).contains(&frame);
            app.game_view_focused = role != 1 || !(9..12).contains(&frame);
            app.egui_wants_keyboard = role == 1 && (12..15).contains(&frame);
            if frame == 5 {
                assert!(app.send_emu_command_checked(EmuCommand::Reset).is_err());
                assert!(
                    app.send_emu_command_checked(EmuCommand::SetSampleRate(44_100))
                        .is_err()
                );
            }
            round(&mut app);
            assert_eq!(app.netplay.confirmed, frame + 1);
            let ports = if frame < 2 {
                [0, 0]
            } else {
                [input(frame - 2, 0), input(frame - 2, 1)]
            };
            let crate::emu_backend::EmuBackend::Nes(nes) = &mut reference else {
                panic!("expected NES reference")
            };
            nes.emu.set_input_p1_raw(ports[0]);
            nes.emu.set_input_p2_raw(ports[1]);
            reference.step_frame();
            let mut audio = Vec::new();
            reference.drain_audio_samples_into(&mut audio);
            let expected = identity::checkpoint(&reference, frame + 1, &audio, config).unwrap();
            let (actual, actual_ports, actual_audio) = app.netplay.observed_frames.last().unwrap();
            assert_eq!(actual, &expected);
            assert_eq!(*actual_ports, ports.map(u16::from));
            assert_eq!(
                actual_audio.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
                audio.iter().map(|x| x.to_bits()).collect::<Vec<_>>()
            );
            assert_eq!(
                app.latest_frame.as_ref().unwrap().as_slice(),
                reference.framebuffer()
            );
            assert_eq!(std::fs::read(path.with_extension("sav")).unwrap(), save);
        }
        report["frames"] = json!(frames);
        report["reference_checked_frames"] = json!(frames);
        report["pause_rounds"] = json!(pause_rounds);
        report["checkpoint"] = checkpoint_json(&app.netplay.observed_frames.last().unwrap().0);
        report["outcome"] = json!("complete");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("ready.json"))
            .unwrap();
        file.write_all(&serde_json::to_vec_pretty(&report).unwrap())
            .unwrap();
        println!(
            "{}",
            json!({"awaiting_stop": true, "verified_frames": frames})
        );
        std::io::stdout().flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !root.join("finish").exists() {
            assert!(
                Instant::now() < deadline,
                "proof completion barrier timed out"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        app.request_netplay_stop();
        wait(&mut app, |app| app.netplay.phase == Phase::Idle);
    }
    assert!(app.speed.paused);
    assert_eq!(capture(&mut app), initial);
    app.stop_emu_thread();
    assert_eq!(std::fs::read(path.with_extension("sav")).unwrap(), save);
    report["exact_restore"] = json!(true);
    report["save_protection"] = json!(true);
    report["initial_native_sha256"] = json!(const_hex::encode(Sha256::digest(&initial)));
    report["save_sha256"] = json!(const_hex::encode(Sha256::digest(&save)));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("report.json"))
        .unwrap();
    file.write_all(&serde_json::to_vec_pretty(&report).unwrap())
        .unwrap();
    println!("{}", json!({"report": report}));
}
