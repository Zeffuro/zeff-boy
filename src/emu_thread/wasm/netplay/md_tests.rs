use super::*;
use crate::netplay::md_fixture::{self as fixture, Baseline};
use crate::netplay::network::paired;
use crate::platform::Instant;
use std::time::Duration;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::wasm_bindgen_test;
use zeff_md_core::{ControllerKind, Region, TimingModel};
use zeff_netplay::lockstep::Player;

#[wasm_bindgen::prelude::wasm_bindgen(
    inline_js = "export function md_netplay_fixture_yield() { return new Promise(resolve => setTimeout(resolve, 1)); }"
)]
extern "C" {
    async fn md_netplay_fixture_yield();
}

type Confirmed = (zeff_netplay::wire::Message, [u16; 2], Vec<u32>);

struct Worker {
    thread: EmuThread,
    core: Baseline,
    host: MdWorkerBaseline,
    frames: Vec<Confirmed>,
    ready: Option<crate::netplay::md_input::MdNetplayInputProfile>,
    prediction: u64,
    rollback: u64,
    stepped: u64,
}
impl Worker {
    fn new(backend: EmuBackend) -> Self {
        let core = Baseline::capture(&backend);
        let thread = EmuThread::spawn(backend, false);
        let host = {
            let mut inner = thread.inner.borrow_mut();
            let (_, retained) = crate::platform::capture_save_writes(|| {
                crate::platform::write_save_data(
                    std::path::Path::new("retained-before-md.bin"),
                    &[9, 0, 5],
                )
            })
            .unwrap();
            assert_eq!(retained.len(), 1);
            assert_eq!(retained[0].parts_for_test().1, &[9, 0, 5]);
            inner.battery_dirty.observe(&retained);
            inner.battery_flush_requested = true;
            inner.battery_potentially_dirty = true;
            inner.next_battery_flush = web_time::Instant::now() + Duration::from_secs(117);
            inner
                .pending_audio_discontinuities
                .push(crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation);
            MdWorkerBaseline::capture(&inner)
        };
        Self {
            thread,
            core,
            host,
            frames: Vec::new(),
            ready: None,
            prediction: 0,
            rollback: 0,
            stepped: 0,
        }
    }
    fn start(&self, endpoint: paired::Endpoint, player: Player, build: [u8; 32]) {
        self.thread
            .send(EmuCommand::StartNetplay(Box::new(crate::netplay::Start {
                stream: crate::netplay::Transport::Paired(endpoint),
                player,
                build,
                secret: [5; 32],
                scope: zeff_netplay::endpoint::ConnectionScope::Loopback,
                allow_different_versions: false,
                verify_every_frame: true,
                input_delay: zeff_netplay::rollback::InputDelay::new(0).unwrap(),
            })));
        assert!(self.thread.inner.borrow().netplay.is_some());
    }
    fn pump(&mut self) {
        self.thread.service_netplay();
        let mut inner = self.thread.inner.borrow_mut();
        while let Some(response) = inner.pending_responses.pop_front() {
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
                    panic!("unexpected Node worker failure: {reason}");
                }
                _ => {}
            }
        }
    }
    fn step(&mut self, word: u16) {
        self.thread.send(EmuCommand::StepNetplay(word));
        self.pump();
    }
    fn stop(self) {
        self.thread.send(EmuCommand::StopNetplay);
        let inner = self.thread.inner.borrow();
        assert!(inner.pending_responses.iter().any(|response| matches!(
            response,
            EmuResponse::Netplay(Response::Stopped { restored: true, .. })
        )));
        assert!(!inner.netplay_restore_failed);
        self.host.verify(&inner).unwrap();
        self.core.verify(&inner.backend);
    }
}

async fn run_pair(region: Region, timing: TimingModel, kind: ControllerKind) {
    let a = fixture::loaded(fixture::config(region, timing, kind));
    let b = fixture::loaded(fixture::config(region, timing, kind));
    let mut reference = fixture::loaded(fixture::config(region, timing, kind));
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
    wasm_bindgen_test::console_log!(
        "MD_NORMAL_NETPLAY_ORIGIN region={region:?} timing={timing:?} kind={kind:?} absolute_field={}",
        reference.frame_count()
    );
    assert!(crate::netplay::identity::checkpoint(&reference, 1, &[], config).is_err());
    assert!(crate::netplay::identity::checkpoint(&reference, u64::MAX, &[], config).is_err());
    let (one, two, delivery) = paired::pair();
    let mut a = Worker::new(a);
    let mut b = Worker::new(b);
    a.start(one, Player::One, build);
    b.start(two, Player::Two, build);
    let deadline = Instant::now() + Duration::from_secs(3);
    while a.ready.is_none() || b.ready.is_none() {
        a.pump();
        b.pump();
        assert!(Instant::now() < deadline);
        md_netplay_fixture_yield().await;
    }
    assert_eq!(a.ready.unwrap().player(), Player::One);
    assert_eq!(b.ready.unwrap().player(), Player::Two);
    assert_eq!(a.ready.unwrap().kinds(), [kind; 2]);
    assert_eq!(b.ready.unwrap().kinds(), [kind; 2]);
    let (one_inputs, two_inputs) = fixture::words(kind);
    let mut host_turns = 0usize;
    for frame in 0..6 {
        delivery.hold_input();
        a.step(one_inputs[frame]);
        let deadline = Instant::now() + Duration::from_secs(90);
        while a.stepped < frame as u64 + 1 {
            a.pump();
            b.pump();
            assert!(
                Instant::now() < deadline,
                "MD Node predicted field completion deadline"
            );
            md_netplay_fixture_yield().await;
            host_turns += 1;
        }
        assert_eq!(a.stepped, frame as u64 + 1);
        assert_eq!(a.frames.len(), frame);
        assert!(a.prediction > 0);
        if frame == 0 {
            let deadline = Instant::now() + Duration::from_secs(90);
            while delivery.state().held < paired::INPUT_CAPACITY {
                a.pump();
                b.pump();
                assert!(Instant::now() < deadline, "MD Node held retry deadline");
                md_netplay_fixture_yield().await;
                host_turns += 1;
            }
        } else if frame == 1 {
            let state = delivery.state();
            assert!(state.high_water <= 128);
            assert_eq!(delivery.release_input(), state.held);
            assert_eq!(delivery.state().buffered, state.buffered);
            let deadline = Instant::now() + Duration::from_secs(90);
            while delivery.state().buffered != 0 {
                a.pump();
                b.pump();
                assert!(
                    Instant::now() < deadline,
                    "MD Node old retry drain deadline"
                );
                md_netplay_fixture_yield().await;
                host_turns += 1;
            }
            assert_eq!(a.frames.len(), frame);
            delivery.hold_input();
            assert_eq!(delivery.state().held, 0);
        }
        let refused_before = delivery.state().refused;
        b.step(two_inputs[frame]);
        if frame == 0 {
            let deadline = Instant::now() + Duration::from_secs(90);
            while b.stepped < 1 {
                a.pump();
                b.pump();
                assert!(
                    Instant::now() < deadline,
                    "MD Node refused input completion deadline"
                );
                md_netplay_fixture_yield().await;
                host_turns += 1;
            }
            assert!(delivery.state().refused > refused_before);
            assert_eq!(delivery.state().held, paired::INPUT_CAPACITY);
        } else if frame == 1 {
            let deadline = Instant::now() + Duration::from_secs(90);
            while b.stepped < frame as u64 + 1 || !delivery.state().duplicate_latest {
                a.pump();
                b.pump();
                assert!(
                    Instant::now() < deadline,
                    "MD Node duplicate input deadline"
                );
                md_netplay_fixture_yield().await;
                host_turns += 1;
            }
        }
        a.pump();
        assert_eq!(a.frames.len(), frame);
        let state = delivery.state();
        assert!(state.high_water <= 128);
        let released = delivery.release_input();
        assert_eq!(released, state.held);
        assert_eq!(delivery.state().buffered, state.buffered);
        assert!(released > 0);
        if frame <= 1 {
            wasm_bindgen_test::console_log!(
                "MD_NODE_NETPLAY_DELIVERY frame={frame} held={released} refused={} high_water={} duplicate_latest={}",
                state.refused,
                state.high_water,
                state.duplicate_latest
            );
        }
        let deadline = Instant::now() + Duration::from_secs(90);
        while a.frames.len() < frame + 1
            || b.frames.len() < frame + 1
            || b.stepped < frame as u64 + 1
        {
            a.pump();
            b.pump();
            assert!(
                Instant::now() < deadline,
                "MD Node confirmed field completion deadline"
            );
            md_netplay_fixture_yield().await;
            host_turns += 1;
        }
        assert_eq!(b.stepped, frame as u64 + 1);
    }
    assert!(a.rollback > 0);
    assert_eq!(a.frames, b.frames);
    assert_eq!(a.frames.len(), 6);
    assert!(host_turns > 0);
    wasm_bindgen_test::console_log!(
        "MD_NODE_NETPLAY_HOST_TURNS region={region:?} timing={timing:?} kind={kind:?} pending_turns={host_turns}"
    );
    let a_raw = a
        .thread
        .inner
        .borrow()
        .backend
        .encode_state_bytes()
        .unwrap();
    let b_raw = b
        .thread
        .inner
        .borrow()
        .backend
        .encode_state_bytes()
        .unwrap();
    let frames = std::mem::take(&mut a.frames);
    a.stop();
    b.stop();
    for (index, (checkpoint, ports, pcm)) in frames.iter().enumerate() {
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
    }
    assert_eq!(a_raw, reference.encode_state_bytes().unwrap());
    assert_eq!(b_raw, reference.encode_state_bytes().unwrap());
}

#[wasm_bindgen_test]
async fn md_normal_node_workers_deliver_authenticated_datagram_corrections_and_restore() {
    run_pair(
        Region::NtscUs,
        TimingModel::Standard,
        ControllerKind::ThreeButton,
    )
    .await;
    for kind in [ControllerKind::ThreeButton, ControllerKind::SixButton] {
        run_pair(Region::NtscUs, TimingModel::Precise, kind).await;
    }
}

#[wasm_bindgen_test]
async fn md_normal_node_workers_cross_all_twelve_loaded_configurations_and_both_roles() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            for kind in [ControllerKind::ThreeButton, ControllerKind::SixButton] {
                run_pair(region, timing, kind).await;
            }
        }
    }
}

#[wasm_bindgen_test]
async fn md_node_worker_preserves_dirty_epoch_and_pending_markers_and_fences_changed_host() {
    let backend = fixture::loaded(fixture::config(
        Region::NtscUs,
        TimingModel::Standard,
        ControllerKind::ThreeButton,
    ));
    let mut worker = Worker::new(backend);
    let mut peer = Worker::new(fixture::loaded(fixture::config(
        Region::NtscUs,
        TimingModel::Standard,
        ControllerKind::ThreeButton,
    )));
    let (local, remote, _) = paired::pair();
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
        md_netplay_fixture_yield().await;
    }
    worker.step(fixture::ONE[0]);
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut host_turns = 0usize;
    while worker.stepped < 1 {
        worker.pump();
        peer.pump();
        assert!(
            Instant::now() < deadline,
            "MD Node host-fence field completion deadline"
        );
        md_netplay_fixture_yield().await;
        host_turns += 1;
    }
    assert_eq!(worker.stepped, 1);
    assert!(host_turns > 0);
    wasm_bindgen_test::console_log!(
        "MD_NODE_NETPLAY_HOST_TURNS phase=host-fence pending_turns={host_turns}"
    );
    assert_eq!(worker.thread.inner.borrow().backend.frame_count(), 1);
    assert!(worker.prediction > 0);
    worker.thread.poll_storage();
    {
        let mut inner = worker.thread.inner.borrow_mut();
        worker.host.verify(&inner).unwrap();
        assert!(inner.backend.flush_battery_sram().is_err());
        inner.battery_flush_requested = !inner.battery_flush_requested;
    }
    worker.thread.send(EmuCommand::StopNetplay);
    {
        let mut inner = worker.thread.inner.borrow_mut();
        assert!(inner.netplay_restore_failed);
        assert!(fixture::md(&inner.backend).rollback_owned());
        assert!(!fixture::md(&inner.backend).host_persistence_enabled());
        worker.core.verify_restoring(&mut inner.backend);
        assert!(worker.host.dirty == inner.battery_dirty);
        assert_eq!(
            worker.host.discontinuities,
            inner.pending_audio_discontinuities
        );
        assert!(inner.pending_responses.iter().any(|response| matches!(
            response,
            EmuResponse::Netplay(Response::Stopped {
                restored: false,
                ..
            })
        )));
    }
    peer.stop();
    let blocked = worker
        .thread
        .inner
        .borrow()
        .backend
        .encode_state_bytes()
        .unwrap();
    worker.thread.send(EmuCommand::Reset);
    assert_eq!(
        worker
            .thread
            .inner
            .borrow()
            .backend
            .encode_state_bytes()
            .unwrap(),
        blocked
    );
}

#[wasm_bindgen_test]
fn md_node_missing_and_invalid_bundle_identity_cannot_create_an_artifact_identity() {
    use wasm_bindgen::JsValue;
    struct Restore(JsValue);
    impl Drop for Restore {
        fn drop(&mut self) {
            js_sys::Reflect::set(&js_sys::global(), &"zeffBoyBundleIdentity".into(), &self.0)
                .unwrap();
        }
    }
    let _restore =
        Restore(js_sys::Reflect::get(&js_sys::global(), &"zeffBoyBundleIdentity".into()).unwrap());
    for value in [
        JsValue::UNDEFINED,
        JsValue::NULL,
        JsValue::TRUE,
        "invalid".into(),
        "0".repeat(64).into(),
    ] {
        js_sys::Reflect::set(&js_sys::global(), &"zeffBoyBundleIdentity".into(), &value).unwrap();
        assert!(crate::netplay::connect::executable_build().is_err());
    }
}
