use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use super::*;
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};

const ROM_PATH: &str = "browser-artifact-qualification.nes";
const SAVE_PATH: &str = "browser-artifact-qualification.sav";

pub(super) fn backend(bytes: &[u8]) -> Result<EmuBackend> {
    let path = Path::new(ROM_PATH);
    Ok(load_backend_from_rom_source(
        ActiveSystem::Nes,
        path,
        path,
        Some(bytes.to_vec()),
        BackendLoadConfig {
            sample_rate: Some(48_000),
            initial_input: None,
            netplay_browser_media: crate::emu_backend::loader::NetplayRomMedia::browser(bytes),
            ..Default::default()
        },
    )?
    .backend)
}

pub(super) async fn prepare(bytes: &[u8], supplied_rom: bool) -> Result<Vec<u8>> {
    let initial = backend(bytes)?;
    let battery = initial.capabilities().has_battery;
    let persistent = initial
        .nes()
        .context("NES required")?
        .emu
        .dump_persistent_data();
    if battery {
        ensure!(
            persistent.is_some_and(|bytes| bytes.len() == 8192),
            "qualification requires 8 KiB battery SRAM"
        );
        drop(initial);
        return seed(bytes).await;
    }
    ensure!(
        supplied_rom && persistent.is_none(),
        "generated fixtures require battery SRAM"
    );
    check_sram(zeff_firmware::sha256_bytes(bytes), &[])?;
    Ok(Vec::new())
}

fn check_sram(media: [u8; 32], persistent: &[u8]) -> Result<()> {
    let expected = if persistent.is_empty() {
        None
    } else {
        Some(persistent)
    };
    if expected.is_none() {
        ensure!(
            !crate::platform::save_data_exists(Path::new(SAVE_PATH)),
            "unexpected legacy browser SRAM"
        );
    }
    ensure!(
        crate::platform::read_sram_data(Path::new(SAVE_PATH), "nes", media, "sram")?.as_deref()
            == expected,
        "committed browser SRAM differs"
    );
    Ok(())
}

async fn seed(bytes: &[u8]) -> Result<Vec<u8>> {
    let persistent = vec![0x53; 8192];
    let hash = zeff_firmware::sha256_bytes(bytes);
    let (_, writes) = crate::platform::capture_save_writes(|| {
        crate::platform::write_sram_data("nes", hash, "sram", &persistent)
    })?;
    let completion = Rc::new(RefCell::new(None));
    crate::platform::commit_save_writes(writes, completion.clone());
    let started = Instant::now();
    loop {
        if let Some(result) = completion.borrow_mut().take() {
            result.map_err(anyhow::Error::msg)?;
            break;
        }
        ensure!(
            started.elapsed() < Duration::from_secs(5),
            "SRAM seed deadline"
        );
        qualification_yield().await;
    }
    Ok(persistent)
}

pub(super) fn check_restored(
    thread: &EmuThread,
    original: &[u8],
    runtime: &[u8],
    persistent: &[u8],
) -> Result<()> {
    let inner = thread.inner.borrow();
    ensure!(
        inner.backend.encode_state_bytes()? == original,
        "browser native state restoration differs"
    );
    let nes = inner.backend.nes().context("NES required")?;
    ensure!(
        nes.emu.encode_rollback_runtime_state() == runtime,
        "browser runtime restoration differs"
    );
    ensure!(
        nes.emu.dump_persistent_data().unwrap_or_default() == persistent,
        "browser persistent restoration differs"
    );
    ensure!(
        nes.host_persistence_enabled() && inner.pending_storage.is_none(),
        "browser publication/storage fence differs"
    );
    let published = thread
        .shared_framebuffer
        .load_full()
        .context("restored framebuffer absent")?;
    ensure!(
        published.as_slice() == inner.backend.display_framebuffer().0,
        "restored framebuffer publication differs"
    );
    check_sram(inner.backend.rom_hash(), persistent)?;
    Ok(())
}

pub(super) async fn shutdown(thread: &EmuThread, bytes: &[u8], persistent: &[u8]) -> Result<()> {
    thread.send(EmuCommand::Shutdown);
    let started = Instant::now();
    let mut published = false;
    let mut shutdown = false;
    while !shutdown {
        while let Some(response) = thread.try_recv_response() {
            match response {
                EmuResponse::SramFlushed(path) => {
                    ensure!(
                        if persistent.is_empty() {
                            path.is_none()
                        } else {
                            path.as_deref().is_none_or(|path| path == SAVE_PATH)
                        },
                        "browser SRAM publication path differs"
                    );
                    let inner = &mut *thread.inner.borrow_mut();
                    let (_, writes) = crate::platform::capture_save_writes(|| {
                        inner.backend.flush_battery_sram()?;
                        inner.recovery.capture_generation_write(&inner.backend)
                    })?;
                    ensure!(
                        !writes.is_empty() && crate::platform::save_writes_are_committed(&writes),
                        "SRAM/generation commit witness absent"
                    );
                    ensure!(
                        !persistent.is_empty() || writes.len() == 1,
                        "no-battery shutdown wrote SRAM"
                    );
                    check_sram(zeff_firmware::sha256_bytes(bytes), persistent)?;
                    published = true;
                }
                EmuResponse::ShutdownComplete => {
                    ensure!(published, "browser shutdown preceded SRAM acknowledgment");
                    shutdown = true;
                }
                EmuResponse::Netplay(Response::Stopped { restored, .. }) => {
                    ensure!(restored, "browser failure cleanup did not restore");
                }
                EmuResponse::Netplay(Response::NetworkStats(_)) => {}
                _ => anyhow::bail!("unexpected browser shutdown response"),
            }
        }
        ensure!(
            started.elapsed() < Duration::from_secs(5),
            "browser shutdown deadline"
        );
        qualification_yield().await;
    }
    check_sram(zeff_firmware::sha256_bytes(bytes), persistent)?;
    ensure!(
        thread.inner.borrow().pending_storage.is_none(),
        "browser storage still pending after shutdown"
    );
    Ok(())
}
