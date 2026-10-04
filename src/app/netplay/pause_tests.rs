use super::tests::{app, app_with_timing, capture, connect_pair, set_input, wait};
use super::*;
use zeff_nes_core::hardware::cartridge::TimingMode;

fn hold_schedule(app: &mut App) {
    app.netplay.next_frame = Some(Instant::now() + Duration::from_secs(60));
}

fn round(one: &mut App, two: &mut App) {
    for app in [&mut *one, &mut *two] {
        app.netplay.next_frame = None;
        app.pump_netplay();
        assert!(app.netplay.in_flight);
        hold_schedule(app);
    }
    for app in [one, two] {
        wait(app, |app| !app.netplay.in_flight);
        hold_schedule(app);
        assert!(
            app.netplay.running(),
            "{}",
            app.debug_windows.netplay.status
        );
    }
}

fn stop_pair(one: &mut App, two: &mut App, initial: &[Vec<u8>; 2]) {
    one.request_netplay_stop();
    two.request_netplay_stop();
    for (app, expected) in [(one, &initial[0]), (two, &initial[1])] {
        wait(app, |app| app.netplay.phase == Phase::Idle);
        assert!(app.speed.paused);
        assert!(!app.netplay.fenced());
        assert_eq!(&capture(app), expected);
        app.stop_emu_thread();
    }
}

#[test]
fn either_player_pauses_without_publication_and_resume_preserves_delayed_reference() {
    pause(TimingMode::Ntsc);
}

#[test]
fn pal_either_player_pauses_without_publication_and_resume_preserves_delayed_reference() {
    pause(TimingMode::Pal);
}

#[test]
fn dendy_either_player_pauses_without_publication_and_resume_preserves_delayed_reference() {
    pause(TimingMode::Dendy);
}

fn pause(timing: TimingMode) {
    let directory = crate::test_support::test_directory("app-rollback-pause").unwrap();
    let mut one = app_with_timing(directory.path(), "one", timing);
    let mut two = app_with_timing(directory.path(), "two", timing);
    let (a, b) = connect_pair(&mut one, &mut two);
    let initial = [a, b];
    for frame in 0..4 {
        set_input(&mut one, frame + 1);
        set_input(&mut two, 0x80 >> frame);
        round(&mut one, &mut two);
    }
    one.set_netplay_paused(true);
    assert!(!one.netplay.paused);
    for _ in 0..12 {
        round(&mut one, &mut two);
    }
    for app in [&mut one, &mut two] {
        wait(app, |app| app.netplay.paused);
    }
    assert_eq!(one.netplay.presented, 16);
    assert_eq!(two.netplay.presented, 16);
    let pictures = [
        one.latest_frame.clone().unwrap(),
        two.latest_frame.clone().unwrap(),
    ];
    let counts = [
        one.netplay.observed_frames.len(),
        two.netplay.observed_frames.len(),
    ];
    let audio = [
        one.netplay.queued_audio.clone(),
        two.netplay.queued_audio.clone(),
    ];
    two.set_netplay_paused(true);
    round(&mut one, &mut two);
    one.set_netplay_paused(false);
    for _ in 0..3 {
        round(&mut one, &mut two);
    }
    for (index, app) in [&one, &two].into_iter().enumerate() {
        assert!(app.netplay.paused);
        assert_eq!(app.netplay.presented, 16);
        assert_eq!(app.netplay.observed_frames.len(), counts[index]);
        assert!(std::sync::Arc::ptr_eq(
            &pictures[index],
            app.latest_frame.as_ref().unwrap()
        ));
        assert_eq!(app.netplay.queued_audio, audio[index]);
        assert!(!app.netplay.permits(&EmuCommand::Reset));
        assert!(!app.netplay.permits(&EmuCommand::SetSampleRate(44_100)));
    }
    two.set_netplay_paused(false);
    for app in [&mut one, &mut two] {
        wait(app, |app| !app.netplay.paused);
    }
    round(&mut one, &mut two);
    for app in [&mut one, &mut two] {
        wait(app, |app| !app.netplay.paused);
        assert_eq!(app.netplay.presented, 17);
        assert!(app.netplay.fenced());
    }
    stop_pair(&mut one, &mut two, &initial);
}

#[test]
fn both_requests_need_both_releases_and_pause_keeps_command_and_readiness_fences() {
    let directory = crate::test_support::test_directory("app-rollback-both-pause").unwrap();
    let mut one = app(directory.path(), "one");
    let mut two = app(directory.path(), "two");
    let (a, b) = connect_pair(&mut one, &mut two);
    let initial = [a, b];
    one.set_netplay_paused(true);
    two.set_netplay_paused(true);
    for _ in 0..12 {
        round(&mut one, &mut two);
    }
    for app in [&mut one, &mut two] {
        wait(app, |app| app.netplay.paused);
    }
    one.set_netplay_paused(false);
    round(&mut one, &mut two);
    for app in [&one, &two] {
        assert_eq!(app.netplay.presented, 12);
        assert!(app.netplay.paused);
    }
    two.set_netplay_paused(false);
    for app in [&mut one, &mut two] {
        wait(app, |app| !app.netplay.paused);
    }
    round(&mut one, &mut two);
    for app in [&mut one, &mut two] {
        wait(app, |app| !app.netplay.paused);
        assert_eq!(app.netplay.presented, 13);
    }
    stop_pair(&mut one, &mut two, &initial);
}

#[test]
fn cancel_completed_or_pending_pause_restores_both_apps_and_saves_exactly() {
    for completed in [false, true] {
        let directory = crate::test_support::test_directory("app-rollback-stop-pause").unwrap();
        let mut one = app(directory.path(), "one");
        let mut two = app(directory.path(), "two");
        let (a, b) = connect_pair(&mut one, &mut two);
        let initial = [a, b];
        let paths = [
            one.rom_info.rom_path.clone().unwrap(),
            two.rom_info.rom_path.clone().unwrap(),
        ];
        let saves = paths
            .each_ref()
            .map(|path| std::fs::read(path.with_extension("sav")).unwrap());
        one.set_netplay_paused(true);
        if completed {
            for _ in 0..12 {
                round(&mut one, &mut two);
            }
            wait(&mut one, |app| app.netplay.paused);
        }
        stop_pair(&mut one, &mut two, &initial);
        for (path, save) in paths.iter().zip(saves) {
            assert_eq!(std::fs::read(path.with_extension("sav")).unwrap(), save);
        }
    }
}

#[test]
fn late_pause_request_allows_only_the_sent_round_and_teardown_interrupts_next_exchange() {
    let directory = crate::test_support::test_directory("app-rollback-late-pause").unwrap();
    let mut one = app(directory.path(), "one");
    let mut two = app(directory.path(), "two");
    let (a, b) = connect_pair(&mut one, &mut two);
    let initial = [a, b];
    one.netplay.next_frame = None;
    one.pump_netplay();
    hold_schedule(&mut one);
    one.set_netplay_paused(true);
    wait(&mut one, |app| !app.netplay.in_flight);
    assert_eq!(one.netplay.presented, 1);
    assert!(!one.netplay.paused);
    stop_pair(&mut one, &mut two, &initial);
}
