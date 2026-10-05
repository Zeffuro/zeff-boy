use super::*;
use zeff_ws_core::emulator::link_pair::Endpoint;

fn load(bytes: &[u8], color: bool, zipped: bool) -> EmuBackend {
    let name = if color {
        "browser-pair.wsc"
    } else {
        "browser-pair.ws"
    };
    let (path, rom, bytes) = source(bytes, name, zipped);
    load_backend_from_rom_source(
        ActiveSystem::WonderSwan,
        &path,
        &rom,
        Some(bytes.clone()),
        BackendLoadConfig {
            sample_rate: Some(48_000),
            netplay_browser_media: crate::emu_backend::loader::NetplayRomMedia::browser(&bytes),
            ..Default::default()
        },
    )
    .unwrap()
    .backend
}

async fn seed(bytes: &[u8]) -> Vec<u8> {
    let persistent = vec![0x53; 32 * 1024];
    let (_, writes) = crate::platform::capture_save_writes(|| {
        crate::platform::write_sram_data(
            "ws",
            zeff_firmware::sha256_bytes(bytes),
            "sram",
            &persistent,
        )
    })
    .unwrap();
    let completion = Rc::new(RefCell::new(None));
    crate::platform::commit_save_writes(writes, completion.clone());
    let started = Instant::now();
    loop {
        if let Some(result) = completion.borrow_mut().take() {
            result.unwrap();
            return persistent;
        }
        assert!(started.elapsed() < Duration::from_secs(5));
        netplay_test_yield().await;
    }
}

fn input(frame: u64, endpoint: usize) -> u16 {
    [[0x0715, 0x02a9], [0x06ea, 0x0154], [0x0523, 0x03dc]][frame as usize % 3][endpoint]
}

fn step(threads: &[EmuThread; 2], observed: &mut [Observation; 2], roles: [usize; 2]) {
    for index in 0..2 {
        observed[index].drain(&threads[index]);
        assert!(
            !observed[index].stopped,
            "{:?}",
            observed[index].stop_reason
        );
        if !observed[index].pending {
            threads[index].send(EmuCommand::StepNetplay(input(
                observed[index].presented,
                roles[index],
            )));
            observed[index].pending = true;
        }
    }
}

async fn case(color: bool, frames_delay: u64, host_is_zero: bool) {
    let bytes = crate::emu_backend::ws::netplay_fixture_rom(color);
    let persistent = seed(&bytes).await;
    let first = load(&bytes, color, false);
    let original = first.encode_state_bytes().unwrap();
    let original_runtime = first.ws().unwrap().netplay_runtime_state_bytes().unwrap();
    assert_eq!(
        first.ws().unwrap().netplay_persistent_state_bytes(),
        persistent
    );
    let delay = InputDelay::new(frames_delay).unwrap();
    let id = identity::identity_with_delay(&first, [7; 32], delay).unwrap();
    let [host, guest] = peers(&first, delay).await;
    let roles = if host_is_zero { [0, 1] } else { [1, 0] };
    let threads = [
        EmuThread::spawn(first, false),
        EmuThread::spawn(load(&bytes, color, true), false),
    ];
    let mut starts = [Some(host), Some(guest)];
    let mut observed = [Observation::default(), Observation::default()];
    for index in if host_is_zero { [0, 1] } else { [1, 0] } {
        threads[index].send(EmuCommand::StartNetplay(Box::new(Start {
            stream: starts[index].take().unwrap(),
            player: if roles[index] == 0 {
                Player::One
            } else {
                Player::Two
            },
            build: [7; 32],
            secret: [9; 32],
            scope: ConnectionScope::TrustedPrivate,
            allow_different_versions: false,
            verify_every_frame: true,
            input_delay: delay,
        })));
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
        original
    );
    assert!(threads[0].inner.borrow().pending_storage.is_none());
    for frame in 0..6 {
        threads[0].send(EmuCommand::StepNetplay(input(frame, roles[0])));
        observed[0].pending = true;
        let started = Instant::now();
        while observed[0].pending {
            for index in 0..2 {
                observed[index].drain(&threads[index]);
                assert!(!observed[index].stopped);
            }
            assert!(started.elapsed() < Duration::from_secs(5));
            netplay_test_yield().await;
        }
        assert_eq!(observed[0].presented, frame + 1);
    }
    let started = Instant::now();
    while !observed.iter().all(|peer| peer.frames.len() >= 72) {
        step(&threads, &mut observed, roles);
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "WS color={color}/delay={frames_delay}/host-zero={host_is_zero}, presented={:?}, confirmed={:?}",
            observed.each_ref().map(|peer| peer.presented),
            observed.each_ref().map(|peer| peer.frames.len())
        );
        netplay_test_yield().await;
    }
    let mut references = [load(&bytes, color, false), load(&bytes, color, true)];
    let mut leases = [
        references[0]
            .ws()
            .unwrap()
            .begin_netplay_rollback(if roles[0] == 0 {
                Endpoint::Zero
            } else {
                Endpoint::One
            })
            .unwrap(),
        references[1]
            .ws()
            .unwrap()
            .begin_netplay_rollback(if roles[1] == 0 {
                Endpoint::Zero
            } else {
                Endpoint::One
            })
            .unwrap(),
    ];
    let mut different_audio = false;
    for frame in 0..72_u64 {
        let ports = if frame < frames_delay {
            [0, 0]
        } else {
            [
                input(frame - frames_delay, 0),
                input(frame - frames_delay, 1),
            ]
        };
        let mut audio = [Vec::new(), Vec::new()];
        for index in 0..2 {
            let EmuBackend::Ws(backend) = &mut references[index] else {
                unreachable!()
            };
            audio[index] = leases[index].advance_frame(backend, ports).unwrap();
            let snapshot = leases[index].capture(backend).unwrap();
            let expected = identity::checkpoint_with_snapshot(
                &references[index],
                frame + 1,
                &audio[index],
                id.config,
                None,
                Some(&snapshot),
            )
            .unwrap();
            assert_eq!(observed[index].frames[frame as usize].0, expected);
            assert_eq!(observed[index].frames[frame as usize].1, ports);
            assert_eq!(observed[index].frames[frame as usize].2, audio[index]);
        }
        assert_eq!(
            observed[0].frames[frame as usize].0,
            observed[1].frames[frame as usize].0
        );
        different_audio |= audio[0] != audio[1];
        reference::service_peers(&threads, &mut observed).await;
    }
    assert!(different_audio);
    assert!(observed.iter().any(|peer| peer.replayed > 0));
    assert!(
        observed[0]
            .frames
            .iter()
            .any(|(_, _, audio)| audio.iter().any(|sample| *sample != 0.0))
    );
    assert_ne!(
        references[0].encode_state_bytes().unwrap(),
        references[1].encode_state_bytes().unwrap()
    );
    assert_ne!(references[0].framebuffer(), references[1].framebuffer());
    for index in 0..2 {
        let final_input = input(71 - frames_delay, 1 - roles[index]);
        let received = 0x40
            | (((final_input >> 8) as u8 & 1) << 2)
            | (((final_input >> 9) as u8 & 1) << 3)
            | (((final_input >> 10) as u8 & 1) << 1);
        assert_eq!(
            references[index].ws().unwrap().emu.system_ram()[0x203],
            received
        );
    }
    threads[0].send(EmuCommand::SetNetplayPaused(true));
    let started = Instant::now();
    while !observed.iter().all(|peer| peer.paused) {
        step(&threads, &mut observed, roles);
        assert!(started.elapsed() < Duration::from_secs(5));
        netplay_test_yield().await;
    }
    let paused = observed.each_ref().map(|peer| peer.presented);
    threads[0].send(EmuCommand::SendNetplayChat("paired pause".into()));
    let started = Instant::now();
    while observed[1].chat == 0 {
        for index in 0..2 {
            observed[index].drain(&threads[index]);
        }
        assert!(started.elapsed() < Duration::from_secs(3));
        netplay_test_yield().await;
    }
    assert_eq!(observed.each_ref().map(|peer| peer.presented), paused);
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
        let backend = inner.backend.ws().unwrap();
        assert_eq!(
            backend.netplay_runtime_state_bytes().unwrap(),
            original_runtime
        );
        assert_eq!(backend.netplay_persistent_state_bytes(), persistent);
        assert!(backend.host_persistence_enabled());
        assert!(inner.pending_storage.is_none());
    }
    assert_eq!(
        crate::platform::read_sram_data(
            Path::new("browser-pair.sav"),
            "ws",
            zeff_firmware::sha256_bytes(&bytes),
            "sram"
        )
        .unwrap()
        .unwrap(),
        persistent
    );
}

async fn cases(color: bool) {
    crate::platform::init_storage().await;
    for delay in [0, 2, 3] {
        for host_is_zero in [true, false] {
            case(color, delay, host_is_zero).await;
        }
    }
}

#[wasm_bindgen_test(async)]
async fn browser_netplay_ws_worker_replicates_link_and_restores() {
    cases(false).await;
}

#[wasm_bindgen_test(async)]
async fn browser_netplay_wsc_worker_replicates_link_and_restores() {
    cases(true).await;
}
