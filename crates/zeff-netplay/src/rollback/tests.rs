use super::*;
use crate::lockstep::INPUT_DELAY;

#[test]
fn input_delay_is_bounded_and_default_is_compatible() {
    assert_eq!(
        (InputDelay::MIN, InputDelay::MAX, InputDelay::DEFAULT),
        (0, 8, 2)
    );
    assert_eq!(InputDelay::default().frames(), INPUT_DELAY);
    assert_eq!(
        Timeline::new(Player::One).input_delay(),
        InputDelay::default()
    );
    for frames in InputDelay::MIN..=InputDelay::MAX {
        let delay = InputDelay::new(frames).unwrap();
        assert_eq!(delay.frames(), frames);
        assert_eq!(delay.pause_lead(), PREDICTION_WINDOW + 2 * frames);
        assert_eq!(delay.lookahead(), delay.pause_lead() + 1);
    }
    for frames in [9, 255, u64::MAX] {
        assert!(InputDelay::new(frames).is_err());
    }
}

#[test]
fn local_buttons_reach_the_first_simulated_frame_at_zero_delay() {
    for player in [Player::One, Player::Two] {
        for frames in 0..=2 {
            let mut timeline = Timeline::with_delay(player, InputDelay::new(frames).unwrap());
            for frame in 0..=frames {
                let buttons = 17 + frame as u8;
                assert_eq!(
                    timeline.sample_local(buttons).unwrap(),
                    (frame + frames, buttons)
                );
                assert_eq!(
                    timeline.sample_local(255).unwrap(),
                    (frame + frames, buttons)
                );
                let input = timeline.next_frame().unwrap().unwrap();
                let mut expected = [0; 2];
                if frame == frames {
                    expected[port(player)] = 17;
                }
                assert_eq!(
                    input,
                    FrameInput {
                        frame,
                        ports: expected
                    }
                );
                timeline.advance(input).unwrap();
            }
        }
    }
}

#[test]
fn low_delay_corrects_late_remote_changes_without_rewriting_owned_inputs() {
    for player in [Player::One, Player::Two] {
        for frames in 0..=1 {
            let delay = InputDelay::new(frames).unwrap();
            let mut timeline = Timeline::with_delay(player, delay);
            let end = frames + PREDICTION_WINDOW;
            for frame in 0..end {
                timeline.sample_local(17 + frame as u8).unwrap();
                timeline
                    .advance(timeline.next_frame().unwrap().unwrap())
                    .unwrap();
            }
            assert!(timeline.next_frame().unwrap().is_none());
            assert!(timeline.receive_remote(frames + 3, 41).unwrap());
            assert!(timeline.receive_remote(frames, 23).unwrap());
            assert!(timeline.next_frame().is_err());
            let replay = timeline.correction().unwrap();
            assert_eq!(replay.len(), PREDICTION_WINDOW as usize);
            for input in &replay {
                assert_eq!(input.ports[port(player)], 17 + (input.frame - frames) as u8);
                assert_eq!(
                    input.ports[1 - port(player)],
                    if input.frame < frames + 3 { 23 } else { 41 }
                );
            }
            let retained = timeline.retained_inputs();
            assert!(!timeline.receive_remote(frames, 23).unwrap());
            assert!(timeline.receive_remote(frames, 24).is_err());
            assert!(
                timeline
                    .receive_remote(end + delay.lookahead() + 1, 0)
                    .is_err()
            );
            assert_eq!(timeline.retained_inputs(), retained);
            assert_eq!(timeline.correction().unwrap(), replay);
            assert!(timeline.corrected(&replay[1..]).is_err());
            assert_eq!(timeline.correction().unwrap(), replay);
            timeline.corrected(&replay).unwrap();
            assert_eq!(timeline.confirmed_frame(), frames + 1);
            assert_eq!(timeline.prediction_depth(), PREDICTION_WINDOW - 1);
            let input = timeline.next_frame().unwrap().unwrap();
            assert_eq!(input.ports[1 - port(player)], 41);
            timeline.sample_local(99).unwrap();
            assert_eq!(timeline.sample_local(100).unwrap(), (end + frames, 99));
        }
    }
}

#[test]
fn every_delay_primes_neutral_frames_and_preserves_prediction_and_lookahead_bounds() {
    for player in [Player::One, Player::Two] {
        for frames in InputDelay::MIN..=InputDelay::MAX {
            let delay = InputDelay::new(frames).unwrap();
            let mut timeline = Timeline::with_delay(player, delay);
            assert_eq!(timeline.retained_inputs(), frames as usize);
            for frame in 0..frames + PREDICTION_WINDOW {
                assert_eq!(timeline.sample_local(17).unwrap(), (frame + frames, 17));
                let input = timeline.next_frame().unwrap().unwrap();
                let mut expected = [0; 2];
                if frame >= frames {
                    expected[port(player)] = 17;
                }
                assert_eq!(input.ports, expected);
                timeline.advance(input).unwrap();
            }
            assert!(timeline.next_frame().unwrap().is_none());
            assert_eq!(timeline.prediction_depth(), PREDICTION_WINDOW);
            let mut slow = Timeline::with_delay(player, delay);
            assert!(slow.receive_remote(delay.lookahead(), 19).unwrap());
            let retained = slow.retained_inputs();
            assert!(slow.receive_remote(delay.lookahead() + 1, 19).is_err());
            assert_eq!(slow.retained_inputs(), retained);
            timeline.receive_remote(frames, 1).unwrap();
            let replay = timeline.correction().unwrap();
            assert_eq!(replay.len(), PREDICTION_WINDOW as usize);
            assert!(
                replay
                    .iter()
                    .all(|input| input.ports[port(player)] == 17
                        && input.ports[1 - port(player)] == 1)
            );
            timeline.corrected(&replay).unwrap();
            assert_eq!(timeline.prediction_depth(), PREDICTION_WINDOW - 1);
        }
    }
}

#[test]
fn inputs_stream_ahead_and_missing_inputs_are_predicted_with_a_bound() {
    let mut timeline = Timeline::new(Player::One);
    for frame in 0..INPUT_DELAY + PREDICTION_WINDOW {
        let scheduled = timeline.sample_local(frame as u8).unwrap();
        assert_eq!(scheduled.0, frame + INPUT_DELAY);
        let input = timeline.next_frame().unwrap().unwrap();
        assert_eq!(input.ports[1], 0);
        timeline.advance(input).unwrap();
    }
    assert!(timeline.next_frame().unwrap().is_none());
    assert_eq!(timeline.prediction_depth(), PREDICTION_WINDOW);
    assert_eq!(
        timeline.sample_local(33).unwrap(),
        timeline.sample_local(99).unwrap()
    );
}

#[test]
fn late_changed_input_replays_following_predictions_and_later_known_values() {
    let mut timeline = Timeline::new(Player::One);
    for _ in 0..7 {
        timeline.sample_local(1).unwrap();
        timeline
            .advance(timeline.next_frame().unwrap().unwrap())
            .unwrap();
    }
    timeline.receive_remote(5, 17).unwrap();
    timeline.receive_remote(3, 9).unwrap();
    let replay = timeline.correction().unwrap();
    assert_eq!(
        replay
            .iter()
            .map(|input| (input.frame, input.ports[1]))
            .collect::<Vec<_>>(),
        vec![(3, 9), (4, 9), (5, 17), (6, 17)]
    );
    assert!(timeline.next_frame().is_err());
    timeline.corrected(&replay).unwrap();
    assert_eq!(timeline.frame(), 7);
    assert_eq!(timeline.next_frame().unwrap().unwrap().ports[1], 17);
}

#[test]
fn confirmation_waits_for_contiguous_owned_inputs_not_packets_or_checksums() {
    let mut timeline = Timeline::new(Player::Two);
    for _ in 0..5 {
        timeline.sample_local(23).unwrap();
        timeline
            .advance(timeline.next_frame().unwrap().unwrap())
            .unwrap();
    }
    assert_eq!(timeline.confirmed_frame(), 2);
    timeline.receive_remote(4, 0).unwrap();
    assert_eq!(timeline.confirmed_frame(), 2);
    timeline.receive_remote(2, 0).unwrap();
    assert_eq!(timeline.confirmed_frame(), 3);
    timeline.receive_remote(3, 0).unwrap();
    assert_eq!(timeline.confirmed_frame(), 5);
}

#[test]
fn invalid_rewrites_and_far_future_frames_do_not_grow_history() {
    let mut timeline = Timeline::new(Player::One);
    assert!(timeline.receive_remote(u64::MAX, 1).is_err());
    assert!(timeline.receive_remote(0, 1).is_err());
    assert_eq!(timeline.retained_inputs(), 2);
    assert!(timeline.receive_remote(2, 7).unwrap());
    assert!(!timeline.receive_remote(2, 7).unwrap());
    assert!(timeline.receive_remote(2, 8).is_err());
    assert_eq!(timeline.retained_inputs(), 3);
}

#[test]
fn thousands_of_frames_retire_history_without_losing_prediction_seed() {
    for player in [Player::One, Player::Two] {
        for frames in InputDelay::MIN..=InputDelay::MAX {
            let mut timeline = Timeline::with_delay(player, InputDelay::new(frames).unwrap());
            for frame in 0..10_000 {
                let (scheduled, _) = timeline.sample_local(frame as u8).unwrap();
                timeline.receive_remote(scheduled, 81).unwrap();
                let input = timeline.next_frame().unwrap().unwrap();
                if frame >= frames {
                    assert_eq!(input.ports[1 - port(player)], 81);
                }
                timeline.advance(input).unwrap();
                assert!(timeline.retained_inputs() <= (RETAINED_INPUTS + frames + 1) as usize);
                assert!(timeline.executed.is_empty());
            }
            assert!(!timeline.receive_remote(2, 81).unwrap());
            assert_eq!(
                timeline.next_frame().unwrap().unwrap().ports[1 - port(player)],
                81
            );
        }
    }
}

#[test]
fn replay_request_cannot_skip_or_rewrite_a_frame() {
    let mut timeline = Timeline::new(Player::One);
    for _ in 0..5 {
        timeline.sample_local(0).unwrap();
        timeline
            .advance(timeline.next_frame().unwrap().unwrap())
            .unwrap();
    }
    timeline.receive_remote(2, 1).unwrap();
    let replay = timeline.correction().unwrap();
    assert!(timeline.corrected(&replay[1..]).is_err());
    assert_eq!(timeline.correction().unwrap(), replay);
}

#[test]
fn slow_peer_can_receive_inputs_already_scheduled_by_an_ahead_peer() {
    let mut one = Timeline::new(Player::One);
    let mut two = Timeline::new(Player::Two);
    let (frame, buttons) = one.sample_local(7).unwrap();
    two.receive_remote(frame, buttons).unwrap();
    for _ in 0..INPUT_DELAY + 1 + PREDICTION_WINDOW {
        two.sample_local(9).unwrap();
        two.advance(two.next_frame().unwrap().unwrap()).unwrap();
    }
    let (frame, buttons) = two.sample_local(9).unwrap();
    assert_eq!(frame, 13);
    one.receive_remote(frame, buttons).unwrap();
    assert!(one.receive_remote(frame + 1, buttons).is_err());
}

#[test]
fn delay_jitter_loss_duplicates_and_reordering_preserve_corrected_confirmation() {
    for frames in InputDelay::MIN..=InputDelay::MAX {
        for rtt in [0, 30, 60, 100, 150] {
            for jitter in [0, 5, 15] {
                for loss in [0, 1, 3] {
                    virtual_pair(InputDelay::new(frames).unwrap(), rtt, jitter, loss);
                }
            }
        }
    }
}

#[test]
fn higher_delay_avoids_replay_when_one_way_delivery_fits_its_lead() {
    let (low_replays, low_stalls) = virtual_pair(InputDelay::new(2).unwrap(), 150, 15, 0);
    let (high_replays, high_stalls) = virtual_pair(InputDelay::new(8).unwrap(), 150, 15, 0);
    assert!(low_replays > 0);
    assert_eq!(high_replays, 0);
    assert_eq!((low_stalls, high_stalls), (0, 0));
}

fn virtual_pair(
    input_delay: InputDelay,
    rtt_ms: u64,
    jitter_ms: u64,
    loss_percent: u64,
) -> (usize, u64) {
    let mut peers = [
        Timeline::with_delay(Player::One, input_delay),
        Timeline::with_delay(Player::Two, input_delay),
    ];
    let mut sampled = [BTreeMap::new(), BTreeMap::new()];
    let mut simulated = [BTreeMap::new(), BTreeMap::new()];
    let mut checked = [0, 0];
    let mut packets: Vec<(u64, usize, u64, u8)> = Vec::new();
    let mut corrections = 0;
    let mut stalls = 0;
    let mut terminal = None;
    for tick in 0..640_u64 {
        if tick == 600 {
            terminal = Some(peers.iter().map(Timeline::frame).max().unwrap());
        }
        let mut due = Vec::new();
        packets.retain(|&(arrival, player, frame, buttons)| {
            if arrival <= tick {
                due.push((player, frame, buttons));
                false
            } else {
                true
            }
        });
        due.reverse();
        for (player, frame, buttons) in due {
            peers[1 - player].receive_remote(frame, buttons).unwrap();
        }
        for player in 0..2 {
            let peer = &mut peers[player];
            let correction = peer.correction().unwrap();
            corrections += correction.len();
            for input in &correction {
                simulated[player].insert(input.frame, input.ports);
            }
            peer.corrected(&correction).unwrap();
            if terminal.is_none_or(|frame| peer.frame() < frame) {
                let buttons =
                    ((peer.frame() * (17 + player as u64 * 12) + 3) ^ (peer.frame() >> 1)) as u8;
                let (frame, buttons) = peer.sample_local(buttons).unwrap();
                if sampled[player].insert(frame, buttons).is_none() {
                    let seed = frame * 31 + player as u64 * 47;
                    let jitter = if jitter_ms == 0 {
                        0
                    } else {
                        seed % (jitter_ms + 1)
                    };
                    let extra = if seed % 100 < loss_percent { 3 } else { 0 };
                    let delay = (rtt_ms / 2 + jitter).div_ceil(16) + extra;
                    packets.push((tick + delay, player, frame, buttons));
                    if seed.is_multiple_of(19) {
                        packets.push((tick + delay + 1, player, frame, buttons));
                    }
                }
                if let Some(input) = peer.next_frame().unwrap() {
                    simulated[player].insert(input.frame, input.ports);
                    peer.advance(input).unwrap();
                } else {
                    stalls += 1;
                }
            }
            while checked[player] < peer.confirmed_frame() {
                let frame = checked[player];
                let expected = if frame < input_delay.frames() {
                    [0, 0]
                } else {
                    [sampled[0][&frame], sampled[1][&frame]]
                };
                assert_eq!(
                    simulated[player][&frame], expected,
                    "delay={input_delay:?}, rtt={rtt_ms}, jitter={jitter_ms}, loss={loss_percent}, frame={frame}, player={player}"
                );
                checked[player] += 1;
            }
            assert!(peer.prediction_depth() <= PREDICTION_WINDOW);
            assert!(
                peer.retained_inputs() <= (RETAINED_INPUTS + input_delay.lookahead() + 1) as usize
            );
            assert!(peer.executed.len() <= PREDICTION_WINDOW as usize);
        }
    }
    assert!(checked.iter().all(|&frames| frames >= 540));
    assert!(packets.is_empty());
    if rtt_ms >= 100 && input_delay.frames() <= 2 {
        assert!(corrections > 0);
    }
    if loss_percent == 0 {
        assert_eq!(stalls, 0);
    }
    for player in 0..2 {
        assert_eq!(
            checked[player],
            terminal.unwrap(),
            "delay={input_delay:?}, rtt={rtt_ms}, jitter={jitter_ms}, loss={loss_percent}, player={player}"
        );
        assert_eq!(peers[player].frame(), terminal.unwrap());
    }
    (corrections, stalls)
}
