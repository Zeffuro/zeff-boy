use super::tests::{app_with_timing, audio_bits, capture, connect_pair, load, set_input, wait};
use super::*;
use crate::emu_backend::EmuBackend;
use crate::netplay::{connect::executable_build, identity};
use zeff_nes_core::hardware::cartridge::TimingMode;

fn step(app: &mut App, buttons: u8, frame: u64) {
    set_input(app, buttons);
    app.netplay.next_frame = None;
    app.pump_netplay();
    app.netplay.next_frame = Some(Instant::now() + Duration::from_secs(60));
    wait(app, |app| !app.netplay.in_flight);
    assert!(
        app.netplay.running(),
        "{}",
        app.debug_windows.netplay.status
    );
    assert_eq!(app.netplay.presented, frame);
}

#[test]
fn low_delay_applies_a_local_pulse_at_its_selected_frame_for_both_players() {
    for timing in [TimingMode::Ntsc, TimingMode::Pal, TimingMode::Dendy] {
        for delay in [0, 1, 2] {
            let directory = crate::test_support::test_directory("app-low-delay-pulse").unwrap();
            let mut one = app_with_timing(directory.path(), "one", timing);
            let mut two = app_with_timing(directory.path(), "two", timing);
            one.debug_windows.netplay.input_delay = delay;
            let initial = connect_pair(&mut one, &mut two);
            for frame in 0..delay + 2 {
                step(&mut one, if frame == 0 { 1 } else { 0 }, frame + 1);
                step(&mut two, if frame == 0 { 0x80 } else { 0 }, frame + 1);
            }
            for app in [&mut one, &mut two] {
                wait(app, |app| app.netplay.confirmed == delay + 2);
                let observed: Vec<_> = app.netplay.observed_frames.iter().map(|x| x.1).collect();
                let mut expected = vec![[0; 2]; delay as usize];
                expected.extend([[1, 0x80], [0, 0]]);
                assert_eq!(observed, expected);
            }
            one.request_netplay_stop();
            wait(&mut one, |app| app.netplay.phase == Phase::Idle);
            wait(&mut two, |app| app.netplay.phase == Phase::Idle);
            assert_eq!(capture(&mut one), initial.0);
            assert_eq!(capture(&mut two), initial.1);
            one.stop_emu_thread();
            two.stop_emu_thread();
        }
    }
}

#[test]
fn late_changing_inputs_correct_display_and_pcm_without_waiting_for_peer_steps() {
    for timing in [TimingMode::Ntsc, TimingMode::Pal, TimingMode::Dendy] {
        for delay in
            zeff_netplay::rollback::InputDelay::MIN..=zeff_netplay::rollback::InputDelay::MAX
        {
            let directory = crate::test_support::test_directory("app-rollback-late-input").unwrap();
            let mut one = app_with_timing(directory.path(), "one", timing);
            let mut two = app_with_timing(directory.path(), "two", timing);
            one.debug_windows.netplay.input_delay = delay;
            let initial = connect_pair(&mut one, &mut two);
            let mut reference = load(one.rom_info.rom_path.as_ref().unwrap());
            let config = identity::identity_with_delay(
                &reference,
                executable_build().unwrap(),
                zeff_netplay::rollback::InputDelay::new(delay).unwrap(),
            )
            .unwrap()
            .config;
            let inputs: Vec<[u8; 2]> = (0..delay + 6)
                .map(|frame| [1 << (frame % 4), 0x80 >> (frame % 4)])
                .collect();
            for (frame, buttons) in inputs.iter().enumerate() {
                step(&mut one, buttons[0], frame as u64 + 1);
            }
            assert_eq!(one.netplay.confirmed, delay);
            assert_eq!(one.netplay.observed_frames.len(), delay as usize);
            assert_eq!(one.netplay.audio_started, delay >= 3);
            assert_eq!(two.netplay.presented, 0);
            for (frame, buttons) in inputs.iter().enumerate() {
                step(&mut two, buttons[1], frame as u64 + 1);
            }
            for app in [&mut one, &mut two] {
                wait(app, |app| {
                    app.netplay.confirmed == delay + 6
                        && app.netplay.published_confirmed >= delay + 6
                });
            }
            assert!(one.netplay.rollback_frames > 0);
            for frame in 0..(delay + 6) as usize {
                let ports = if frame < delay as usize {
                    [0, 0]
                } else {
                    inputs[frame - delay as usize]
                };
                let EmuBackend::Nes(nes) = &mut reference else {
                    unreachable!()
                };
                nes.emu.set_input_p1_raw(ports[0]);
                nes.emu.set_input_p2_raw(ports[1]);
                reference.step_frame();
                let mut audio = Vec::new();
                reference.drain_audio_samples_into(&mut audio);
                let expected =
                    identity::checkpoint(&reference, frame as u64 + 1, &audio, config).unwrap();
                for app in [&one, &two] {
                    let (checkpoint, used, pcm) = &app.netplay.observed_frames[frame];
                    assert_eq!(*checkpoint, expected);
                    assert_eq!(*used, ports);
                    assert_eq!(audio_bits(pcm), audio_bits(&audio));
                }
            }
            for app in [&one, &two] {
                assert_eq!(
                    app.latest_frame.as_ref().unwrap().as_slice(),
                    reference.framebuffer()
                );
                assert!(app.netplay.audio_started);
            }
            for (app, initial) in [(&mut one, initial.0), (&mut two, initial.1)] {
                app.request_netplay_stop();
                wait(app, |app| app.netplay.phase == Phase::Idle);
                assert_eq!(capture(app), initial);
                app.stop_emu_thread();
            }
        }
    }
}
