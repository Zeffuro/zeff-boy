use super::tests::{app, capture, connect_pair, wait};
use crate::debug::MenuAction;

#[test]
fn chat_action_delivers_both_directions_at_frame_zero_and_preserves_restore() {
    let directory = crate::test_support::test_directory("netplay-player-chat").unwrap();
    let mut one = app(directory.path(), "one");
    let mut two = app(directory.path(), "two");
    let initial = connect_pair(&mut one, &mut two);
    one.handle_netplay_action(&MenuAction::SendNesNetplayChat("  Ready? 🎮  ".into()));
    wait(&mut one, |app| {
        app.debug_windows.netplay.chat.messages().count() == 1
    });
    wait(&mut two, |app| {
        app.debug_windows.netplay.chat.messages().count() == 1
    });
    let sent = one.debug_windows.netplay.chat.messages().next().unwrap();
    assert!(sent.local);
    assert_eq!(sent.text, "Ready? 🎮");
    let received = two.debug_windows.netplay.chat.messages().next().unwrap();
    assert!(!received.local);
    assert_eq!(received.text, sent.text);
    two.handle_netplay_action(&MenuAction::SendNesNetplayChat("Let's play".into()));
    for app in [&mut one, &mut two] {
        wait(app, |app| {
            app.debug_windows.netplay.chat.messages().count() == 2
        });
        assert!(app.netplay.running());
        assert_eq!(app.netplay.presented, 0);
        assert_eq!(app.netplay.confirmed, 0);
    }
    one.request_netplay_stop();
    wait(&mut one, |app| !app.netplay.fenced());
    wait(&mut two, |app| !app.netplay.fenced());
    assert_eq!(capture(&mut one), initial.0);
    assert_eq!(capture(&mut two), initial.1);
    assert_eq!(one.debug_windows.netplay.chat.messages().count(), 2);
    one.begin_netplay(None).unwrap();
    assert_eq!(one.debug_windows.netplay.chat.messages().count(), 0);
    one.request_netplay_stop();
}

#[test]
fn invalid_or_fast_chat_is_rejected_without_stopping_gameplay() {
    let directory = crate::test_support::test_directory("netplay-chat-validation").unwrap();
    let mut one = app(directory.path(), "one");
    let mut two = app(directory.path(), "two");
    connect_pair(&mut one, &mut two);
    one.handle_netplay_action(&MenuAction::SendNesNetplayChat("a\nb".into()));
    wait(&mut one, |app| {
        !app.debug_windows.netplay.chat.error.is_empty()
    });
    assert_eq!(one.debug_windows.netplay.chat.messages().count(), 0);
    for index in 0..4 {
        one.handle_netplay_action(&MenuAction::SendNesNetplayChat(format!("message {index}")));
        wait(&mut one, |app| {
            app.debug_windows.netplay.chat.messages().count() == index + 1
        });
    }
    one.handle_netplay_action(&MenuAction::SendNesNetplayChat("one too many".into()));
    wait(&mut one, |app| {
        !app.debug_windows.netplay.chat.error.is_empty()
    });
    wait(&mut two, |app| {
        app.debug_windows.netplay.chat.messages().count() == 4
    });
    assert!(one.netplay.running() && two.netplay.running());
    assert_eq!(one.debug_windows.netplay.chat.messages().count(), 4);
    one.request_netplay_stop();
    wait(&mut one, |app| !app.netplay.fenced());
    wait(&mut two, |app| !app.netplay.fenced());
}

#[test]
fn disconnected_chat_does_not_touch_worker_state() {
    let directory = crate::test_support::test_directory("netplay-chat-disconnected").unwrap();
    let mut one = app(directory.path(), "one");
    let initial = capture(&mut one);
    one.handle_netplay_action(&MenuAction::SendNesNetplayChat("hello".into()));
    assert!(!one.debug_windows.netplay.chat.error.is_empty());
    assert_eq!(capture(&mut one), initial);
}
