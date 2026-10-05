use super::*;

#[test]
fn simulated_200ms_rtt_corrects_inputs_and_bounds_jitter_stalls() {
    for player in [Player::One, Player::Two] {
        for delay in [0, 2, 3, 4] {
            // 100 ms one-way rounds to six 60 Hz frames or eight WonderSwan frames.
            for (travel, jitter) in [(6, 0), (8, 0), (6, 6)] {
                let mut timeline = Timeline::with_delay(player, InputDelay::new(delay).unwrap());
                let mut packets = VecDeque::new();
                let mut owned = BTreeMap::new();
                let mut remote = BTreeMap::new();
                let mut executed = BTreeMap::new();
                let mut stalls = 0;
                let mut corrections = 0;
                for tick in 0..360u64 {
                    let value = (tick.wrapping_mul(17) + 1) as u8;
                    let arrival = tick + travel + if tick % 31 == 0 { jitter } else { 0 };
                    remote.insert(tick + delay, value);
                    packets.push_back((arrival, tick + delay, value));
                    let (scheduled, buttons) = timeline.sample_local((tick * 7) as u8).unwrap();
                    owned.entry(scheduled).or_insert(buttons);
                    let mut pending = VecDeque::new();
                    while let Some((arrival, frame, buttons)) = packets.pop_front() {
                        if arrival <= tick {
                            timeline.receive_remote(frame, buttons).unwrap();
                        } else {
                            pending.push_back((arrival, frame, buttons));
                        }
                    }
                    packets = pending;
                    let replay = timeline.correction().unwrap();
                    corrections += replay.len();
                    for input in &replay {
                        executed.insert(input.frame, input.ports);
                    }
                    timeline.corrected(&replay).unwrap();
                    if let Some(input) = timeline.next_frame().unwrap() {
                        executed.insert(input.frame, input.ports);
                        timeline.advance(input).unwrap();
                    } else {
                        stalls += 1;
                    }
                    for frame in
                        timeline.confirmed_frame().saturating_sub(16)..timeline.confirmed_frame()
                    {
                        let mut expected = [0; 2];
                        if frame >= delay {
                            expected[port(player)] = owned[&frame];
                            expected[1 - port(player)] = remote[&frame];
                        }
                        assert_eq!(executed[&frame], expected);
                    }
                    assert!(timeline.prediction_depth() <= PREDICTION_WINDOW);
                    assert!(timeline.retained_inputs() <= (RETAINED_INPUTS + 24) as usize);
                }
                assert!(corrections > 0);
                if jitter == 0 && delay >= 2 {
                    assert_eq!(stalls, 0, "steady travel={travel}, delay={delay}");
                }
                if jitter != 0 && delay <= 3 {
                    assert!(stalls > 0, "jitter must reach the prediction bound");
                }
            }
        }
    }
}
