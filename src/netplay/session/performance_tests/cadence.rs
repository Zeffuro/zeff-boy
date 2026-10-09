use super::*;
use crate::test_support::TestDirectory;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use zeff_netplay::wire::Identity;

const FRAMES: u64 = 72;
const BURST: u64 = 6;
const EXECUTION_BOUND: usize = profile::MAX_RECORDS;

struct Expected {
    ports: [u16; 2],
    pcm: Vec<u32>,
    checkpoint: Message,
}

struct Reference {
    _directory: TestDirectory,
    backend: EmuBackend,
    frames: Vec<Expected>,
    config: [u8; 32],
}

impl Reference {
    fn new(case: Case, player: Player) -> Self {
        let (directory, mut backend) = case.load();
        let config = identity::identity_with_delay(&backend, [7; 32], InputDelay::default())
            .unwrap()
            .config;
        core::persistence(&mut backend, Some(false)).unwrap();
        let mut lease = Lease::begin(&mut backend, player).unwrap();
        let mask = lease.input_mask();
        let delay = InputDelay::default().frames();
        let frames = (0..FRAMES)
            .map(|frame| {
                let ports = if frame < delay {
                    [0; 2]
                } else {
                    ports(frame - delay, mask)
                };
                let audio = lease.advance(&mut backend, ports).unwrap();
                let snapshot = lease.capture(&backend).unwrap();
                Expected {
                    ports,
                    pcm: audio.iter().map(|sample| sample.to_bits()).collect(),
                    checkpoint: checkpoint(&backend, &snapshot, &audio, config),
                }
            })
            .collect();
        Self {
            _directory: directory,
            backend,
            frames,
            config,
        }
    }
}

struct Peer {
    directory: TestDirectory,
    backend: EmuBackend,
    session: Option<Session>,
    initial: Vec<u8>,
    identity: Identity,
    publication: bool,
    files: BTreeMap<PathBuf, Vec<u8>>,
    confirmed: u64,
    max_retained: usize,
    max_snapshots: usize,
    max_outputs: usize,
}

fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, result);
            } else {
                result.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    std::fs::read(&path).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

impl Peer {
    fn new(case: Case, stream: TcpStream, player: Player, dense: bool) -> Self {
        let (directory, mut backend) = case.load();
        let initial = backend.encode_state_bytes().unwrap();
        let identity =
            identity::identity_with_delay(&backend, [7; 32], InputDelay::default()).unwrap();
        let publication = core::persistence(&mut backend, None).unwrap();
        let files = files(directory.path());
        let session = Session::start(
            &mut backend,
            Start {
                stream: stream.into(),
                player,
                build: [7; 32],
                secret: [5; 32],
                scope: zeff_netplay::endpoint::ConnectionScope::Loopback,
                allow_different_versions: false,
                verify_every_frame: dense,
                input_delay: InputDelay::default(),
            },
        )
        .unwrap();
        Self {
            directory,
            backend,
            session: Some(session),
            initial,
            identity,
            publication,
            files,
            confirmed: 0,
            max_retained: 0,
            max_snapshots: 0,
            max_outputs: 0,
        }
    }

    fn session(&self) -> &Session {
        self.session.as_ref().unwrap()
    }

    fn collect(&mut self, reference: &Reference, dense: bool) {
        let session = self.session.as_mut().unwrap();
        while let Some(response) = session.responses.pop_front() {
            let (frame, audio) = match response {
                Response::Frame {
                    checkpoint,
                    ports,
                    audio,
                } => {
                    assert!(dense, "unexpected dense response in periodic session");
                    let expected = &reference.frames[self.confirmed as usize];
                    assert_eq!(checkpoint, expected.checkpoint);
                    assert_eq!(ports, expected.ports);
                    (self.confirmed + 1, audio)
                }
                Response::Audio { frame, audio } => {
                    assert!(!dense, "unexpected sparse response in dense session");
                    (frame, audio)
                }
                Response::Presented { retained_bytes, .. } => {
                    self.max_retained = self.max_retained.max(retained_bytes);
                    continue;
                }
                Response::Ready | Response::NetworkStats(_) => continue,
                _ => panic!("unexpected cost fixture response"),
            };
            assert_eq!(frame, self.confirmed + 1);
            assert!(frame <= FRAMES);
            assert_eq!(
                audio
                    .iter()
                    .map(|sample| sample.to_bits())
                    .collect::<Vec<_>>(),
                reference.frames[self.confirmed as usize].pcm,
                "confirmed PCM frame {frame}"
            );
            self.confirmed = frame;
        }
        self.max_retained = self.max_retained.max(session.retained_bytes());
        self.max_snapshots = self.max_snapshots.max(session.snapshots.len());
        self.max_outputs = self.max_outputs.max(session.outputs.len());
        assert!(session.snapshots.len() <= PREDICTION_WINDOW as usize + 1);
        assert!(session.outputs.len() <= PREDICTION_WINDOW as usize);
        assert!(session.profile.as_ref().unwrap().frames.len() <= EXECUTION_BOUND);
        assert!(!core::persistence(&mut self.backend, None).unwrap());
        assert!(self.backend.flush_battery_sram().unwrap().is_none());
    }

    fn step(&mut self, reference: &Reference, dense: bool) {
        let session = self.session.as_mut().unwrap();
        let frame = session.timeline.frame();
        let index = if session.player == Player::One { 0 } else { 1 };
        let buttons = ports(frame, session.lease.input_mask())[index];
        session.step(&mut self.backend, buttons).unwrap();
        assert_eq!(session.timeline.frame(), frame + 1);
        self.collect(reference, dense);
    }

    fn batch(&mut self, reference: &Reference, dense: bool) {
        self.session
            .as_mut()
            .unwrap()
            .batch(&mut self.backend, None)
            .unwrap();
        self.collect(reference, dense);
    }

    fn verify_snapshot(&mut self, reference: &Reference, frame: u64) {
        let session = self.session.as_mut().unwrap();
        assert_eq!(self.confirmed, frame);
        assert_eq!(session.committed, frame);
        assert_eq!(session.snapshots.len(), 1);
        assert!(session.outputs.is_empty());
        let snapshot = session.snapshots.get(&frame).unwrap();
        session.lease.restore(&mut self.backend, snapshot).unwrap();
        let captured = session.lease.capture(&self.backend).unwrap();
        let expected = &reference.frames[frame as usize - 1];
        let audio: Vec<_> = expected
            .pcm
            .iter()
            .map(|bits| f32::from_bits(*bits))
            .collect();
        assert_eq!(
            checkpoint(&self.backend, &captured, &audio, session.config),
            expected.checkpoint,
            "retained snapshot frame {frame}"
        );
        if frame.is_multiple_of(checks::HASH_INTERVAL) && !matches!(self.backend, EmuBackend::Ws(_))
        {
            assert_eq!(
                identity::checkpoint(&self.backend, frame, &audio, session.config).unwrap(),
                expected.checkpoint,
                "canonical periodic checkpoint frame {frame}"
            );
        }
    }

    fn report_and_restore(mut self, reference: &Reference, dense: bool) -> serde_json::Value {
        assert_eq!(self.confirmed, FRAMES);
        assert_eq!(
            self.backend.encode_state_bytes().unwrap(),
            reference.backend.encode_state_bytes().unwrap()
        );
        assert_eq!(
            identity::persistent_hash(&self.backend).unwrap(),
            identity::persistent_hash(&reference.backend).unwrap()
        );
        let mut session = self.session.take().unwrap();
        let profile = session.profile.take().unwrap();
        assert!(
            profile.frames.len() > FRAMES as usize,
            "forced correction did not replay"
        );
        let mut executions = BTreeMap::<u64, usize>::new();
        for cost in &profile.frames {
            assert!((1..=FRAMES).contains(&cost.frame));
            assert_eq!(
                cost.checkpoint.is_some(),
                dense || cost.frame.is_multiple_of(checks::HASH_INTERVAL)
            );
            assert!(
                cost.total >= cost.advance + cost.capture + cost.checkpoint.unwrap_or_default()
            );
            *executions.entry(cost.frame).or_default() += 1;
        }
        assert_eq!(executions.len(), FRAMES as usize);
        let stage = |samples: Vec<Duration>| {
            let micros: Vec<_> = samples
                .iter()
                .map(|value| value.as_secs_f64() * 1_000_000.0)
                .collect();
            let mut result = summary(&micros);
            result["total_us"] = serde_json::json!(micros.iter().sum::<f64>());
            result
        };
        let report = serde_json::json!({
            "player": format!("{:?}", session.player),
            "executed_frames": profile.frames.len(),
            "replayed_frames": profile.frames.len() - FRAMES as usize,
            "checkpoint_executions": profile.frames.iter().filter(|cost| cost.checkpoint.is_some()).count(),
            "periodic_replays": executions.iter().filter(|(frame, count)| frame.is_multiple_of(checks::HASH_INTERVAL) && **count > 1).map(|(frame, count)| serde_json::json!({"frame": frame, "executions": count})).collect::<Vec<_>>(),
            "advance": stage(profile.frames.iter().map(|cost| cost.advance).collect()),
            "capture": stage(profile.frames.iter().map(|cost| cost.capture).collect()),
            "checkpoint": stage(profile.frames.iter().filter_map(|cost| cost.checkpoint).collect()),
            "execute": stage(profile.frames.iter().map(|cost| cost.total).collect()),
            "max_reported_retained_bytes": self.max_retained,
            "max_snapshots": self.max_snapshots, "max_outputs": self.max_outputs,
            "profile_records_bound": EXECUTION_BOUND,
        });
        session.restore(&mut self.backend).unwrap();
        assert_eq!(self.backend.encode_state_bytes().unwrap(), self.initial);
        assert_eq!(
            identity::persistent_hash(&self.backend).unwrap(),
            self.identity.persistent
        );
        assert_eq!(
            core::persistence(&mut self.backend, None).unwrap(),
            self.publication
        );
        assert_eq!(
            identity::identity_with_delay(&self.backend, [7; 32], InputDelay::default()).unwrap(),
            self.identity
        );
        assert_eq!(files(self.directory.path()), self.files);
        report
    }
}

fn settle(peers: &mut [Peer; 2], references: &[Reference; 2], dense: bool, target: u64) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while peers.iter().any(|peer| peer.confirmed != target) {
        for (peer, reference) in peers.iter_mut().zip(references) {
            peer.batch(reference, dense);
        }
        assert!(
            Instant::now() < deadline,
            "cost fixture confirmation stalled at {target}"
        );
        std::thread::yield_now();
    }
    for (peer, reference) in peers.iter_mut().zip(references) {
        peer.verify_snapshot(reference, target);
    }
}

fn admitted(case: Case, dense: bool) -> [Peer; 2] {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let local = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (remote, _) = listener.accept().unwrap();
    let players = [
        case.player(),
        if case.player() == Player::One {
            Player::Two
        } else {
            Player::One
        },
    ];
    let mut peers = [
        Peer::new(case, local, players[0], dense),
        Peer::new(case, remote, players[1], dense),
    ];
    let deadline = Instant::now() + Duration::from_secs(3);
    while peers.iter().any(|peer| !peer.session().ready) {
        for peer in &mut peers {
            let session = peer.session.as_mut().unwrap();
            session.poll(&mut peer.backend).unwrap();
        }
        assert!(Instant::now() < deadline, "cost fixture admission stalled");
        std::thread::yield_now();
    }
    peers
}

fn measure_session(case: Case, dense: bool, reverse_order: bool) -> serde_json::Value {
    let players = [
        case.player(),
        if case.player() == Player::One {
            Player::Two
        } else {
            Player::One
        },
    ];
    let references = players.map(|player| Reference::new(case, player));
    let mut peers = admitted(case, dense);
    for (peer, reference) in peers.iter_mut().zip(&references) {
        let session = peer.session.as_mut().unwrap();
        assert_eq!(session.config, reference.config);
        session.responses.clear();
        session.profile = Some(Default::default());
    }
    let first = usize::from(reverse_order);
    let mut frame = 0;
    while frame < FRAMES {
        if [0, 30, 54].contains(&frame) {
            let leader = if frame == 30 { 1 - first } else { first };
            for endpoint in [leader, 1 - leader] {
                for _ in 0..BURST {
                    peers[endpoint].step(&references[endpoint], dense);
                }
                if endpoint == leader {
                    assert!(
                        peers[endpoint].session().timeline.prediction_depth()
                            >= BURST - InputDelay::default().frames()
                    );
                }
            }
            frame += BURST;
        } else {
            for endpoint in [first, 1 - first] {
                peers[endpoint].step(&references[endpoint], dense);
            }
            frame += 1;
        }
        settle(&mut peers, &references, dense, frame);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while peers
        .iter()
        .any(|peer| peer.session().hashes.matched_frame() != Some(checks::HASH_INTERVAL))
    {
        for (peer, reference) in peers.iter_mut().zip(&references) {
            peer.batch(reference, dense);
        }
        assert!(
            Instant::now() < deadline,
            "cost fixture periodic hash stalled"
        );
        std::thread::yield_now();
    }
    assert!(peers.iter().any(|peer| {
        peer.session()
            .profile
            .as_ref()
            .unwrap()
            .frames
            .iter()
            .filter(|cost| cost.frame == checks::HASH_INTERVAL)
            .count()
            > 1
    }));
    let [one, two] = peers;
    serde_json::json!({
        "scope": "actual_session_execute_stages", "fixture": case.label(),
        "comparison": "periodic versus dense on the same instrumented test source",
        "fixture_scope": "synthetic, unpaced loopback with snapshot restore/verification between synchronized targets",
        "measurement_limits": "execution clocks/allocations included, profile record append excluded; out-of-timer oracle work can perturb caches; not shipping, WAN or presentation latency",
        "verify_every_frame": dense, "frames": FRAMES,
        "input_delay": InputDelay::default().frames(), "reverse_step_order": reverse_order,
        "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "debug_assertions": cfg!(debug_assertions),
        "build_profile": std::env::var("ZEFF_NETPLAY_COST_PROFILE").unwrap_or_else(|_| "unspecified".into()),
        "source_revision": std::env::var("ZEFF_NETPLAY_COST_SOURCE").unwrap_or_else(|_| "unspecified".into()),
        "profile_label_source": "ZEFF_NETPLAY_COST_PROFILE environment; debug_assertions is compiler-derived",
        "source_label_source": "ZEFF_NETPLAY_COST_SOURCE environment",
        "root": env!("CARGO_MANIFEST_DIR"), "root_source": "CARGO_MANIFEST_DIR compile-time",
        "excludes": "admission, transport, correction restore, commit, reference and snapshot verification",
        "peers": [one.report_and_restore(&references[0], dense), two.report_and_restore(&references[1], dense)],
    })
}

#[test]
fn actual_session_checkpoint_cadence_preserves_all_adapters() {
    for case in CASES {
        for dense in [true, false] {
            measure_session(case, dense, false);
        }
    }
}

#[test]
fn full_execution_profile_rejects_before_core_or_session_mutation() {
    let mut peers = admitted(CASES[0], false);
    let peer = &mut peers[0];
    let before = peer.backend.encode_state_bytes().unwrap();
    let session = peer.session.as_mut().unwrap();
    let mut profile = profile::ExecutionProfile::default();
    for frame in 0..EXECUTION_BOUND {
        profile.frames.push(profile::FrameCost {
            frame: frame as u64,
            advance: Duration::ZERO,
            capture: Duration::ZERO,
            checkpoint: None,
            total: Duration::ZERO,
        });
    }
    session.profile = Some(profile);
    let counts = (
        session.snapshots.len(),
        session.outputs.len(),
        session.responses.len(),
    );
    let timeline = (
        session.timeline.frame(),
        session.timeline.confirmed_frame(),
        session.timeline.prediction_depth(),
    );
    let error = session
        .execute(
            &mut peer.backend,
            zeff_netplay::rollback::FrameInput {
                frame: 0,
                ports: [0; 2],
            },
        )
        .unwrap_err();
    assert_eq!(error.to_string(), "execution profile is full");
    assert_eq!(peer.backend.encode_state_bytes().unwrap(), before);
    assert_eq!(peer.backend.frame_count(), 0);
    assert_eq!(
        (
            session.snapshots.len(),
            session.outputs.len(),
            session.responses.len()
        ),
        counts
    );
    assert_eq!(
        (
            session.timeline.frame(),
            session.timeline.confirmed_frame(),
            session.timeline.prediction_depth()
        ),
        timeline
    );
    assert_eq!(
        session.profile.as_ref().unwrap().frames.len(),
        EXECUTION_BOUND
    );
    for mut peer in peers {
        peer.session
            .take()
            .unwrap()
            .restore(&mut peer.backend)
            .unwrap();
        assert_eq!(peer.backend.encode_state_bytes().unwrap(), peer.initial);
        assert_eq!(
            core::persistence(&mut peer.backend, None).unwrap(),
            peer.publication
        );
        assert_eq!(files(peer.directory.path()), peer.files);
    }
}

#[test]
#[ignore = "actual Session cadence ABBA, run alone with --ignored --nocapture --test-threads=1"]
fn actual_session_checkpoint_cadence_abba() {
    for case in CASES {
        for block in 0..2 {
            for (order, dense) in [true, false, false, true].into_iter().enumerate() {
                let mut report = measure_session(case, dense, block != 0);
                report["abba_block"] = serde_json::json!(block);
                report["abba_order"] = serde_json::json!(order);
                println!("{report}");
            }
        }
    }
}
