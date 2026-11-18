use super::*;
use crate::emu_backend::EmuBackend;
use crate::netplay::md_fixture::{self as fixture, Baseline};
use zeff_md_core::{ControllerKind, ControllerPort, Region, TimingModel};

type Confirmed = (zeff_netplay::wire::Message, [u16; 2], Vec<u32>);

struct Worker {
    worker: EmuLoop,
    responses: crossbeam_channel::Receiver<EmuResponse>,
    baseline: Baseline,
    battery: crate::emu_thread::persistence::BatteryFlushSchedule,
    markers: Vec<crate::audio_recorder::AudioTimelineDiscontinuity>,
    frames: Vec<Confirmed>,
    ready: Option<crate::netplay::md_input::MdNetplayInputProfile>,
    prediction: u64,
    rollback: u64,
    stepped: u64,
}

impl Worker {
    fn new(backend: EmuBackend) -> Self {
        let baseline = Baseline::capture(&backend);
        let (_commands, rx) = crossbeam_channel::unbounded();
        let (tx, frames) = crossbeam_channel::bounded(2);
        let (responses, received) = crossbeam_channel::unbounded();
        let mut worker = EmuLoop::new(
            backend,
            rx,
            tx,
            frames,
            responses,
            EmuLoopConfig {
                shared_framebuffer: crate::emu_thread::types::new_shared_framebuffer(),
                save_recovery_on_shutdown: false,
                recovery: None,
            },
        );
        worker.battery_flush.mark_potentially_dirty();
        worker
            .battery_flush
            .finish_attempt(Instant::now() + Duration::from_secs(2), false);
        worker
            .pending_audio_discontinuities
            .push(crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation);
        Self {
            battery: worker.battery_flush.clone(),
            markers: worker.pending_audio_discontinuities.clone(),
            worker,
            responses: received,
            baseline,
            frames: Vec::new(),
            ready: None,
            prediction: 0,
            rollback: 0,
            stepped: 0,
        }
    }
    fn start(&mut self, stream: TcpStream, player: Player, build: [u8; 32]) {
        assert!(
            self.worker
                .handle_command(EmuCommand::StartNetplay(Box::new(crate::netplay::Start {
                    stream: stream.into(),
                    player,
                    build,
                    secret: [5; 32],
                    scope: zeff_netplay::endpoint::ConnectionScope::Loopback,
                    allow_different_versions: false,
                    verify_every_frame: true,
                    input_delay: zeff_netplay::rollback::InputDelay::new(0).unwrap(),
                })))
        );
        assert!(self.worker.netplay.is_some());
        assert!(fixture::md(&self.worker.backend).rollback_owned());
        assert!(!fixture::md(&self.worker.backend).host_persistence_enabled());
    }
    fn pump(&mut self) {
        self.worker.poll_netplay();
        while let Ok(response) = self.responses.try_recv() {
            match response {
                EmuResponse::Netplay(Response::Ready { md_input }) => {
                    assert!(self.ready.is_none());
                    self.ready = Some(md_input.unwrap());
                }
                EmuResponse::Netplay(Response::Frame {
                    checkpoint,
                    ports,
                    audio,
                }) => {
                    assert!(audio.iter().any(|sample| *sample != 0.0));
                    self.frames.push((checkpoint, ports, fixture::bits(&audio)));
                }
                EmuResponse::Netplay(Response::Presented {
                    frame,
                    step_complete,
                    prediction_depth,
                    rollback_frames,
                    ..
                }) => {
                    if step_complete {
                        self.stepped = frame;
                    }
                    self.prediction = self.prediction.max(prediction_depth);
                    self.rollback += rollback_frames;
                }
                EmuResponse::Netplay(
                    Response::Rejected(reason) | Response::Stopped { reason, .. },
                ) => {
                    panic!("unexpected worker failure: {reason}");
                }
                _ => {}
            }
        }
    }
    fn step(&mut self, input: u16) {
        assert!(self.worker.handle_command(EmuCommand::StepNetplay(input)));
        self.pump();
    }
    fn stop(mut self) {
        assert!(self.worker.handle_command(EmuCommand::StopNetplay));
        let stopped: Vec<_> = self.responses.try_iter().collect();
        assert!(stopped.iter().any(|response| matches!(
            response,
            EmuResponse::Netplay(Response::Stopped { restored: true, .. })
        )));
        assert!(!self.worker.netplay_restore_failed);
        assert!(self.worker.battery_flush == self.battery);
        assert_eq!(self.worker.pending_audio_discontinuities, self.markers);
        self.baseline.verify(&self.worker.backend);
    }
}

#[derive(Debug)]
enum OwnerCommand {
    Start(TcpStream, [u8; 32]),
    Step(u16),
    Inspect,
    ArmStop,
    Stop,
}

struct OwnerStatus {
    ready: Option<crate::netplay::md_input::MdNetplayInputProfile>,
    frames: usize,
    prediction: u64,
    stepped: u64,
}
impl OwnerStatus {
    fn capture(worker: &Worker) -> Self {
        Self {
            ready: worker.ready,
            frames: worker.frames.len(),
            prediction: worker.prediction,
            stepped: worker.stepped,
        }
    }
}

struct ConcurrentWorker {
    commands: crossbeam_channel::Sender<OwnerCommand>,
    statuses: crossbeam_channel::Receiver<OwnerStatus>,
}
impl ConcurrentWorker {
    fn request(&self, command: OwnerCommand) -> OwnerStatus {
        self.commands
            .send_timeout(command, Duration::from_secs(3))
            .expect("MD owner command deadline");
        self.statuses
            .recv_timeout(Duration::from_secs(3))
            .expect("MD owner response deadline")
    }
    fn stop(&self) {
        self.commands
            .send_timeout(OwnerCommand::Stop, Duration::from_secs(3))
            .expect("MD owner stop deadline");
    }
}

fn timed_owner_call(
    player: Player,
    phase: &str,
    reports: &mut usize,
    action: impl FnOnce(),
) -> Duration {
    let started = Instant::now();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action));
    let elapsed = started.elapsed();
    if phase != "pump" || (elapsed >= Duration::from_millis(10) && *reports < 16) || result.is_err()
    {
        println!(
            "MD_NATIVE_OWNER_WORK role={player:?} phase={phase} elapsed_us={} failed={}",
            elapsed.as_micros(),
            result.is_err()
        );
        if phase == "pump" {
            *reports += 1;
        }
    }
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
    elapsed
}

fn run_concurrent_owner(
    backend: EmuBackend,
    player: Player,
    commands: crossbeam_channel::Receiver<OwnerCommand>,
    statuses: crossbeam_channel::Sender<OwnerStatus>,
) -> (Vec<Confirmed>, u64, Vec<u8>) {
    let mut worker = Worker::new(backend);
    let mut reports = 0;
    let mut max_pump = Duration::ZERO;
    let mut raw = Vec::new();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        loop {
            let command = match commands.recv_timeout(Duration::from_millis(1)) {
                Ok(command) => command,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    max_pump = max_pump.max(timed_owner_call(player, "pump", &mut reports, || {
                        worker.pump();
                    }));
                    continue;
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break false,
            };
            match command {
                OwnerCommand::Start(stream, build) => {
                    timed_owner_call(player, "start", &mut reports, || {
                        worker.start(stream, player, build);
                    });
                    max_pump = max_pump.max(timed_owner_call(player, "pump", &mut reports, || {
                        worker.pump();
                    }));
                }
                OwnerCommand::Step(word) => {
                    timed_owner_call(player, "step", &mut reports, || worker.step(word));
                }
                OwnerCommand::Inspect => {
                    max_pump = max_pump.max(timed_owner_call(player, "pump", &mut reports, || {
                        worker.pump();
                    }));
                }
                OwnerCommand::ArmStop => {
                    if statuses
                        .send_timeout(OwnerStatus::capture(&worker), Duration::from_secs(3))
                        .is_err()
                    {
                        break false;
                    }
                    if !matches!(
                        commands.recv_timeout(Duration::from_secs(3)),
                        Ok(OwnerCommand::Stop)
                    ) {
                        break false;
                    }
                    raw = worker.worker.backend.encode_state_bytes().unwrap();
                    break true;
                }
                OwnerCommand::Stop => panic!("MD owner stop was not armed"),
            }
            if statuses
                .send_timeout(OwnerStatus::capture(&worker), Duration::from_secs(3))
                .is_err()
            {
                break false;
            }
        }
    }));
    println!(
        "MD_NATIVE_OWNER_WORK role={player:?} phase=pump-summary max_elapsed_us={} reports={reports}",
        max_pump.as_micros()
    );
    let frames = std::mem::take(&mut worker.frames);
    let rollback = worker.rollback;
    match result {
        Ok(true) => worker.stop(),
        result => {
            worker.worker.handle_command(EmuCommand::StopNetplay);
            worker.worker.handle_command(EmuCommand::Shutdown);
            if let Err(payload) = result {
                std::panic::resume_unwind(payload);
            }
        }
    }
    (frames, rollback, raw)
}

fn run_pair(region: Region, timing: TimingModel, kind: ControllerKind) {
    let (_a_dir, a) = fixture::loaded(fixture::config(region, timing, kind));
    let (_b_dir, b) = fixture::loaded(fixture::config(region, timing, kind));
    let (_reference_dir, mut reference) = fixture::loaded(fixture::config(region, timing, kind));
    let build = crate::netplay::connect::executable_build().unwrap();
    let config = crate::netplay::identity::identity_with_delay(
        &reference,
        build,
        zeff_netplay::rollback::InputDelay::new(0).unwrap(),
    )
    .unwrap_or_else(|error| {
        panic!("MD reference identity {region:?}/{timing:?}/{kind:?}: {error:#}")
    })
    .config;
    println!(
        "MD_NORMAL_NETPLAY_ORIGIN region={region:?} timing={timing:?} kind={kind:?} absolute_field={}",
        reference.frame_count()
    );
    assert!(crate::netplay::identity::checkpoint(&reference, 1, &[], config).is_err());
    assert!(crate::netplay::identity::checkpoint(&reference, u64::MAX, &[], config).is_err());
    let (one_inputs, two_inputs) = fixture::words(kind);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let one = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (two, _) = listener.accept().unwrap();
    let (a_frames, b_frames, rollback, a_raw, b_raw) = std::thread::scope(|scope| {
        let spawn = |backend, player| {
            let (command_tx, command_rx) = crossbeam_channel::bounded(1);
            let (status_tx, status_rx) = crossbeam_channel::bounded(1);
            let join =
                scope.spawn(move || run_concurrent_owner(backend, player, command_rx, status_tx));
            (
                ConcurrentWorker {
                    commands: command_tx,
                    statuses: status_rx,
                },
                join,
            )
        };
        let (a, a_join) = spawn(a, Player::One);
        let (b, b_join) = spawn(b, Player::Two);
        let mut a_status = a.request(OwnerCommand::Start(one, build));
        let mut b_status = b.request(OwnerCommand::Start(two, build));
        let deadline = Instant::now() + Duration::from_secs(3);
        while a_status.ready.is_none() || b_status.ready.is_none() {
            a_status = a.request(OwnerCommand::Inspect);
            b_status = b.request(OwnerCommand::Inspect);
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(a_status.ready.unwrap().player(), Player::One);
        assert_eq!(b_status.ready.unwrap().player(), Player::Two);
        for status in [&a_status, &b_status] {
            assert_eq!(status.ready.unwrap().kinds(), [kind; 2]);
        }
        for frame in 0..6 {
            a_status = a.request(OwnerCommand::Step(one_inputs[frame]));
            let deadline = Instant::now() + Duration::from_secs(90);
            while a_status.stepped < frame as u64 + 1 {
                a_status = a.request(OwnerCommand::Inspect);
                b.request(OwnerCommand::Inspect);
                assert!(
                    Instant::now() < deadline,
                    "MD predicted field completion deadline"
                );
                std::thread::yield_now();
            }
            assert_eq!(a_status.stepped, frame as u64 + 1);
            assert_eq!(a_status.frames, frame);
            assert!(a_status.prediction > 0);
            b_status = b.request(OwnerCommand::Step(two_inputs[frame]));
            let deadline = Instant::now() + Duration::from_secs(90);
            while a_status.frames < frame + 1
                || b_status.frames < frame + 1
                || b_status.stepped < frame as u64 + 1
            {
                a_status = a.request(OwnerCommand::Inspect);
                b_status = b.request(OwnerCommand::Inspect);
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert_eq!(b_status.stepped, frame as u64 + 1);
        }
        a.request(OwnerCommand::ArmStop);
        b.request(OwnerCommand::ArmStop);
        a.stop();
        b.stop();
        let (a_frames, rollback, a_raw) = a_join.join().expect("MD first owner failed");
        let (b_frames, _, b_raw) = b_join.join().expect("MD second owner failed");
        (a_frames, b_frames, rollback, a_raw, b_raw)
    });
    assert!(rollback > 0);
    assert_eq!(a_frames, b_frames);
    assert_eq!(a_frames.len(), 6);
    for (index, (checkpoint, ports, pcm)) in a_frames.iter().enumerate() {
        assert_eq!(*ports, [one_inputs[index], two_inputs[index]]);
        reference.set_input(one_inputs[index] as u8, (one_inputs[index] >> 8) as u8);
        reference.set_input_p2(two_inputs[index] as u8, (two_inputs[index] >> 8) as u8);
        reference.step_frame();
        let mut audio = Vec::new();
        reference.drain_audio_samples_into(&mut audio);
        assert_eq!(*pcm, fixture::bits(&audio));
        assert_eq!(
            *checkpoint,
            crate::netplay::identity::checkpoint(&reference, index as u64 + 1, &audio, config)
                .unwrap()
        );
        assert_eq!(
            &fixture::md(&reference).core().work_ram()[0x100..0x104],
            &fixture::PINS[index % 2]
        );
        if kind == ControllerKind::SixButton {
            assert_eq!(
                &fixture::md(&reference).core().work_ram()[0x104..0x108],
                &fixture::EXTRA_PINS[index % 2]
            );
        }
        for (port, raw) in
            [ControllerPort::One, ControllerPort::Two]
                .into_iter()
                .zip(if index % 2 == 0 {
                    [0xe7, 0x5d]
                } else {
                    [0xde, 0xab]
                })
        {
            assert_eq!(
                fixture::md(&reference).core().input().controller_raw(port),
                raw
            );
        }
    }
    assert_eq!(a_raw, reference.encode_state_bytes().unwrap());
    assert_eq!(b_raw, reference.encode_state_bytes().unwrap());
}

#[test]
fn md_normal_native_workers_deliver_real_corrections_and_restore_frozen_baselines() {
    run_pair(
        Region::NtscUs,
        TimingModel::Standard,
        ControllerKind::ThreeButton,
    );
    for kind in [ControllerKind::ThreeButton, ControllerKind::SixButton] {
        run_pair(Region::NtscUs, TimingModel::Precise, kind);
    }
}

#[test]
fn md_normal_native_workers_cross_all_twelve_loaded_configurations_and_both_roles() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            for kind in [ControllerKind::ThreeButton, ControllerKind::SixButton] {
                run_pair(region, timing, kind);
            }
        }
    }
}

#[test]
fn md_native_worker_preserves_scheduled_flushes_and_fences_a_changed_host_on_stop() {
    let (_directory, backend) = fixture::loaded(fixture::config(
        Region::NtscUs,
        TimingModel::Standard,
        ControllerKind::ThreeButton,
    ));
    let mut worker = Worker::new(backend);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let local = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (remote, _) = listener.accept().unwrap();
    let (_peer_directory, peer_backend) = fixture::loaded(fixture::config(
        Region::NtscUs,
        TimingModel::Standard,
        ControllerKind::ThreeButton,
    ));
    let mut peer = Worker::new(peer_backend);
    peer.start(
        remote,
        Player::Two,
        crate::netplay::connect::executable_build().unwrap(),
    );
    worker.start(
        local,
        Player::One,
        crate::netplay::connect::executable_build().unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    while worker.ready.is_none() || peer.ready.is_none() {
        worker.pump();
        peer.pump();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    worker.step(fixture::ONE[0]);
    let deadline = Instant::now() + Duration::from_secs(90);
    while worker.stepped < 1 {
        worker.pump();
        peer.pump();
        assert!(
            Instant::now() < deadline,
            "MD host-fence field completion deadline"
        );
        std::thread::yield_now();
    }
    assert_eq!(worker.stepped, 1);
    assert_eq!(worker.worker.backend.frame_count(), 1);
    assert!(worker.prediction > 0);
    worker
        .worker
        .flush_battery_sram_if_due(Instant::now() + Duration::from_secs(300));
    assert!(worker.worker.battery_flush == worker.battery);
    assert!(worker.worker.backend.flush_battery_sram().is_err());
    worker
        .worker
        .battery_flush
        .finish_attempt(Instant::now() + Duration::from_secs(99), false);
    assert!(worker.worker.handle_command(EmuCommand::StopNetplay));
    assert!(worker.worker.netplay_restore_failed);
    assert!(fixture::md(&worker.worker.backend).rollback_owned());
    assert!(!fixture::md(&worker.worker.backend).host_persistence_enabled());
    worker.baseline.verify_restoring(&mut worker.worker.backend);
    peer.stop();
    assert!(worker.responses.try_iter().any(|response| matches!(
        response,
        EmuResponse::Netplay(Response::Stopped {
            restored: false,
            ..
        })
    )));
    let blocked = worker.worker.backend.encode_state_bytes().unwrap();
    assert!(worker.worker.handle_command(EmuCommand::Reset));
    assert_eq!(worker.worker.backend.encode_state_bytes().unwrap(), blocked);
    assert!(matches!(
        worker.responses.try_recv().unwrap(),
        EmuResponse::Netplay(Response::Stopped {
            restored: false,
            ..
        })
    ));
}
