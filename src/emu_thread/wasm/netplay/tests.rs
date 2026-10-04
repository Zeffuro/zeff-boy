use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use wasm_bindgen::JsValue;
use wasm_bindgen_test::wasm_bindgen_test;
use zeff_netplay::endpoint::ConnectionScope;
use zeff_netplay::lockstep::Player;
use zeff_netplay::rollback::InputDelay;
use zeff_netplay_connect::browser::BrowserLobby;
use zeff_netplay_connect::protocol::{ClientMessage, SessionIdentity, SessionMode, VERSION};

use super::*;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::netplay::{Start, Transport, identity};
use crate::platform::Instant;

#[wasm_bindgen::prelude::wasm_bindgen(
    inline_js = "export function netplay_test_yield() { return new Promise(resolve => setTimeout(resolve, 1)); }"
)]
extern "C" {
    async fn netplay_test_yield();
}

#[wasm_bindgen::prelude::wasm_bindgen(
    inline_js = "export function netplay_test_suspend() { return new Promise(resolve => setTimeout(resolve, 3200)); }"
)]
extern "C" {
    async fn netplay_test_suspend();
}

fn media(timing: u8) -> Vec<u8> {
    let mut bytes = zeff_netplay::fixture::rom();
    bytes[7] = 0x28;
    bytes[10] = 0x70;
    bytes[12] = timing;
    bytes
}

fn backend(bytes: &[u8]) -> EmuBackend {
    let path = Path::new("browser-netplay-proof.nes");
    load_backend_from_rom_source(
        ActiveSystem::Nes,
        path,
        path,
        Some(bytes.to_vec()),
        BackendLoadConfig {
            sample_rate: Some(48_000),
            initial_input: None,
            ..Default::default()
        },
    )
    .unwrap()
    .backend
}

async fn seed(bytes: &[u8]) -> Vec<u8> {
    let persistent = vec![0x53; 8192];
    let hash = zeff_firmware::sha256_bytes(bytes);
    let (_, writes) = crate::platform::capture_save_writes(|| {
        crate::platform::write_sram_data("nes", hash, "sram", &persistent)
    })
    .unwrap();
    let completion = Rc::new(RefCell::new(None));
    crate::platform::commit_save_writes(writes, completion.clone());
    let started = Instant::now();
    loop {
        if let Some(result) = completion.borrow_mut().take() {
            result.unwrap();
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(5));
        netplay_test_yield().await;
    }
    persistent
}

async fn peers(backend: &EmuBackend, delay: InputDelay) -> [Transport; 2] {
    let id = identity::identity_with_delay(backend, [7; 32], delay).unwrap();
    let session = SessionIdentity {
        core: "nes".into(),
        content_hash: const_hex::encode(id.effective),
        compatibility_hash: const_hex::encode(id.config),
        mode: SessionMode::SharedConsole,
    };
    let url = option_env!("ZEFF_BROWSER_TEST_LOBBY_URL").unwrap_or("ws://127.0.0.1:47180/v1/ws");
    let mut host = BrowserLobby::new(
        url,
        &ClientMessage::Create {
            version: VERSION,
            access_token: String::new(),
            identity: session.clone(),
        },
    )
    .unwrap();
    host.open().await.unwrap();
    let mut guest = BrowserLobby::new(
        url,
        &ClientMessage::Join {
            version: VERSION,
            access_token: String::new(),
            room: host.room().unwrap().into(),
            identity: session,
        },
    )
    .unwrap();
    guest.open().await.unwrap();
    let guest_result = Rc::new(RefCell::new(None));
    let completion = guest_result.clone();
    wasm_bindgen_futures::spawn_local(async move {
        *completion.borrow_mut() = Some(guest.establish().await);
    });
    let host = host.establish().await.unwrap();
    let guest = loop {
        if let Some(result) = guest_result.borrow_mut().take() {
            break result.unwrap();
        }
        netplay_test_yield().await;
    };
    [Transport::Browser(host), Transport::Browser(guest)]
}

fn input(frame: u64, player: usize) -> u8 {
    let shift = ((frame / 3 + player as u64 * 3) % 8) as u32;
    (1u8 << shift) ^ (frame.is_multiple_of(5) as u8 * 0x18)
}

#[derive(Default)]
struct Observation {
    ready: bool,
    pending: bool,
    presented: u64,
    frames: Vec<(zeff_netplay::wire::Message, [u8; 2], Vec<f32>)>,
    paused: bool,
    chat: usize,
    stopped: bool,
    rejected: usize,
    shutdown: bool,
}

impl Observation {
    fn drain(&mut self, thread: &EmuThread) {
        while let Some(response) = thread.try_recv_response() {
            match response {
                EmuResponse::Netplay(Response::Ready) => self.ready = true,
                EmuResponse::Netplay(Response::Presented {
                    frame,
                    step_complete,
                    ..
                }) => {
                    self.presented = frame;
                    if step_complete {
                        self.pending = false;
                    }
                }
                EmuResponse::Netplay(Response::Frame {
                    checkpoint,
                    ports,
                    audio,
                }) => self.frames.push((checkpoint, ports, audio)),
                EmuResponse::Netplay(Response::Paused { local, peer, .. }) => {
                    self.paused = local || peer
                }
                EmuResponse::Netplay(Response::Chat { .. }) => self.chat += 1,
                EmuResponse::Netplay(Response::Rejected(_)) => self.rejected += 1,
                EmuResponse::Netplay(Response::Stopped { reason, restored }) => {
                    assert!(restored, "{reason}");
                    self.stopped = true;
                }
                EmuResponse::ShutdownComplete => self.shutdown = true,
                EmuResponse::SramFlushed(_) | EmuResponse::RecoverySaved(_) => {}
                _ => panic!("Unexpected browser netplay response"),
            }
        }
    }
}

async fn case(timing: u8, frames_delay: u64, host_first: bool) {
    let bytes = media(timing);
    let persistent = seed(&bytes).await;
    let first = backend(&bytes);
    assert_eq!(
        first.nes().unwrap().emu.dump_persistent_data().unwrap(),
        persistent
    );
    let original = first.encode_state_bytes().unwrap();
    let delay = InputDelay::new(frames_delay).unwrap();
    let id = identity::identity_with_delay(&first, [7; 32], delay).unwrap();
    let [host, guest] = peers(&first, delay).await;
    let threads = [
        EmuThread::spawn(first, false),
        EmuThread::spawn(backend(&bytes), false),
    ];
    let mut observed = [Observation::default(), Observation::default()];
    let mut starts = [
        Some(Start {
            stream: host,
            player: Player::One,
            build: [7; 32],
            secret: [9; 32],
            scope: ConnectionScope::TrustedPrivate,
            allow_different_versions: false,
            verify_every_frame: true,
            input_delay: delay,
        }),
        Some(Start {
            stream: guest,
            player: Player::Two,
            build: [7; 32],
            secret: [9; 32],
            scope: ConnectionScope::TrustedPrivate,
            allow_different_versions: false,
            verify_every_frame: true,
            input_delay: delay,
        }),
    ];
    for index in if host_first { [0, 1] } else { [1, 0] } {
        threads[index].send(EmuCommand::StartNetplay(Box::new(
            starts[index].take().unwrap(),
        )));
    }
    let started = Instant::now();
    while !observed.iter().all(|peer| peer.ready) {
        for index in 0..2 {
            observed[index].drain(&threads[index]);
            assert!(!observed[index].stopped);
        }
        assert!(started.elapsed() < Duration::from_secs(5));
        netplay_test_yield().await;
    }
    let initial = threads[0]
        .inner
        .borrow()
        .backend
        .encode_state_bytes()
        .unwrap();
    threads[0].send(EmuCommand::Reset);
    threads[0].send(EmuCommand::FlushBatterySram);
    threads[0].send(EmuCommand::SetSampleRate(44_100));
    observed[0].drain(&threads[0]);
    assert_eq!(observed[0].rejected, 3);
    assert_eq!(
        threads[0]
            .inner
            .borrow()
            .backend
            .encode_state_bytes()
            .unwrap(),
        initial
    );
    assert!(threads[0].inner.borrow().pending_storage.is_none());
    let started = Instant::now();
    while !observed.iter().all(|peer| peer.frames.len() >= 72) {
        for index in 0..2 {
            observed[index].drain(&threads[index]);
            assert!(!observed[index].stopped);
            if !observed[index].pending {
                threads[index].send(EmuCommand::StepNetplay(input(
                    observed[index].presented,
                    index,
                )));
                observed[index].pending = true;
            }
        }
        assert!(started.elapsed() < Duration::from_secs(15));
        netplay_test_yield().await;
    }
    let mut reference = backend(&bytes);
    let EmuBackend::Nes(nes) = &mut reference else {
        unreachable!()
    };
    let lease = nes.emu.begin_rollback_session().unwrap();
    for frame in 0..72u64 {
        let ports = if frame < frames_delay {
            [0, 0]
        } else {
            [
                input(frame - frames_delay, 0),
                input(frame - frames_delay, 1),
            ]
        };
        let EmuBackend::Nes(nes) = &mut reference else {
            unreachable!()
        };
        let audio = lease.advance_frame(&mut nes.emu, ports).unwrap();
        let expected = identity::checkpoint(&reference, frame + 1, &audio, id.config).unwrap();
        for peer in &observed {
            assert_eq!(peer.frames[frame as usize].0, expected);
            assert_eq!(peer.frames[frame as usize].1, ports);
            assert_eq!(peer.frames[frame as usize].2, audio);
        }
    }
    threads[0].send(EmuCommand::SetNetplayPaused(true));
    let started = Instant::now();
    while !observed.iter().all(|peer| peer.paused) {
        for index in 0..2 {
            observed[index].drain(&threads[index]);
            assert!(!observed[index].stopped);
            if !observed[index].pending {
                threads[index].send(EmuCommand::StepNetplay(input(
                    observed[index].presented,
                    index,
                )));
                observed[index].pending = true;
            }
        }
        assert!(started.elapsed() < Duration::from_secs(5));
        netplay_test_yield().await;
    }
    let paused_frames = observed.each_ref().map(|peer| peer.presented);
    threads[0].send(EmuCommand::SendNetplayChat("hello".into()));
    let started = Instant::now();
    while observed[1].chat == 0 {
        for index in 0..2 {
            observed[index].drain(&threads[index]);
        }
        assert!(started.elapsed() < Duration::from_secs(3));
        netplay_test_yield().await;
    }
    assert_eq!(
        observed.each_ref().map(|peer| peer.presented),
        paused_frames
    );
    threads[0].send(EmuCommand::SetNetplayPaused(false));
    let started = Instant::now();
    while observed.iter().any(|peer| peer.paused) {
        for index in 0..2 {
            observed[index].drain(&threads[index]);
        }
        assert!(started.elapsed() < Duration::from_secs(3));
        netplay_test_yield().await;
    }
    threads[0].send(EmuCommand::Shutdown);
    threads[1].send(EmuCommand::StopNetplay);
    let started = Instant::now();
    while !observed[0].shutdown {
        observed[0].drain(&threads[0]);
        assert!(started.elapsed() < Duration::from_secs(5));
        netplay_test_yield().await;
    }
    for index in 0..2 {
        observed[index].drain(&threads[index]);
        assert!(observed[index].stopped);
        let inner = threads[index].inner.borrow();
        assert_eq!(inner.backend.encode_state_bytes().unwrap(), original);
        assert_eq!(
            inner
                .backend
                .nes()
                .unwrap()
                .emu
                .dump_persistent_data()
                .unwrap(),
            persistent
        );
        assert!(inner.pending_storage.is_none());
        assert!(inner.backend.nes().unwrap().host_persistence_enabled());
    }
    assert_eq!(
        crate::platform::read_sram_data(
            Path::new("browser-netplay-proof.sav"),
            "nes",
            zeff_firmware::sha256_bytes(&bytes),
            "sram"
        )
        .unwrap()
        .unwrap(),
        persistent
    );
}

#[wasm_bindgen_test(async)]
async fn browser_netplay_actual_worker_matches_reference_and_restores_sram() {
    crate::platform::init_storage().await;
    for timing in [0, 1, 3] {
        for delay in [0, 2] {
            for host_first in [true, false] {
                case(timing, delay, host_first).await;
            }
        }
    }
}

#[wasm_bindgen_test(async)]
async fn browser_netplay_build_mismatch_and_suspended_peer_restore_before_saving() {
    crate::platform::init_storage().await;
    let bytes = media(0);
    let persistent = seed(&bytes).await;
    let original = backend(&bytes).encode_state_bytes().unwrap();
    for mismatch in [true, false] {
        let delay = InputDelay::new(0).unwrap();
        let [host, guest] = peers(&backend(&bytes), delay).await;
        let threads = [
            EmuThread::spawn(backend(&bytes), false),
            EmuThread::spawn(backend(&bytes), false),
        ];
        for (index, transport) in [host, guest].into_iter().enumerate() {
            threads[index].send(EmuCommand::StartNetplay(Box::new(Start {
                stream: transport,
                player: if index == 0 { Player::One } else { Player::Two },
                build: if mismatch && index == 1 {
                    [8; 32]
                } else {
                    [7; 32]
                },
                secret: [9; 32],
                scope: ConnectionScope::TrustedPrivate,
                allow_different_versions: false,
                verify_every_frame: true,
                input_delay: delay,
            })));
        }
        let mut observed = [Observation::default(), Observation::default()];
        if !mismatch {
            let started = Instant::now();
            while !observed.iter().all(|peer| peer.ready) {
                for index in 0..2 {
                    observed[index].drain(&threads[index]);
                    assert!(!observed[index].stopped);
                }
                assert!(started.elapsed() < Duration::from_secs(5));
                netplay_test_yield().await;
            }
            netplay_test_suspend().await;
        }
        let started = Instant::now();
        while !observed.iter().all(|peer| peer.stopped) {
            for index in 0..2 {
                observed[index].drain(&threads[index]);
            }
            assert!(started.elapsed() < Duration::from_secs(5));
            netplay_test_yield().await;
        }
        assert_eq!(observed.each_ref().map(|peer| peer.ready), [!mismatch; 2]);
        for thread in &threads {
            let inner = thread.inner.borrow();
            assert_eq!(inner.backend.encode_state_bytes().unwrap(), original);
            assert!(inner.pending_storage.is_none());
            assert!(inner.backend.nes().unwrap().host_persistence_enabled());
        }
    }
    assert_eq!(
        crate::platform::read_sram_data(
            Path::new("browser-netplay-proof.sav"),
            "nes",
            zeff_firmware::sha256_bytes(&bytes),
            "sram"
        )
        .unwrap()
        .unwrap(),
        persistent
    );
}
