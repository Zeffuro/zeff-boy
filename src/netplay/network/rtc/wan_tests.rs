use std::collections::BTreeMap;

use super::*;
use crate::netplay::network::tests::{event, identity};
use zeff_netplay::rollback::{FrameInput, PREDICTION_WINDOW, Timeline};

mod link;
use link::{IMPAIRED, LOCAL, Profile, WAN};

const TICKS: u64 = 120;

struct Driver {
    network: Network,
    timeline: Timeline,
    sampled: Option<u64>,
    executed: BTreeMap<u64, [u16; 2]>,
    verified: u64,
    progress: u64,
    stalls: u64,
    replayed: u64,
    max_prediction: u64,
    ack_samples: u64,
    ack_wait: Vec<f64>,
    tick_time: Vec<f64>,
}

impl Driver {
    fn drain(&mut self, remote: Player) {
        while let Some(event) = self.network.poll().unwrap() {
            match event {
                Event::Message(Message::Input {
                    player,
                    frame,
                    buttons,
                }) => {
                    assert_eq!(player, remote);
                    self.timeline.receive_remote(frame, buttons).unwrap();
                }
                Event::Message(Message::Progress { frame, confirmed }) => {
                    assert_eq!(frame, self.progress);
                    assert_eq!(confirmed, frame);
                    self.progress += 1;
                }
                other => panic!("unexpected shaped-worker event: {other:?}"),
            }
        }
        let correction = self.timeline.correction().unwrap();
        self.replayed += correction.len() as u64;
        for FrameInput { frame, ports } in &correction {
            self.executed.insert(*frame, *ports);
        }
        self.timeline.corrected(&correction).unwrap();
        let stats = self.network.stats();
        if stats.ack_samples != self.ack_samples {
            self.ack_samples = stats.ack_samples;
            self.ack_wait
                .push(stats.ack_wait.unwrap().as_secs_f64() * 1000.0);
        }
    }

    fn tick(&mut self, port: usize, tick: u64, truth: &mut [BTreeMap<u64, u16>; 2]) {
        let started = Instant::now();
        self.drain([Player::Two, Player::One][port]);
        if !self.advance(port, truth) {
            self.stalls += 1;
        }
        self.network
            .send(Message::Progress {
                frame: tick,
                confirmed: tick,
            })
            .unwrap();
        self.tick_time
            .push(started.elapsed().as_secs_f64() * 1000.0);
    }

    fn advance(&mut self, port: usize, truth: &mut [BTreeMap<u64, u16>; 2]) -> bool {
        let value = ((self.timeline.frame() * 17 + 1 + port as u64 * 29) % 256) as u16;
        let (frame, buttons) = self.timeline.sample_local(value).unwrap();
        if self.sampled != Some(frame) {
            assert!(truth[port].insert(frame, buttons).is_none());
            self.network
                .send(Message::Input {
                    player: [Player::One, Player::Two][port],
                    frame,
                    buttons,
                })
                .unwrap();
            self.sampled = Some(frame);
        }
        let advanced = if let Some(input) = self.timeline.next_frame().unwrap() {
            self.executed.insert(input.frame, input.ports);
            self.timeline.advance(input).unwrap();
            true
        } else {
            false
        };
        self.max_prediction = self.max_prediction.max(self.timeline.prediction_depth());
        assert!(self.max_prediction <= PREDICTION_WINDOW);
        assert!(self.timeline.retained_inputs() <= 64);
        advanced
    }

    fn verify(&mut self, truth: &[BTreeMap<u64, u16>; 2], delay: u64) {
        let confirmed = self.timeline.confirmed_frame();
        for frame in self.verified..confirmed {
            let expected = if frame < delay {
                [0; 2]
            } else {
                [truth[0][&frame], truth[1][&frame]]
            };
            assert_eq!(self.executed[&frame], expected, "confirmed frame {frame}");
        }
        self.verified = confirmed;
    }
}

fn percentile(values: &[f64], percentile: usize) -> Option<f64> {
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    values
        .get((values.len().saturating_sub(1) * percentile).div_ceil(100))
        .copied()
}

fn run(profile: Profile, hz: u64, delay: u64) -> serde_json::Value {
    let peers = link::pair(profile);
    let counters = peers.each_ref().map(|peer| peer.counters.clone());
    let mut drivers: [Driver; 2] = peers
        .into_iter()
        .enumerate()
        .map(|(port, peer)| {
            let player = [Player::One, Player::Two][port];
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let input_delay = InputDelay::new(delay).unwrap();
            let network =
                spawn_peer(peer, runtime, player, identity(), [9; 32], input_delay).unwrap();
            Driver {
                network,
                timeline: Timeline::with_delay(player, input_delay),
                sampled: None,
                executed: BTreeMap::new(),
                verified: 0,
                progress: 0,
                stalls: 0,
                replayed: 0,
                max_prediction: 0,
                ack_samples: 0,
                ack_wait: Vec::new(),
                tick_time: Vec::new(),
            }
        })
        .collect::<Vec<_>>()
        .try_into()
        .ok()
        .unwrap();
    for driver in &drivers {
        assert!(matches!(event(&driver.network), Event::Ready));
    }
    let mut truth = [BTreeMap::new(), BTreeMap::new()];
    let period = Duration::from_secs_f64(1.0 / hz as f64);
    let started = Instant::now();
    let mut next = started;
    let mut late_ticks = 0;
    for tick in 0..TICKS {
        thread::sleep(next.saturating_duration_since(Instant::now()));
        if Instant::now().duration_since(next) >= period {
            late_ticks += 1;
        }
        next = Instant::now() + period;
        for (port, driver) in drivers.iter_mut().enumerate() {
            driver.tick(port, tick, &mut truth);
        }
        for driver in &mut drivers {
            driver.verify(&truth, delay);
        }
    }
    let paced_seconds = started.elapsed().as_secs_f64();
    let paced_stats = drivers.each_ref().map(|driver| driver.network.stats());
    let paced_frames = drivers.each_ref().map(|driver| driver.timeline.frame());
    let target = *paced_frames.iter().max().unwrap();
    let settle_started = Instant::now();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        for (port, driver) in drivers.iter_mut().enumerate() {
            driver.drain([Player::Two, Player::One][port]);
            if Instant::now() >= next && driver.timeline.frame() < target {
                driver.advance(port, &mut truth);
            }
            driver.verify(&truth, delay);
        }
        if Instant::now() >= next {
            next = Instant::now() + period;
        }
        if drivers
            .iter()
            .all(|driver| driver.verified == target && driver.progress == TICKS)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "shaped workers did not settle: {profile:?}, {hz}Hz, delay={delay}, states={:?}",
            drivers.each_ref().map(|driver| (
                driver.timeline.frame(),
                driver.verified,
                driver.progress
            ))
        );
        thread::sleep(Duration::from_millis(1));
    }
    let settle_seconds = settle_started.elapsed().as_secs_f64();
    let peers: Vec<_> = drivers
        .iter()
        .enumerate()
        .map(|(port, driver)| {
            let stats = driver.network.stats();
            assert!(driver.timeline.frame() >= TICKS / 2);
            assert!(stats.ack_samples > 0);
            assert!(paced_stats[port].sent_bytes as f64 / paced_seconds < 100_000.0);
            if profile.travel != Duration::ZERO {
                assert!(driver.replayed > 0);
                assert!(driver.ack_wait.iter().all(|&wait| wait >= 200.0));
            }
            if profile.impaired {
                assert!(counters[port].dropped.load(Ordering::Relaxed) > 0);
                assert!(counters[port].periodic_drops.load(Ordering::Relaxed) > 0);
                assert!(counters[port].burst_drops.load(Ordering::Relaxed) > 0);
                assert!(counters[port].reordered.load(Ordering::Relaxed) > 0);
            }
            serde_json::json!({
                "frames": driver.timeline.frame(), "confirmed": driver.verified,
                "paced_frames": paced_frames[port],
                "recovery_frames": driver.timeline.frame() - paced_frames[port],
                "stall_ticks": driver.stalls, "replayed_inputs": driver.replayed,
                "max_prediction": driver.max_prediction,
                "sent_packets": stats.sent, "sent_payload_bytes": stats.sent_bytes,
                "paced_sent_packets": paced_stats[port].sent,
                "paced_sent_payload_bytes": paced_stats[port].sent_bytes,
                "paced_payload_bytes_per_second": paced_stats[port].sent_bytes as f64 / paced_seconds,
                "repeated_batches": stats.repeated_batches,
                "ack_total_samples": stats.ack_samples,
                "ack_observed_snapshots": driver.ack_wait.len(),
                "observed_ack_wait_ms_p50": percentile(&driver.ack_wait, 50),
                "observed_ack_wait_ms_p95": percentile(&driver.ack_wait, 95),
                "driver_tick_ms_p95": percentile(&driver.tick_time, 95),
                "injected_drops": counters[port].dropped.load(Ordering::Relaxed),
                "injected_periodic_drops": counters[port].periodic_drops.load(Ordering::Relaxed),
                "injected_burst_drops": counters[port].burst_drops.load(Ordering::Relaxed),
                "delivered_reorders": counters[port].reordered.load(Ordering::Relaxed),
                "max_shaper_pending": counters[port].max_pending.load(Ordering::Relaxed),
            })
        })
        .collect();
    for driver in &mut drivers {
        driver.network.cancel();
    }
    assert!(
        counters
            .iter()
            .all(|counter| counter.closed.load(Ordering::Acquire))
    );
    serde_json::json!({
        "scope": "application_worker_and_timeline", "profile": profile.name,
        "hz": hz, "delay_frames": delay, "ticks": TICKS,
        "paced_seconds": paced_seconds, "late_pacing_ticks": late_ticks,
        "settle_seconds": settle_seconds,
        "peers": peers,
    })
}

#[test]
fn wan_workers_recover_authenticated_inputs_and_preserve_reliable_control() {
    for profile in [WAN, IMPAIRED] {
        println!("{}", run(profile, 60, 2));
    }
}

#[test]
#[ignore = "paced WAN measurement matrix, run with --ignored --nocapture"]
fn wan_measurement_matrix() {
    for profile in [LOCAL, WAN, IMPAIRED] {
        for hz in [60, 75] {
            for delay in [0, 2, 3, 4] {
                println!("{}", run(profile, hz, delay));
            }
        }
    }
}
