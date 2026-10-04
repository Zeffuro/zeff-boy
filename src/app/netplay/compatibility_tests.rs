use super::tests::app;
use super::*;

#[test]
fn invalid_host_or_join_delay_does_not_reload_or_pause_the_game() {
    let directory = crate::test_support::test_directory("app-netplay-invalid-delay").unwrap();
    let mut app = app(directory.path(), "invalid");
    app.set_user_paused(false);
    for delay in [9, u64::MAX] {
        app.debug_windows.netplay.input_delay = delay;
        assert!(app.begin_netplay(None).is_err());
        let invitation = format!("127.0.0.1:8766/{}/{delay}", "ab".repeat(32));
        assert!(app.begin_netplay(Some(invitation)).is_err());
        assert!(app.netplay.phase == Phase::Idle);
        assert!(!app.speed.paused);
        assert!(!app.debug_windows.netplay.active);
    }
    app.stop_emu_thread();
}

#[test]
fn version_consent_is_captured_and_cleared_before_host_or_join_reload() {
    let directory = crate::test_support::test_directory("app-netplay-version-consent").unwrap();
    for invitation in [None, Some(format!("127.0.0.1:8766/{}", "ab".repeat(32)))] {
        let mut app = app(
            directory.path(),
            if invitation.is_none() { "host" } else { "join" },
        );
        app.debug_windows.netplay.allow_different_versions = true;
        app.begin_netplay(invitation).unwrap();
        assert!(!app.debug_windows.netplay.allow_different_versions);
        assert!(matches!(
            app.netplay.phase,
            Phase::Preparing(Request::Host(_, true))
                | Phase::Preparing(Request::Join {
                    allow_different_versions: true,
                    ..
                })
        ));
        app.request_netplay_stop();
        assert!(app.netplay.phase == Phase::Idle);
        app.stop_emu_thread();
    }
}
