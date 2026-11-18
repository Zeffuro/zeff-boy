use super::tests::{capture, connect_pair, wait};
use super::*;
use crate::app::tas_control::tests::harness::app_with_worker;
use crate::emu_backend::ActiveSystem;
use crate::emu_thread::EmuThread;
use crate::input::HostButton;
use zeff_md_core::{ControllerKind, Region, TimingModel};
use zeff_netplay::lockstep::Player;

fn app(kind: ControllerKind) -> (crate::test_support::TestDirectory, App) {
    app_with_timing(TimingModel::Standard, kind)
}

fn app_with_timing(
    timing: TimingModel,
    kind: ControllerKind,
) -> (crate::test_support::TestDirectory, App) {
    let (directory, backend) = crate::netplay::md_fixture::loaded(
        crate::netplay::md_fixture::config(Region::NtscUs, timing, kind),
    );
    let hash = backend.rom_hash();
    let path = directory.path().join("actual.md");
    let mut app = app_with_worker(EmuThread::spawn(backend, false), 11, ActiveSystem::Md, path);
    app.rom_info.rom_hash = Some(hash);
    app.debug_windows.netplay.input_delay = 0;
    app.settings.emulation.md_timing = match timing {
        TimingModel::Standard => crate::settings::MdTimingPreference::Standard,
        TimingModel::Precise => crate::settings::MdTimingPreference::Precise,
    };
    app.settings.emulation.md_controller = if kind == ControllerKind::ThreeButton {
        crate::settings::MdControllerPreference::ThreeButton
    } else {
        crate::settings::MdControllerPreference::SixButton
    };
    (directory, app)
}

fn hold(app: &mut App, buttons: u8, dpad: u8) {
    app.host_input.clear_keyboard();
    for button in HostButton::WITH_SIX_BUTTONS {
        let mask = button.host_mask_bit();
        app.host_input.set_keyboard(
            *button,
            (u16::from(buttons) << 4 | u16::from(dpad)) & mask != 0,
        );
        app.host_input.set_keyboard_p2(*button, true);
    }
}

fn wait_md_completion(app: &mut App, predicate: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(90);
    while !predicate(app) {
        app.drain_emu_responses();
        assert!(
            app.netplay.running(),
            "{}",
            app.debug_windows.netplay.status
        );
        assert!(Instant::now() < deadline, "MD completion deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn step(app: &mut App, frame: u64) {
    app.netplay.next_frame = None;
    app.pump_netplay();
    app.netplay.next_frame = Some(Instant::now() + Duration::from_secs(60));
    wait_md_completion(app, |app| !app.netplay.in_flight);
    assert!(
        app.netplay.running(),
        "{}",
        app.debug_windows.netplay.status
    );
    assert_eq!(app.netplay.presented, frame);
}

#[test]
fn md_app_uses_real_ready_configuration_and_local_player_one_controls_for_either_role() {
    for kind in [ControllerKind::ThreeButton, ControllerKind::SixButton] {
        let (_a_dir, mut one) = app(kind);
        let (_b_dir, mut two) = app(kind);
        let initial = connect_pair(&mut one, &mut two);
        assert_eq!(one.netplay.md_input.unwrap().player(), Player::One);
        assert_eq!(two.netplay.md_input.unwrap().player(), Player::Two);
        for app in [&mut one, &mut two] {
            assert_eq!(app.netplay.md_input.unwrap().kinds(), [kind; 2]);
            app.settings.emulation.md_controller = if kind == ControllerKind::ThreeButton {
                crate::settings::MdControllerPreference::SixButton
            } else {
                crate::settings::MdControllerPreference::ThreeButton
            };
        }
        for (index, (buttons, dpad)) in [(0x40, 0), (0xff, 0x0f), (0xb4, 0)].into_iter().enumerate()
        {
            hold(&mut one, buttons, dpad);
            hold(&mut two, buttons, dpad);
            step(&mut one, index as u64 + 1);
            step(&mut two, index as u64 + 1);
            for app in [&mut one, &mut two] {
                wait_md_completion(app, |app| app.netplay.confirmed == index as u64 + 1);
                let expected = (u16::from(buttons) | u16::from(dpad) << 8)
                    & if kind == ControllerKind::ThreeButton {
                        0x0f4b
                    } else {
                        0x0fff
                    };
                assert_eq!(app.netplay.observed_frames[index].1, [expected; 2]);
                assert_eq!(app.current_host_joypad_input(), (buttons, dpad));
                assert_eq!(app.current_host_joypad_p2_input(), (0xff, 0x0f));
                assert!(app.netplay.running());
            }
        }
        one.request_netplay_stop();
        wait(&mut one, |app| app.netplay.phase == Phase::Idle);
        wait(&mut two, |app| app.netplay.phase == Phase::Idle);
        assert!(one.netplay.md_input.is_none() && one.netplay.player.is_none());
        assert!(two.netplay.md_input.is_none() && two.netplay.player.is_none());
        assert_eq!(capture(&mut one), initial.0);
        assert_eq!(capture(&mut two), initial.1);
        one.stop_emu_thread();
        two.stop_emu_thread();
    }
}

#[test]
fn md_app_missing_or_foreign_ready_refuses_before_sampling_and_stopping_never_samples() {
    let (_a_dir, mut one) = app(ControllerKind::ThreeButton);
    let (_b_dir, mut two) = app(ControllerKind::ThreeButton);
    connect_pair(&mut one, &mut two);
    let profile = one.netplay.md_input.unwrap();
    for metadata in [None, Some(profile)] {
        let (_dir, mut rejected) = app(ControllerKind::ThreeButton);
        rejected.netplay.phase = Phase::Admission;
        rejected.netplay.player = Some(Player::Two);
        rejected
            .consume_netplay_response(EmuResponse::Netplay(Response::Ready { md_input: metadata }));
        assert!(rejected.netplay.phase == Phase::Stopping);
        assert!(!rejected.netplay.in_flight);
        assert!(rejected.netplay.md_input.is_none());
        rejected.stop_emu_thread();
    }
    one.request_netplay_stop();
    one.consume_netplay_response(EmuResponse::Netplay(Response::Ready {
        md_input: Some(profile),
    }));
    assert!(one.netplay.phase == Phase::Stopping);
    assert!(!one.netplay.in_flight);
    wait(&mut one, |app| app.netplay.phase == Phase::Idle);
    wait(&mut two, |app| app.netplay.phase == Phase::Idle);
    assert!(one.netplay.md_input.is_none() && two.netplay.md_input.is_none());
    one.stop_emu_thread();
    two.stop_emu_thread();
    for stopping in [false, true] {
        let (directory, worker, start, _peer) = super::md_failure_tests::worker_and_start(
            crate::netplay::session::md_start_failure::Stage::AbortVerification,
        );
        let mut failed = app_with_worker(
            worker,
            11,
            ActiveSystem::Md,
            directory.as_ref().unwrap().path().join("actual.md"),
        );
        failed.netplay.phase = Phase::Connecting;
        failed.netplay.player = Some(Player::One);
        failed
            .send_emu_command_checked(EmuCommand::StartNetplay(Box::new(start)))
            .unwrap();
        failed.netplay.phase = Phase::Admission;
        if stopping {
            failed.request_netplay_stop();
        }
        let response = super::md_failure_tests::next_response(failed.emu_thread.as_ref().unwrap());
        assert!(matches!(
            &response,
            Response::Stopped {
                restored: false,
                ..
            }
        ));
        failed.consume_netplay_response(EmuResponse::Netplay(response));
        assert!(failed.netplay.phase == Phase::Poisoned);
        assert!(failed.netplay.fenced());
        assert!(!failed.netplay.in_flight);
        assert!(failed.debug_windows.netplay.active);
        failed.emu_thread.as_ref().unwrap().send(EmuCommand::Reset);
        let refusal = super::md_failure_tests::next_response(failed.emu_thread.as_ref().unwrap());
        assert!(matches!(
            &refusal,
            Response::Stopped {
                restored: false,
                ..
            }
        ));
        failed.consume_netplay_response(EmuResponse::Netplay(refusal));
        assert!(failed.netplay.phase == Phase::Poisoned);
        assert!(failed.netplay.fenced());
        failed.stop_emu_thread();
    }
}

#[test]
fn md_normal_native_thread_pair_matches_six_literal_fields_and_independent_normal_control() {
    native_pair_matches_control(TimingModel::Standard, ControllerKind::ThreeButton);
    for kind in [ControllerKind::ThreeButton, ControllerKind::SixButton] {
        native_pair_matches_control(TimingModel::Precise, kind);
    }
}

fn native_pair_matches_control(timing: TimingModel, kind: ControllerKind) {
    let (_a_dir, mut one) = app_with_timing(timing, kind);
    let (_b_dir, mut two) = app_with_timing(timing, kind);
    let (_reference_dir, mut reference) = crate::netplay::md_fixture::loaded(
        crate::netplay::md_fixture::config(Region::NtscUs, timing, kind),
    );
    let config = crate::netplay::identity::identity_with_delay(
        &reference,
        crate::netplay::connect::executable_build().unwrap(),
        zeff_netplay::rollback::InputDelay::new(0).unwrap(),
    )
    .unwrap_or_else(|error| panic!("MD reference identity {timing:?}/{kind:?}: {error:#}"))
    .config;
    assert!(crate::netplay::identity::checkpoint(&reference, 1, &[], config).is_err());
    assert!(crate::netplay::identity::checkpoint(&reference, u64::MAX, &[], config).is_err());
    let initial = connect_pair(&mut one, &mut two);
    let baseline = reference.encode_state_bytes().unwrap();
    assert!(
        initial.0 == baseline && initial.1 == baseline,
        "MD host prepared or guest pre-reload baseline differs from {timing:?}/{kind:?} reference"
    );
    let (first, second) = crate::netplay::md_fixture::words(kind);
    for index in 0..6 {
        hold(&mut one, first[index] as u8, (first[index] >> 8) as u8);
        hold(&mut two, second[index] as u8, (second[index] >> 8) as u8);
        step(&mut one, index as u64 + 1);
        assert_eq!(one.netplay.confirmed, index as u64);
        assert_eq!(one.netplay.observed_frames.len(), index);
        step(&mut two, index as u64 + 1);
        for app in [&mut one, &mut two] {
            wait_md_completion(app, |app| app.netplay.confirmed == index as u64 + 1);
            assert_eq!(app.netplay.observed_frames.len(), index + 1);
        }
        reference.set_input(first[index] as u8, (first[index] >> 8) as u8);
        reference.set_input_p2(second[index] as u8, (second[index] >> 8) as u8);
        reference.step_frame();
        let mut audio = Vec::new();
        reference.drain_audio_samples_into(&mut audio);
        assert!(audio.iter().any(|sample| *sample != 0.0));
        let checkpoint =
            crate::netplay::identity::checkpoint(&reference, index as u64 + 1, &audio, config)
                .unwrap();
        assert_eq!(
            &crate::netplay::md_fixture::md(&reference).core().work_ram()[0x100..0x104],
            &crate::netplay::md_fixture::PINS[index % 2]
        );
        if kind == ControllerKind::SixButton {
            assert_eq!(
                &crate::netplay::md_fixture::md(&reference).core().work_ram()[0x104..0x108],
                &crate::netplay::md_fixture::EXTRA_PINS[index % 2]
            );
        }
        for app in [&one, &two] {
            let (actual, ports, pcm) = &app.netplay.observed_frames[index];
            assert_eq!(*actual, checkpoint);
            assert_eq!(*ports, [first[index], second[index]]);
            assert_eq!(
                crate::netplay::md_fixture::bits(pcm),
                crate::netplay::md_fixture::bits(&audio)
            );
        }
    }
    assert!(one.netplay.rollback_frames > 0);
    one.request_netplay_stop();
    wait(&mut one, |app| app.netplay.phase == Phase::Idle);
    wait(&mut two, |app| app.netplay.phase == Phase::Idle);
    assert_eq!(capture(&mut one), initial.0);
    assert_eq!(capture(&mut two), initial.1);
    one.stop_emu_thread();
    two.stop_emu_thread();
}
