use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use zeff_nes_core::hardware::cartridge::TimingMode;
use zeff_nes_core::save_state::{
    NES_SAVE_STATE_FORMAT_VERSION, TAS_DETERMINISM_ABI_ID, TAS_STATE_FORMAT_COMPATIBILITY_ID,
};
use zeff_netplay::rollback::InputDelay;
use zeff_netplay::wire::{Identity, Message};

use crate::emu_backend::nes::NesPersistentLoadOutcome;
use crate::emu_backend::{ActiveSystem, EmuBackend};

const CHECKPOINT_ABI: &[u8] = b"ZeffNetplay-NES-replay-v10/mapper-runtime-v1";
const CONFIG_DOMAIN: &[u8] = b"ZeffNetplay-App-native-NES/two-standard-controllers/delay0to8-predict8/runtime-snapshot2/hash60/48000/stereo-f32LE/discard-and-restore/v7";

fn session_config(
    timing: TimingMode,
    media_policy: [u8; 32],
    delay: InputDelay,
) -> Result<[u8; 32]> {
    let timing_tag = match timing {
        TimingMode::Ntsc => 0u8,
        TimingMode::Pal => 1,
        TimingMode::Dendy => 2,
        TimingMode::MultiRegion => anyhow::bail!("netplay requires resolved NES timing"),
    };
    let mut config = Sha256::new();
    config.update(CONFIG_DOMAIN);
    config.update([timing_tag]);
    config.update(delay.frames().to_le_bytes());
    config.update(zeff_netplay::rollback::PREDICTION_WINDOW.to_le_bytes());
    config.update(TAS_DETERMINISM_ABI_ID.as_bytes());
    config.update(TAS_STATE_FORMAT_COMPATIBILITY_ID.as_bytes());
    config.update(media_policy);
    Ok(config.finalize().into())
}

pub(crate) fn identity(backend: &EmuBackend, build: [u8; 32]) -> Result<Identity> {
    identity_with_delay(backend, build, InputDelay::default())
}

pub(crate) fn identity_with_delay(
    backend: &EmuBackend,
    build: [u8; 32],
    delay: InputDelay,
) -> Result<Identity> {
    ensure!(
        backend.system() == ActiveSystem::Nes,
        "netplay requires NES"
    );
    let nes = backend.nes().context("netplay requires NES")?;
    let provenance = backend
        .nes_tas_load_provenance()
        .context("netplay requires loader-owned media provenance")?;
    let load = provenance.load;
    ensure!(load.direct_nes_file, "netplay requires a direct .nes file");
    ensure!(
        !load.any_mod_enabled && !load.any_mod_applied,
        "netplay does not support ROM modifications"
    );
    ensure!(
        load.raw_source_media_sha256 == backend.rom_hash(),
        "netplay source and effective media differ"
    );
    ensure!(
        load.raw_source_media_len >= 16,
        "invalid netplay media length"
    );
    ensure!(
        load.persistent_load != NesPersistentLoadOutcome::Unknown,
        "netplay persistent load outcome is unknown"
    );
    ensure!(
        load.initial_input.buttons == 0 && load.initial_input.dpad == 0,
        "netplay requires neutral initial input"
    );
    ensure!(
        load.initial_sample_rate == 48_000
            && provenance.current_sample_rate == 48_000
            && load
                .configured_sample_rate
                .is_none_or(|rate| rate == 48_000),
        "netplay requires 48000 Hz audio generation"
    );
    ensure!(
        backend.frame_count() == 0 && !backend.is_suspended(),
        "netplay requires an unexecuted running core"
    );
    ensure!(
        load.initial_state_sha256
            .context("netplay initial state is unavailable")?
            == <[u8; 32]>::from(Sha256::digest(backend.encode_state_bytes()?)),
        "netplay core state changed after loading"
    );
    ensure!(
        nes.has_standard_console_hardware(),
        "unsupported NES console"
    );
    let config = session_config(
        nes.emu.resolved_timing_mode(),
        load.sync_config_sha256,
        delay,
    )?;
    ensure!(
        nes.emu.has_full_audio_output_at_rate(48_000),
        "netplay requires full 48000 Hz audio generation"
    );
    ensure!(
        nes.emu.has_default_video_palette(),
        "netplay requires the default video palette"
    );
    ensure!(
        !nes.emu.has_debugger_stop_controls(),
        "netplay does not support debugger stop controls"
    );
    ensure!(
        nes.emu.has_standard_controller_topology(),
        "netplay requires two standard controllers"
    );
    ensure!(
        backend.media_slot_snapshot().is_none() && backend.replay_metadata().firmware.is_empty(),
        "netplay does not support removable media or firmware"
    );
    let persistent = persistent_hash(backend)?;
    ensure!(
        persistent == load.initial_persistent_sha256,
        "netplay persistent state changed after loading"
    );
    Ok(Identity {
        build,
        build_info: super::compatibility::describe(backend, false),
        source: load.raw_source_media_sha256,
        effective: backend.rom_hash(),
        media_len: load.raw_source_media_len,
        config,
        initial: logical_hash(backend, 0, config)?,
        persistent,
        state_format: NES_SAVE_STATE_FORMAT_VERSION,
    })
}

pub(crate) fn checkpoint(
    backend: &EmuBackend,
    frame: u64,
    audio: &[f32],
    config: [u8; 32],
) -> Result<Message> {
    ensure!(
        backend.system() == ActiveSystem::Nes,
        "netplay requires NES"
    );
    ensure!(backend.frame_count() == frame, "netplay core frame drift");
    let mut audio_hash = Sha256::new();
    for sample in audio {
        audio_hash.update(sample.to_bits().to_le_bytes());
    }
    Ok(Message::Checkpoint {
        frame,
        logical: logical_hash(backend, frame, config)?,
        video: Sha256::digest(backend.framebuffer()).into(),
        audio: audio_hash.finalize().into(),
        persistent: persistent_hash(backend)?,
    })
}

fn logical_hash(backend: &EmuBackend, frame: u64, config: [u8; 32]) -> Result<[u8; 32]> {
    let mut digest = Sha256::new();
    digest.update(CHECKPOINT_ABI);
    digest.update(config);
    digest.update(frame.to_le_bytes());
    digest.update(backend.encode_replay_hash_state_bytes()?);
    digest.update(
        backend
            .nes()
            .context("netplay requires NES")?
            .emu
            .encode_rollback_runtime_state(),
    );
    Ok(digest.finalize().into())
}

fn persistent_hash(backend: &EmuBackend) -> Result<[u8; 32]> {
    let nes = backend.nes().context("netplay requires NES")?;
    Ok(Sha256::digest(nes.emu.dump_persistent_data().unwrap_or_default()).into())
}

#[cfg(test)]
mod tests;
