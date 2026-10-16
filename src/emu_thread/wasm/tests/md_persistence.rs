use super::*;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};

mod transaction_abort;

const SIZES: [usize; 3] = [512, 8192, 32768];

fn rom(size: usize, marker: u8) -> Vec<u8> {
    let mut bytes = vec![0; 0x400];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    bytes[0x1b0..0x1ba].copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20]);
    bytes[0x1ba..0x1bc].copy_from_slice(&((size * 2 - 1) as u16).to_be_bytes());
    let mut program = Vec::new();
    if size == 512 {
        program.extend_from_slice(&[0x13, 0xfc, 0, 1, 0, 0xa1, 0x30, 0xf1]);
    }
    for (address, value) in [
        (0x20_0001u32, 0xa5),
        (0x20_0000 + size as u32 * 2 - 1, 0x5a),
    ] {
        program.extend_from_slice(&[0x13, 0xfc, 0, value]);
        program.extend_from_slice(&address.to_be_bytes());
    }
    program.extend_from_slice(&[0x60, 0xfe]);
    bytes[0x200..0x200 + program.len()].copy_from_slice(&program);
    bytes[0x300] = marker;
    bytes
}

fn load(size: usize, marker: u8) -> anyhow::Result<EmuBackend> {
    let path = std::path::Path::new("browser-save.md");
    Ok(load_backend_from_rom_source(
        ActiveSystem::Md,
        path,
        path,
        Some(rom(size, marker)),
        BackendLoadConfig::default(),
    )?
    .backend)
}

fn save(backend: &EmuBackend) -> Vec<u8> {
    let EmuBackend::Md(backend) = backend else {
        panic!("MD backend expected");
    };
    backend.emu.dump_save_ram().unwrap()
}

fn import(backend: &mut EmuBackend, bytes: &[u8]) {
    let EmuBackend::Md(md) = backend else {
        unreachable!();
    };
    md.emu.load_save_ram(bytes).unwrap();
}

fn pattern(size: usize, salt: u8) -> Vec<u8> {
    (0..size)
        .map(|i| (i as u8).wrapping_mul(37) ^ (i >> 8) as u8 ^ salt)
        .collect()
}

fn step(backend: EmuBackend, expected: &mut [u8]) -> EmuThread {
    let thread = EmuThread::spawn(backend, true);
    thread.send(EmuCommand::StepFrames(Box::new(frame_input())));
    assert!(thread.try_recv_frame().is_some());
    expected[0] = 0xa5;
    *expected.last_mut().unwrap() = 0x5a;
    assert_eq!(save(&thread.inner.borrow().backend), expected);
    thread
}

fn begin_shutdown(thread: &EmuThread) {
    thread.send(EmuCommand::Shutdown);
    assert!(thread.inner.borrow().pending_storage.is_some());
    assert!(thread.try_recv_response().is_none());
    assert!(thread.inner.borrow().battery_potentially_dirty);
}

fn assert_entries(thread: &EmuThread, expected: &[u8], entries: &[(String, Vec<u8>)]) -> u64 {
    let inner = thread.inner.borrow();
    let backend = &inner.backend;
    let identity = backend.rom_hash();
    let component_hash = backend.battery_component_hash();
    let discriminator = backend.recovery_discriminator();
    let key = format!("zeff-sram-v2:md:{}:sram", const_hex::encode(identity));
    let generation_path =
        crate::save_paths::recovery_state::battery_generation_path("md", identity).unwrap();
    let recovery_path = crate::save_paths::recovery_state::recovery_state_path(
        "md",
        backend.system().state_extension(),
        identity,
    )
    .unwrap();
    let mut keys = vec![
        key.clone(),
        format!("zeff-state-{}", generation_path.display()),
        format!("zeff-state-{}", recovery_path.display()),
    ];
    keys.sort();
    assert_eq!(
        entries
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>(),
        keys
    );
    assert_eq!(entries.iter().find(|(k, _)| k == &key).unwrap().1, expected);
    let generations: Vec<_> = entries
        .iter()
        .filter_map(|(_, data)| {
            crate::save_paths::recovery_state::decode_battery_generation(data, identity)
        })
        .collect();
    assert_eq!(generations.len(), 1);
    assert_eq!(generations[0].component_sha256, component_hash);
    let envelopes: Vec<_> = entries
        .iter()
        .filter_map(|(_, data)| {
            crate::save_paths::recovery_state::decode_recovery_state(
                data,
                crate::save_paths::recovery_state::RecoveryStateIdentity {
                    system: "md",
                    discriminator: &discriminator,
                    media_sha256: identity,
                },
            )
            .ok()
        })
        .collect();
    assert_eq!(envelopes.len(), 1);
    assert_eq!(
        envelopes[0].native_payload,
        backend.encode_state_bytes().unwrap()
    );
    assert_eq!(
        envelopes[0].battery,
        crate::save_paths::recovery_state::BatteryGenerationWitness::Committed {
            generation: generations[0].generation,
            component_sha256: component_hash,
        }
    );
    assert!(!inner.battery_potentially_dirty);
    generations[0].generation
}

fn receipt(phase: &str, size: usize, generation: u64, entries: &[(String, Vec<u8>)]) {
    let entries = entries
        .iter()
        .map(|(key, bytes)| {
            serde_json::json!({
                "key": key, "length": bytes.len(),
                "sha256": const_hex::encode(zeff_firmware::sha256_bytes(bytes)),
            })
        })
        .collect::<Vec<_>>();
    wasm_bindgen_test::console_log!(
        "MD_BROWSER_RECEIPT {}",
        serde_json::json!({"phase": phase, "size": size, "generation": generation, "entries": entries})
    );
}

async fn entries() -> Vec<(String, Vec<u8>)> {
    crate::platform::fresh_browser_storage_entries_for_test()
        .await
        .unwrap()
}

async fn clear() {
    crate::platform::clear_browser_storage_for_test()
        .await
        .unwrap();
}

async fn commit(writes: Vec<crate::platform::SaveWrite>) {
    let completion = std::rc::Rc::new(std::cell::RefCell::new(None));
    crate::platform::commit_save_writes(writes, completion.clone());
    let deadline = js_sys::Date::now() + 10_000.0;
    loop {
        if let Some(result) = completion.borrow_mut().take() {
            result.unwrap();
            return;
        }
        assert!(js_sys::Date::now() < deadline);
        browser_test_delay(5).await.unwrap();
    }
}

#[wasm_bindgen_test]
async fn wasm_md_browser_nonvolatile_save_transaction_and_cold_reload() {
    for size in SIZES {
        clear().await;
        let mut expected = pattern(size, 0);
        let mut backend = load(size, 0).unwrap();
        assert!(!backend.has_battery());
        assert!(backend.save_ram_kind().is_persistent());
        import(&mut backend, &expected);
        assert!(backend.flush_battery_sram().is_err());
        assert!(entries().await.is_empty());
        let identity = backend.rom_hash();
        let thread = step(backend, &mut expected);
        begin_shutdown(&thread);
        wait_for_browser_terminal(&thread).await;
        let stored = entries().await;
        let generation = assert_entries(&thread, &expected, &stored);
        receipt("commit-cold-reload", size, generation, &stored);
        drop(thread);
        assert_eq!(save(&load(size, 0).unwrap()), expected);
        assert_eq!(save(&load(size, 1).unwrap()), vec![0xff; size]);
        let wrong = vec![0x42; size - 1];
        let (_, writes) = crate::platform::capture_save_writes(|| {
            crate::platform::write_sram_data(
                "md",
                identity,
                crate::save_paths::SRAM_COMPONENT,
                &wrong,
            )
        })
        .unwrap();
        commit(writes).await;
        let stored = entries().await;
        assert!(load(size, 0).is_err());
        let key = format!("zeff-sram-v2:md:{}:sram", const_hex::encode(identity));
        assert_eq!(stored.iter().find(|(k, _)| k == &key).unwrap().1, wrong);
        assert_eq!(entries().await, stored);
        receipt("wrong-size-rejected", size, generation, &stored);
        clear().await;
        assert_eq!(save(&load(size, 0).unwrap()), vec![0xff; size]);
    }
}

#[wasm_bindgen_test]
async fn wasm_md_browser_transaction_abort_preserves_committed_save_and_retry() {
    for size in SIZES {
        clear().await;
        let mut old = pattern(size, 0);
        let mut backend = load(size, 0).unwrap();
        import(&mut backend, &old);
        let thread = step(backend, &mut old);
        begin_shutdown(&thread);
        wait_for_browser_terminal(&thread).await;
        let committed = entries().await;
        let old_generation = assert_entries(&thread, &old, &committed);
        drop(thread);

        let mut changed = pattern(size, 0x79);
        let mut backend = load(size, 0).unwrap();
        assert_eq!(save(&backend), old);
        import(&mut backend, &changed);
        let thread = step(backend, &mut changed);
        let abort = transaction_abort::AbortNextWrite::arm();
        begin_shutdown(&thread);
        transaction_abort::wait_for_failed_shutdown(&thread).await;
        assert_eq!(abort.count(), 1);
        drop(abort);
        assert!(thread.inner.borrow().pending_storage.is_none());
        assert!(thread.inner.borrow().battery_potentially_dirty);
        assert_eq!(save(&load(size, 0).unwrap()), old);
        assert_eq!(entries().await, committed);
        receipt(
            "abort-retained-old-commit",
            size,
            old_generation,
            &committed,
        );

        begin_shutdown(&thread);
        wait_for_browser_terminal(&thread).await;
        let retried = entries().await;
        let generation = assert_entries(&thread, &changed, &retried);
        assert_eq!(generation, old_generation + 1);
        receipt("retry-cold-reload", size, generation, &retried);
        drop(thread);
        assert_eq!(save(&load(size, 0).unwrap()), changed);
        clear().await;
    }
}
