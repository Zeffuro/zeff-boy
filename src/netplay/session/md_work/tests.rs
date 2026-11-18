use super::super::md_tests::{loaded, ready, start, transports};
use super::*;
use crate::netplay::md_fixture::{self as fixture, Baseline};

fn deliver_input(
    a: &mut Session,
    b: &mut Session,
    two: &mut EmuBackend,
    remote: Player,
    word: u16,
) {
    b.network
        .send(Message::Input {
            player: remote,
            frame: 0,
            buttons: word,
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(event) = a.network.poll().unwrap() {
            let arrived = matches!(&event, Event::Message(Message::Input { player, frame: 0, buttons })
                if *player == remote && *buttons == word);
            a.receive_md_event(event).unwrap();
            if arrived {
                return;
            }
        }
        b.poll(two).unwrap();
        assert!(Instant::now() < deadline);
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::yield_now();
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_authenticated_input_during_and_after_pending_work_publishes_corrected_frames_once() {
    for local in [Player::One, Player::Two] {
        let remote = if local == Player::One {
            Player::Two
        } else {
            Player::One
        };
        for late in [false, true] {
            for remote_word in [0, 0x080a] {
                let (_a_dir, mut one) = loaded();
                let (_b_dir, mut two) = loaded();
                let (_reference_dir, mut reference) = loaded();
                let baseline_one = Baseline::capture(&one);
                let baseline_two = Baseline::capture(&two);
                let (left, right) = transports();
                let mut a = Session::start(&mut one, start(left, local)).unwrap();
                let mut b = Session::start(&mut two, start(right, remote)).unwrap();
                ready(&mut a, &mut b, &mut one, &mut two);
                let before = one.encode_state_bytes().unwrap();
                let snapshots = a.snapshots.keys().copied().collect::<Vec<_>>();
                a.step(&mut one, 0x0101).unwrap();
                assert!(a.poll(&mut one).unwrap().is_none());
                assert!(matches!(
                    a.md_work.as_ref().unwrap().transaction,
                    Some(Transaction::Normal(_))
                ));
                assert_eq!(one.encode_state_bytes().unwrap(), before);
                assert_eq!(a.timeline.frame(), 0);
                assert_eq!(a.snapshots.keys().copied().collect::<Vec<_>>(), snapshots);
                assert!(a.outputs.is_empty());
                let mut step_completions = 0;
                if late {
                    let deadline = Instant::now() + Duration::from_secs(5);
                    while step_completions == 0 {
                        if let Some(response) = a.poll(&mut one).unwrap() {
                            match response {
                                Response::Presented {
                                    frame,
                                    confirmed,
                                    step_complete: true,
                                    ..
                                } => {
                                    assert_eq!((frame, confirmed), (1, 0));
                                    step_completions += 1;
                                }
                                Response::Frame { .. } => panic!("unconfirmed frame escaped"),
                                _ => {}
                            }
                        }
                        b.poll(&mut two).unwrap();
                        assert!(Instant::now() < deadline);
                    }
                }
                deliver_input(&mut a, &mut b, &mut two, remote, remote_word);
                if !late {
                    assert_eq!(a.timeline.frame(), 0);
                    assert_eq!(a.timeline.confirmed_frame(), 0);
                    assert_eq!(a.md_work.as_ref().unwrap().deferred.len(), 1);
                    assert!(a.md_work.as_ref().unwrap().payload_bytes().unwrap() > 0);
                }
                let mut confirmed_frame = None;
                let mut last_confirmed = 0;
                let mut confirmation_only = false;
                let deadline = Instant::now() + Duration::from_secs(5);
                while confirmed_frame.is_none() || last_confirmed != 1 {
                    if let Some(response) = a.poll(&mut one).unwrap() {
                        match response {
                            Response::Frame {
                                checkpoint,
                                ports,
                                audio,
                            } => {
                                assert!(confirmed_frame.is_none(), "duplicate confirmed frame");
                                confirmed_frame = Some((checkpoint, ports, audio));
                            }
                            Response::Presented {
                                frame,
                                confirmed,
                                changed,
                                step_complete,
                                rollback_frames,
                                ..
                            } => {
                                assert_eq!(frame, 1);
                                last_confirmed = confirmed;
                                step_completions += usize::from(step_complete);
                                confirmation_only |= !step_complete
                                    && !changed
                                    && rollback_frames == 0
                                    && confirmed == 1;
                            }
                            _ => {}
                        }
                    }
                    b.poll(&mut two).unwrap();
                    assert!(Instant::now() < deadline);
                }
                assert_eq!(step_completions, 1);
                if late && remote_word == 0 {
                    assert!(confirmation_only)
                }
                let work = a.md_work.as_ref().unwrap();
                assert!(
                    work.step.is_none() && work.transaction.is_none() && work.deferred.is_empty()
                );
                let actual = one.encode_state_bytes().unwrap();
                let config = a.config;
                a.restore(&mut one).unwrap();
                b.restore(&mut two).unwrap();
                baseline_one.verify(&one);
                baseline_two.verify(&two);
                let ports = if local == Player::One {
                    [0x0101, remote_word]
                } else {
                    [remote_word, 0x0101]
                };
                reference.set_input(ports[0] as u8, (ports[0] >> 8) as u8);
                reference.set_input_p2(ports[1] as u8, (ports[1] >> 8) as u8);
                reference.step_frame();
                let mut expected = Vec::new();
                reference.drain_audio_samples_into(&mut expected);
                assert!(expected.iter().any(|sample| *sample != 0.0));
                let (checkpoint, actual_ports, audio) = confirmed_frame.unwrap();
                assert_eq!(actual_ports, ports);
                assert_eq!(
                    checkpoint,
                    identity::checkpoint(&reference, 1, &expected, config).unwrap()
                );
                assert!(fixture::bits(&audio) == fixture::bits(&expected));
                assert!(actual == reference.encode_state_bytes().unwrap());
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_stop_during_normal_or_correction_pending_work_returns_original_without_partial_output() {
    for local in [Player::One, Player::Two] {
        let remote = if local == Player::One {
            Player::Two
        } else {
            Player::One
        };
        for correction in [false, true] {
            let (_a_dir, mut one) = loaded();
            let (_b_dir, mut two) = loaded();
            let baseline_one = Baseline::capture(&one);
            let baseline_two = Baseline::capture(&two);
            let (left, right) = transports();
            let mut a = Session::start(&mut one, start(left, local)).unwrap();
            let mut b = Session::start(&mut two, start(right, remote)).unwrap();
            ready(&mut a, &mut b, &mut one, &mut two);
            a.step(&mut one, 0x0101).unwrap();
            if correction {
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    let done = matches!(
                        a.poll(&mut one).unwrap(),
                        Some(Response::Presented {
                            step_complete: true,
                            ..
                        })
                    );
                    b.poll(&mut two).unwrap();
                    if done {
                        break;
                    }
                    assert!(Instant::now() < deadline);
                }
                deliver_input(&mut a, &mut b, &mut two, remote, 0x080a);
            }
            let response = a.poll(&mut one).unwrap();
            assert!(!matches!(
                response,
                Some(Response::Frame { .. } | Response::Presented { .. })
            ));
            let transaction = &a.md_work.as_ref().unwrap().transaction;
            if correction {
                assert!(matches!(transaction, Some(Transaction::Correction { .. })));
            } else {
                assert!(matches!(transaction, Some(Transaction::Normal(_))));
            }
            assert_eq!(fixture::md(&one).core().pacing_field_count(), 0);
            assert!(fixture::md(&one).rollback_owned());
            assert!(!fixture::md(&one).host_persistence_enabled());
            a.restore(&mut one).unwrap();
            b.restore(&mut two).unwrap();
            baseline_one.verify(&one);
            baseline_two.verify(&two);
        }
    }
}
