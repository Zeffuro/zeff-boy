use super::tests::{app_with_timing, audio_bits, capture, load, set_input, wait};
use super::*;
use crate::emu_backend::EmuBackend;
use crate::netplay::{connect::executable_build, identity, test_lobby::TestLobby};
use zeff_nes_core::hardware::cartridge::TimingMode;
use zeff_netplay::rollback::InputDelay;

#[test]
fn lobby_apps_match_regional_reference_and_restore_saves() {
    let lobby = TestLobby::start();
    for timing in [TimingMode::Ntsc, TimingMode::Pal, TimingMode::Dendy] {
        for delay in [0, 2] {
            play(&lobby, timing, delay);
        }
    }
}

fn hold(app: &mut App) {
    app.netplay.next_frame = Some(Instant::now() + Duration::from_secs(60));
}

fn play(lobby: &TestLobby, timing: TimingMode, delay: u64) {
    let directory = crate::test_support::test_directory("app-lobby-gameplay").unwrap();
    let mut one = app_with_timing(directory.path(), "one", timing);
    let mut two = app_with_timing(directory.path(), "two", timing);
    for app in [&mut one, &mut two] {
        app.debug_windows.netplay.lobby = true;
        app.debug_windows.netplay.lobby_url = lobby.url.clone();
        app.debug_windows.netplay.input_delay = delay;
    }
    one.begin_netplay(None).unwrap();
    one.pump_netplay();
    let initial_one = capture(&mut one);
    let deadline = Instant::now() + Duration::from_secs(15);
    while one.debug_windows.netplay.invitation.is_empty() {
        one.pump_netplay();
        one.drain_emu_responses();
        assert!(one.netplay.fenced(), "{}", one.debug_windows.netplay.status);
        assert!(Instant::now() < deadline, "Lobby invitation deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
    two.begin_netplay(Some(one.debug_windows.netplay.invitation.clone()))
        .unwrap();
    two.pump_netplay();
    let initial_two = capture(&mut two);
    while !one.netplay.running() || !two.netplay.running() {
        for app in [&mut one, &mut two] {
            if !app.netplay.running() {
                app.pump_netplay();
            }
            app.drain_emu_responses();
            assert!(app.netplay.fenced(), "{}", app.debug_windows.netplay.status);
            hold(app);
        }
        assert!(Instant::now() < deadline, "Lobby admission deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(lobby.lobby.room_count(), 0);
    assert!(one.netplay.observed_connection.is_none());
    assert_eq!(initial_one, initial_two);
    let paths = [
        one.rom_info.rom_path.clone().unwrap(),
        two.rom_info.rom_path.clone().unwrap(),
    ];
    let saves = paths
        .each_ref()
        .map(|path| std::fs::read(path.with_extension("sav")).unwrap());
    let mut reference = load(&paths[0]);
    let config = identity::identity_with_delay(
        &reference,
        executable_build().unwrap(),
        InputDelay::new(delay).unwrap(),
    )
    .unwrap()
    .config;
    let mut scheduled = Vec::new();
    for frame in 0..24_u64 {
        let raw = [(frame * 17 + 3) as u8, (frame * 29 + 11) as u8];
        scheduled.push(raw);
        for (app, raw) in [(&mut one, raw[0]), (&mut two, raw[1])] {
            set_input(app, raw);
            app.netplay.next_frame = None;
            app.pump_netplay();
            hold(app);
        }
        for app in [&mut one, &mut two] {
            wait(app, |app| {
                app.netplay.confirmed == frame + 1
                    && app.netplay.published_confirmed == frame + 1
                    && !app.netplay.in_flight
            });
            assert!(
                app.netplay.running(),
                "{}",
                app.debug_windows.netplay.status
            );
        }
        let ports = if frame < delay {
            [0, 0]
        } else {
            scheduled[(frame - delay) as usize]
        };
        let EmuBackend::Nes(nes) = &mut reference else {
            unreachable!()
        };
        nes.emu.set_input_p1_raw(ports[0]);
        nes.emu.set_input_p2_raw(ports[1]);
        reference.step_frame();
        let mut audio = Vec::new();
        reference.drain_audio_samples_into(&mut audio);
        let expected = identity::checkpoint(&reference, frame + 1, &audio, config).unwrap();
        for app in [&one, &two] {
            let (checkpoint, actual_ports, actual_audio) =
                app.netplay.observed_frames.last().unwrap();
            assert_eq!(
                *checkpoint, expected,
                "{timing:?} delay{delay} frame{frame}"
            );
            assert_eq!(*actual_ports, ports);
            assert_eq!(audio_bits(actual_audio), audio_bits(&audio));
        }
    }
    one.request_netplay_stop();
    two.request_netplay_stop();
    for (app, initial) in [(&mut one, initial_one), (&mut two, initial_two)] {
        wait(app, |app| app.netplay.phase == Phase::Idle);
        assert_eq!(capture(app), initial);
        assert!(app.speed.paused);
        app.stop_emu_thread();
    }
    for (path, expected) in paths.iter().zip(saves) {
        assert_eq!(std::fs::read(path.with_extension("sav")).unwrap(), expected);
    }
}

#[test]
fn cancel_lobby_wait_releases_connector_and_room() {
    let lobby = TestLobby::start();
    let directory = crate::test_support::test_directory("app-lobby-cancel").unwrap();
    let mut app = app_with_timing(directory.path(), "one", TimingMode::Ntsc);
    app.debug_windows.netplay.lobby = true;
    app.debug_windows.netplay.lobby_url = lobby.url.clone();
    app.begin_netplay(None).unwrap();
    app.pump_netplay();
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.debug_windows.netplay.invitation.is_empty() {
        app.pump_netplay();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let start = Instant::now();
    app.request_netplay_stop();
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(app.netplay.phase == Phase::Idle);
    assert!(app.speed.paused);
    while lobby.lobby.room_count() != 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    app.stop_emu_thread();
}
