use super::*;
use crate::app::netplay::tests::{app, capture, connect_pair, wait};

#[test]
fn closing_native_controls_keeps_the_worker_session_and_pause_vote() {
    let directory = crate::test_support::test_directory("netplay-native-window-close").unwrap();
    let mut one = app(directory.path(), "one");
    let mut two = app(directory.path(), "two");
    one.debug_windows.netplay.input_delay = 8;
    let initial = connect_pair(&mut one, &mut two);
    one.debug_windows.netplay.open();
    one.game_window_focused = false;
    one.settings.emulation.pause_on_unfocus = true;
    one.handle_netplay_window_event(WindowEvent::Focused(true));
    one.apply_focus_state();
    assert!(one.window_focused);
    assert!(!one.speed.paused);
    one.handle_netplay_action(&MenuAction::SetNesNetplayPaused(true));
    wait(&mut one, |app| app.netplay.local_pause);
    one.handle_netplay_window_event(WindowEvent::CloseRequested);
    one.apply_focus_state();
    assert!(!one.debug_windows.netplay.is_open());
    assert!(!one.debug_windows.netplay.host_window_focused());
    assert!(one.netplay.running());
    assert!(one.netplay.local_pause);
    assert!(one.debug_windows.netplay.active);
    assert_eq!(one.netplay.presented, 0);
    one.debug_windows.netplay.open();
    assert!(one.debug_windows.netplay.take_focus_request());
    one.handle_netplay_action(&MenuAction::SetNesNetplayPaused(false));
    wait(&mut one, |app| !app.netplay.local_pause);
    for app in [&mut one, &mut two] {
        app.netplay.next_frame = None;
        app.pump_netplay();
        app.netplay.next_frame = Some(Instant::now() + std::time::Duration::from_secs(60));
        wait(app, |app| !app.netplay.in_flight);
        assert!(app.netplay.running());
        assert_eq!(app.netplay.presented, 1);
    }
    one.handle_netplay_action(&MenuAction::StopNesNetplay);
    wait(&mut one, |app| !app.netplay.fenced());
    wait(&mut two, |app| !app.netplay.fenced());
    assert_eq!(capture(&mut one), initial.0);
    assert_eq!(capture(&mut two), initial.1);
}

#[test]
fn native_controls_report_bad_invitation_without_reloading_or_pausing_game() {
    let directory = crate::test_support::test_directory("netplay-native-window-invalid").unwrap();
    let mut app = app(directory.path(), "one");
    let initial = capture(&mut app);
    let paused = app.speed.paused;
    app.handle_netplay_action(&MenuAction::JoinNesNetplay("invalid".into()));
    assert!(!app.netplay.fenced());
    assert!(!app.debug_windows.netplay.active);
    assert!(!app.debug_windows.netplay.status.is_empty());
    assert_eq!(app.speed.paused, paused);
    assert_eq!(capture(&mut app), initial);
}
