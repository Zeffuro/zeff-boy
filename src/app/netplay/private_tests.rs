use super::tests::{app, capture, connect_pair, set_input, wait};
use super::*;
use std::net::TcpListener;

fn private_host(app: &mut App) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    app.debug_windows.netplay.private_network = true;
    app.debug_windows.netplay.host_address = "127.0.0.1".into();
    app.debug_windows.netplay.host_port = listener.local_addr().unwrap().port();
}

#[test]
fn invalid_private_host_and_nonlocal_invitation_do_not_reload_or_pause() {
    let directory = crate::test_support::test_directory("app-netplay-private-validation").unwrap();
    let mut app = app(directory.path(), "one");
    let initial = capture(&mut app);
    let generation = app.emu_worker_generation;
    let invitation = format!("192.168.1.1:8766/{}", "ab".repeat(32));
    assert!(app.begin_netplay(Some(invitation)).is_err());
    app.debug_windows.netplay.private_network = true;
    for address in ["", "0.0.0.0", "8.8.8.8", "localhost", "fe80::1"] {
        app.debug_windows.netplay.host_address = address.into();
        assert!(app.begin_netplay(None).is_err(), "{address}");
        assert!(app.netplay.phase == Phase::Idle);
        assert!(!app.speed.paused);
        assert_eq!(app.emu_worker_generation, generation);
        assert!(app.netplay.connector.is_none());
        assert!(!app.debug_windows.netplay.active);
    }
    assert_eq!(capture(&mut app), initial);
    assert!(
        !app.rom_info
            .rom_path
            .as_ref()
            .unwrap()
            .with_extension("sav")
            .exists()
    );
    app.stop_emu_thread();
}

#[test]
fn host_endpoint_and_scope_are_captured_before_fresh_load() {
    let directory = crate::test_support::test_directory("app-netplay-private-snapshot").unwrap();
    let mut app = app(directory.path(), "one");
    private_host(&mut app);
    let port = app.debug_windows.netplay.host_port;
    app.begin_netplay(None).unwrap();
    app.debug_windows.netplay.private_network = false;
    app.debug_windows.netplay.host_address = "8.8.8.8".into();
    app.debug_windows.netplay.host_port = 1;
    app.pump_netplay();
    assert!(
        app.netplay.phase == Phase::Connecting,
        "{}",
        app.debug_windows.netplay.status
    );
    assert!(
        app.debug_windows
            .netplay
            .invitation
            .starts_with(&format!("127.0.0.1:{port}/"))
    );
    app.request_netplay_stop();
    assert!(app.netplay.phase == Phase::Idle);
    assert!(TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_ok());
    app.stop_emu_thread();
}

#[test]
fn explicit_private_apps_execute_and_restore_without_save_publication() {
    let directory = crate::test_support::test_directory("app-netplay-private-pair").unwrap();
    let mut one = app(directory.path(), "one");
    let mut two = app(directory.path(), "two");
    private_host(&mut one);
    two.debug_windows.netplay.private_network = true;
    let initial = connect_pair(&mut one, &mut two);
    let paths = [
        one.rom_info.rom_path.clone().unwrap(),
        two.rom_info.rom_path.clone().unwrap(),
    ];
    let saves = paths
        .each_ref()
        .map(|p| std::fs::read(p.with_extension("sav")).unwrap());
    for frame in 0..8_u64 {
        for app in [&mut one, &mut two] {
            set_input(app, frame as u8 + 1);
            app.netplay.next_frame = None;
            app.pump_netplay();
            app.netplay.next_frame = Some(Instant::now() + Duration::from_secs(60));
        }
        for app in [&mut one, &mut two] {
            wait(app, |app| {
                app.netplay.confirmed == frame + 1
                    && !app.netplay.in_flight
                    && app.netplay.published_confirmed > frame
            });
            assert!(app.netplay.running());
        }
        assert_eq!(
            one.netplay.observed_frames.last(),
            two.netplay.observed_frames.last()
        );
        for (path, expected) in paths.iter().zip(&saves) {
            assert_eq!(
                &std::fs::read(path.with_extension("sav")).unwrap(),
                expected
            );
        }
    }
    one.request_netplay_stop();
    two.request_netplay_stop();
    for (app, expected) in [(&mut one, initial.0), (&mut two, initial.1)] {
        wait(app, |app| app.netplay.phase == Phase::Idle);
        assert!(app.speed.paused);
        assert_eq!(capture(app), expected);
        app.stop_emu_thread();
    }
    for (path, expected) in paths.iter().zip(&saves) {
        assert_eq!(
            &std::fs::read(path.with_extension("sav")).unwrap(),
            expected
        );
    }
}
