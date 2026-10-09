use super::*;
use crate::netplay::mixed_fixture::{self as fixture, FRAMES, Frame, MAX_FRAMES, Progress};
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen::prelude::wasm_bindgen(
    inline_js = "export async function gameplayFixtureRequest(path, body) { const r = await fetch('http://127.0.0.1:47180/test/gameplay/' + path, body == null ? {} : { method: 'POST', body }); if (!r.ok) throw new Error('gameplay fixture HTTP ' + r.status); return await r.text(); }"
)]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = gameplayFixtureRequest)]
    fn request(path: &str, body: Option<&str>) -> js_sys::Promise;
}

async fn http(path: &str, body: Option<&str>) -> String {
    JsFuture::from(request(path, body))
        .await
        .unwrap()
        .as_string()
        .unwrap()
}

async fn status() -> Progress {
    let result: Progress = serde_json::from_str(&http("status", None).await).unwrap();
    assert!(result.error.is_none(), "{:?}", result.error);
    result
}

async fn case(timing: u8, delay_frames: u64, native_host: bool) {
    wasm_bindgen_test::console_log!(
        "mixed gameplay timing={timing} delay={delay_frames} native_host={native_host}"
    );
    let bytes = fixture::media(timing);
    let persistent = seed(&bytes).await;
    let local = backend(&bytes);
    let original = local.encode_state_bytes().unwrap();
    let original_runtime = local.nes().unwrap().emu.encode_rollback_runtime_state();
    let delay = InputDelay::new(delay_frames).unwrap();
    let id = identity::identity_with_delay(&local, [7; 32], delay).unwrap();
    let session = SessionIdentity {
        core: "nes".into(),
        content_hash: const_hex::encode(id.effective),
        compatibility_hash: const_hex::encode(id.config),
        mode: crate::netplay::capabilities::session_mode(ActiveSystem::Nes),
    };
    let url = "ws://127.0.0.1:47180/v1/ws";
    let mut lobby = if native_host {
        let room = http("start", Some(&format!("{timing}:{delay_frames}:1:"))).await;
        BrowserLobby::new(
            url,
            &ClientMessage::Join {
                version: VERSION,
                access_token: String::new(),
                room,
                identity: session,
            },
        )
        .unwrap()
    } else {
        let mut host = BrowserLobby::new(
            url,
            &ClientMessage::Create {
                version: VERSION,
                access_token: String::new(),
                identity: session,
            },
        )
        .unwrap();
        host.open().await.unwrap();
        let room = host.room().unwrap().to_owned();
        assert_eq!(
            http("start", Some(&format!("{timing}:{delay_frames}:0:{room}"))).await,
            room
        );
        host
    };
    if native_host {
        lobby.open().await.unwrap();
    }
    let cancellation = lobby.cancellation();
    let peer = lobby.establish().await.unwrap();
    cancellation.cancel();
    let thread = EmuThread::spawn(local, false);
    assert_eq!(
        crate::netplay::compatibility::describe(&thread.inner.borrow().backend, false),
        zeff_netplay::wire::BuildInfo {
            version: env!("CARGO_PKG_VERSION").into(),
            ..zeff_netplay::wire::BuildInfo::default()
        }
    );
    thread.send(EmuCommand::StartNetplay(Box::new(Start {
        stream: Transport::Browser(peer),
        player: if native_host {
            Player::Two
        } else {
            Player::One
        },
        build: [7; 32],
        secret: [9; 32],
        scope: ConnectionScope::TrustedPrivate,
        allow_different_versions: false,
        verify_every_frame: true,
        input_delay: delay,
    })));
    assert_eq!(
        crate::netplay::compatibility::describe(&thread.inner.borrow().backend, false),
        zeff_netplay::wire::BuildInfo {
            version: env!("CARGO_PKG_VERSION").into(),
            ..zeff_netplay::wire::BuildInfo::default()
        }
    );
    let mut observed = Observation::default();
    let started = Instant::now();
    loop {
        observed.drain(&thread);
        assert!(!observed.stopped, "{:?}", observed.stop_reason);
        let native = status().await;
        if observed.ready && native.presented >= 6 {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(5));
        netplay_test_yield().await;
    }
    http("control", Some("release")).await;
    let native = loop {
        observed.drain(&thread);
        assert!(!observed.stopped, "{:?}", observed.stop_reason);
        assert!(observed.frames.len() <= MAX_FRAMES && observed.presented < MAX_FRAMES as u64);
        if !observed.pending && observed.frames.len() < FRAMES {
            thread.send(EmuCommand::StepNetplay(fixture::input(
                observed.presented,
                usize::from(native_host),
            )));
            observed.pending = true;
        }
        let native = status().await;
        if observed.frames.len() >= FRAMES
            && native.verified >= FRAMES
            && !native.pending
            && !observed.pending
        {
            break native;
        }
        assert!(started.elapsed() < Duration::from_secs(20));
        netplay_test_yield().await;
    };
    assert!(native.replayed > 0, "native prediction was not corrected");
    let browser_close_frame = thread.inner.borrow().backend.frame_count();
    if native_host {
        http("control", Some("stop")).await;
    } else {
        thread.send(EmuCommand::StopNetplay);
    }
    let stopped = Instant::now();
    while !observed.stopped {
        observed.drain(&thread);
        assert!(stopped.elapsed() < Duration::from_secs(1));
        netplay_test_yield().await;
    }
    while crate::netplay::network::Network::pending_closes() != 0 {
        assert!(stopped.elapsed() < Duration::from_secs(1));
        netplay_test_yield().await;
    }
    let mut reference = backend(&bytes);
    let lease = reference::ReferenceLease::begin(&mut reference);
    let mut receipts = Vec::new();
    for frame in 0..observed.frames.len() {
        let ports = fixture::ports(frame as u64, delay_frames);
        let pcm = lease.advance(&mut reference, ports);
        let checkpoint =
            identity::checkpoint(&reference, frame as u64 + 1, &pcm, id.config).unwrap();
        let actual = &observed.frames[frame];
        assert_eq!(actual.0, checkpoint);
        assert_eq!(actual.1, ports);
        assert!(
            actual
                .2
                .iter()
                .map(|sample| sample.to_bits())
                .eq(pcm.iter().map(|sample| sample.to_bits()))
        );
        receipts.push(Frame::new(&checkpoint, ports, &pcm).unwrap());
        netplay_test_yield().await;
    }
    {
        let inner = thread.inner.borrow();
        assert_eq!(inner.backend.encode_state_bytes().unwrap(), original);
        assert_eq!(
            inner
                .backend
                .nes()
                .unwrap()
                .emu
                .encode_rollback_runtime_state(),
            original_runtime
        );
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
        assert!(inner.backend.nes().unwrap().host_persistence_enabled());
        assert!(inner.pending_storage.is_none());
    }
    let result = loop {
        let result = status().await;
        if result.finished {
            break result;
        }
        assert!(stopped.elapsed() < Duration::from_secs(5));
        netplay_test_yield().await;
    };
    assert!(result.restored && result.published);
    if native_host {
        assert_eq!(result.stop_reason.as_deref(), Some("local stop"));
        assert_eq!(
            observed.stop_reason.as_deref(),
            Some(format!("netplay peer closed at frame {}", result.closed_at).as_str())
        );
    } else {
        assert_eq!(observed.stop_reason.as_deref(), Some("Disconnected"));
        assert_eq!(
            result.stop_reason.as_deref(),
            Some(format!("netplay peer closed at frame {browser_close_frame}").as_str())
        );
    }
    assert_eq!(result.frames, receipts[..FRAMES]);
    thread.send(EmuCommand::Shutdown);
    let mut published = false;
    let mut shutdown = false;
    while !shutdown {
        while let Some(response) = thread.try_recv_response() {
            match response {
                EmuResponse::SramFlushed(path) => {
                    if let Some(path) = path {
                        assert_eq!(path, "browser-netplay-proof.sav");
                    }
                    let inner = &mut *thread.inner.borrow_mut();
                    let (_, writes) = crate::platform::capture_save_writes(|| {
                        inner.backend.flush_battery_sram()?;
                        inner.recovery.capture_generation_write(&inner.backend)
                    })
                    .unwrap();
                    assert!(!writes.is_empty());
                    assert!(crate::platform::save_writes_are_committed(&writes));
                    published = true;
                }
                EmuResponse::ShutdownComplete => {
                    assert!(published);
                    shutdown = true;
                }
                _ => panic!("unexpected mixed browser shutdown response"),
            }
        }
        assert!(stopped.elapsed() < Duration::from_secs(5));
        netplay_test_yield().await;
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
async fn browser_mixed_gameplay_matches_native_worker_and_restores() {
    crate::platform::init_storage().await;
    for timing in [0, 1, 3] {
        for delay in [0, 2] {
            for native_host in [true, false] {
                case(timing, delay, native_host).await;
            }
        }
    }
}
