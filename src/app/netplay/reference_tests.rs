use super::tests::{app_with_timing, audio_bits, capture, connect_pair, load, set_input, wait};
use super::*;
use crate::emu_backend::EmuBackend;
use crate::netplay::{connect::executable_build, identity};
use zeff_nes_core::hardware::cartridge::TimingMode;

#[test]
fn two_apps_schedule_owned_delayed_input_confirmed_frame_audio_and_exact_stop() {
    delayed_reference(TimingMode::Ntsc);
}

#[test]
fn pal_two_apps_schedule_owned_delayed_input_confirmed_frame_audio_and_exact_stop() {
    delayed_reference(TimingMode::Pal);
}

#[test]
fn dendy_two_apps_schedule_owned_delayed_input_confirmed_frame_audio_and_exact_stop() {
    delayed_reference(TimingMode::Dendy);
}

fn delayed_reference(timing: TimingMode) {
    let directory = crate::test_support::test_directory("app-netplay-pair").unwrap();
    let mut one = app_with_timing(directory.path(), "one", timing);
    let mut two = app_with_timing(directory.path(), "two", timing);
    let (initial_one, initial_two) = connect_pair(&mut one, &mut two);
    let paths = [
        one.rom_info.rom_path.clone().unwrap(),
        two.rom_info.rom_path.clone().unwrap(),
    ];
    let baseline = paths
        .each_ref()
        .map(|path| std::fs::read(path.with_extension("sav")).unwrap());
    let mut reference = load(&paths[0]);
    let emu = &reference.nes().unwrap().emu;
    assert_eq!(emu.resolved_timing_mode(), timing);
    for app in [&one, &two] {
        assert_eq!(
            app.nominal_frame_duration_ns(),
            emu.nominal_frame_duration_ns()
        );
    }
    if timing != TimingMode::Ntsc {
        assert!(emu.nominal_frame_duration_ns() > 19_000_000);
    }
    assert_eq!(reference.encode_state_bytes().unwrap(), initial_one);
    assert_eq!(initial_one, initial_two);
    let config = identity::identity(&reference, executable_build().unwrap())
        .unwrap()
        .config;
    let mut scheduled = Vec::new();
    let mut prebuffer = Vec::new();
    for frame in 0..24_u64 {
        let raw = [
            ((frame * 17 + 3) ^ (frame >> 2)) as u8,
            ((frame * 29 + 11) ^ (frame >> 1)) as u8,
        ];
        set_input(&mut one, raw[0]);
        set_input(&mut two, raw[1]);
        one.game_window_focused = !(5..8).contains(&frame);
        two.game_view_focused = !(9..12).contains(&frame);
        two.egui_wants_keyboard = (12..15).contains(&frame);
        scheduled.push([
            if one.game_window_focused { raw[0] } else { 0 },
            if two.game_view_focused && !two.egui_wants_keyboard {
                raw[1]
            } else {
                0
            },
        ]);
        one.netplay.next_frame = None;
        two.netplay.next_frame = None;
        one.pump_netplay();
        assert!(one.netplay.in_flight);
        one.pump_netplay();
        wait(&mut one, |app| {
            app.netplay.presented == frame + 1 && !app.netplay.in_flight
        });
        assert_eq!(one.netplay.presented, frame + 1);
        assert!(!one.netplay.in_flight);
        two.pump_netplay();
        two.pump_netplay();
        for app in [&mut one, &mut two] {
            app.netplay.next_frame = Some(Instant::now() + Duration::from_secs(60));
            app.tick();
            wait(app, |app| {
                app.netplay.confirmed == frame + 1
                    && app.netplay.published_confirmed > frame
                    && !app.netplay.in_flight
            });
            assert!(app.netplay.running());
            assert!(!app.netplay.in_flight);
            assert_eq!(app.frames_in_flight, 0);
            assert_eq!(app.netplay.observed_frames.len(), frame as usize + 1);
            assert!(app.emu_thread.as_ref().unwrap().try_recv_frame().is_none());
        }
        let ports = if frame < 2 {
            [0, 0]
        } else {
            scheduled[frame as usize - 2]
        };
        let EmuBackend::Nes(nes) = &mut reference else {
            unreachable!()
        };
        nes.emu.set_input_p1_raw(ports[0]);
        nes.emu.set_input_p2_raw(ports[1]);
        reference.step_frame();
        let mut expected_audio = Vec::new();
        reference.drain_audio_samples_into(&mut expected_audio);
        let expected =
            identity::checkpoint(&reference, frame + 1, &expected_audio, config).unwrap();
        if frame < 3 {
            prebuffer.extend_from_slice(&expected_audio);
        }
        for app in [&one, &two] {
            let (checkpoint, actual_ports, audio) = app.netplay.observed_frames.last().unwrap();
            assert_eq!(checkpoint, &expected, "checkpoint frame {frame}");
            assert_eq!(*actual_ports, ports, "owned input frame {frame}");
            assert_eq!(audio_bits(audio), audio_bits(&expected_audio));
            if frame < 2 {
                assert!(app.netplay.queued_audio.is_none());
            } else {
                let (queued, speed) = app.netplay.queued_audio.as_ref().unwrap();
                assert_eq!(*speed, 1);
                assert_eq!(
                    audio_bits(queued),
                    audio_bits(if frame == 2 {
                        &prebuffer
                    } else {
                        &expected_audio
                    })
                );
            }
            assert_eq!(
                app.latest_frame
                    .as_ref()
                    .or(app.last_core_frame.as_ref())
                    .unwrap()
                    .as_slice(),
                reference.framebuffer()
            );
        }
        for (path, expected) in paths.iter().zip(&baseline) {
            assert_eq!(
                &std::fs::read(path.with_extension("sav")).unwrap(),
                expected
            );
        }
    }
    assert_ne!(
        reference.nes().unwrap().emu.dump_persistent_data().unwrap(),
        baseline[0]
    );
    one.request_netplay_stop();
    two.request_netplay_stop();
    for (app, initial) in [(&mut one, &initial_one), (&mut two, &initial_two)] {
        wait(app, |app| app.netplay.phase == Phase::Idle);
        assert!(app.speed.paused);
        assert!(!app.netplay.fenced());
        assert_eq!(capture(app), *initial);
        app.stop_emu_thread();
    }
    for (path, expected) in paths.iter().zip(&baseline) {
        assert_eq!(
            &std::fs::read(path.with_extension("sav")).unwrap(),
            expected
        );
    }
}
