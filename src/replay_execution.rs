use anyhow::Result;
use zeff_emu_common::replay::{ReplayEvent, ReplayMetadata, ReplayPlayer};
use zeff_firmware::sha256_bytes;

use crate::emu_backend::{ActiveSystem, EmuBackend};

pub(crate) fn validate_replay_playback(player: &ReplayPlayer, backend: &EmuBackend) -> Result<()> {
    validate_replay_metadata(player.metadata(), backend)?;
    validate_replay_input_devices(player, backend)
}

pub(crate) fn validate_game_boy_replay_start_tick(
    label: &str,
    backend: &EmuBackend,
    player: &ReplayPlayer,
) -> Result<()> {
    let Some(expected) = player.metadata().game_boy_link_start_tick else {
        return Ok(());
    };
    let actual = backend.game_boy_cpu_cycles().ok_or_else(|| {
        anyhow::anyhow!("{label} declares a GB start tick but current backend is not Game Boy")
    })?;
    if actual != expected {
        anyhow::bail!("{label} GB start tick mismatch: metadata={expected}, state={actual}");
    }
    Ok(())
}

pub(crate) fn validate_wonder_swan_replay_start_tick(
    label: &str,
    backend: &EmuBackend,
    player: &ReplayPlayer,
) -> Result<()> {
    let Some(expected) = player.metadata().wonder_swan_link_start_tick else {
        return Ok(());
    };
    let actual = backend.wonder_swan_cpu_cycles().ok_or_else(|| {
        anyhow::anyhow!(
            "{label} declares a WonderSwan start tick but current backend is not WonderSwan"
        )
    })?;
    if actual != expected {
        anyhow::bail!(
            "{label} WonderSwan start tick mismatch: metadata={expected}, state={actual}"
        );
    }
    Ok(())
}

pub(crate) fn validate_replay_checkpoint(
    player: &ReplayPlayer,
    backend: &EmuBackend,
) -> Result<()> {
    let frame = u64::try_from(player.cursor()).unwrap_or(u64::MAX);
    validate_replay_checkpoint_at_cursor(player.metadata(), frame, backend)
}

pub(crate) fn validate_replay_checkpoint_at_cursor(
    metadata: &ReplayMetadata,
    cursor: u64,
    backend: &EmuBackend,
) -> Result<()> {
    let checkpoints = metadata
        .checkpoints
        .iter()
        .filter(|checkpoint| checkpoint.frame == cursor);
    for checkpoint in checkpoints {
        let actual = sha256_bytes(&backend.encode_replay_hash_state_bytes()?);
        if actual != checkpoint.state_sha256 {
            anyhow::bail!(
                "replay diverged at checkpoint frame {cursor}: expected {}, got {}",
                const_hex::encode(checkpoint.state_sha256),
                const_hex::encode(actual)
            );
        }
    }
    Ok(())
}

pub(crate) fn apply_replay_events_at_cursor(
    backend: &mut EmuBackend,
    player: &mut ReplayPlayer,
) -> Result<usize> {
    let mut applied = 0;
    for event in player.take_events_at_cursor() {
        applied += usize::from(apply_immediate_replay_event(backend, &event)?);
    }
    Ok(applied)
}

pub(crate) fn apply_immediate_replay_event(
    backend: &mut EmuBackend,
    event: &ReplayEvent,
) -> Result<bool> {
    match event {
        ReplayEvent::FdsDiskSide { side, .. } => {
            backend.set_fds_disk_side(*side)?;
            Ok(true)
        }
        ReplayEvent::Media { event, .. } => {
            backend.apply_media_event(event)?;
            Ok(true)
        }
        ReplayEvent::GameBoyLinkState { state, .. } => {
            if !backend.restore_game_boy_link_replay_state(*state) {
                anyhow::bail!("replay contains an invalid Game Boy link state event");
            }
            Ok(true)
        }
        ReplayEvent::GameBoyLinkStateAtTick { .. }
        | ReplayEvent::GameBoyLink { .. }
        | ReplayEvent::WonderSwanLink { .. } => Ok(false),
    }
}

fn validate_replay_metadata(metadata: &ReplayMetadata, backend: &EmuBackend) -> Result<()> {
    if metadata.is_empty() {
        return Ok(());
    }
    let current = backend.replay_metadata();

    if metadata.system != current.system {
        anyhow::bail!(
            "replay system differs: replay={}, current={}",
            metadata.system.as_deref().unwrap_or("unknown"),
            current.system.as_deref().unwrap_or("unknown")
        );
    }
    if metadata.core_family.is_some() && metadata.core_family != current.core_family {
        anyhow::bail!(
            "replay core family differs: replay={}, current={}",
            metadata.core_family.as_deref().unwrap_or("unknown"),
            current.core_family.as_deref().unwrap_or("unknown")
        );
    }
    if metadata.rom_sha256 != current.rom_sha256 {
        anyhow::bail!("replay ROM hash differs");
    }
    if !zeff_emu_common::replay::firmware_manifests_match(&metadata.firmware, &current.firmware) {
        anyhow::bail!("replay firmware differs");
    }
    if metadata.cheat_sha256.is_some() {
        anyhow::bail!(
            "replay requires an enabled cheat set; deterministic replay execution does not restore cheats"
        );
    }

    Ok(())
}

fn validate_replay_input_devices(player: &ReplayPlayer, backend: &EmuBackend) -> Result<()> {
    if player.version() == 3 && backend.system() != ActiveSystem::Coleco {
        anyhow::bail!("ZRPL v3 controller input records require a ColecoVision game");
    }
    if backend.system() == ActiveSystem::Coleco && player.version() != 3 {
        anyhow::bail!("ColecoVision replay requires ZRPL v3 controller input records");
    }
    if player.uses_zapper_input() && backend.system() != ActiveSystem::Nes {
        anyhow::bail!("replay contains NES Zapper input but current ROM is not a NES game");
    }
    if player.uses_coleco_input() && backend.system() != ActiveSystem::Coleco {
        anyhow::bail!(
            "replay contains ColecoVision controller input but current ROM is not a ColecoVision game"
        );
    }
    if backend.system() == ActiveSystem::Coleco && player.uses_non_coleco_input() {
        anyhow::bail!(
            "ColecoVision replay contains input outside the standard controller/keypad topology"
        );
    }
    if player.uses_game_boy_link() && backend.system() != ActiveSystem::GameBoy {
        anyhow::bail!("replay contains Game Boy link events but current ROM is not a GB/GBC game");
    }
    if player.uses_wonder_swan_link_events() && backend.system() != ActiveSystem::WonderSwan {
        anyhow::bail!(
            "replay contains WonderSwan link events but current ROM is not a WonderSwan game"
        );
    }
    if player.uses_host_tilt_input() && !backend.is_mbc7() {
        anyhow::bail!("replay contains MBC7 tilt input but current ROM is not an MBC7 game");
    }
    if player.uses_host_camera_input() && !backend.is_pocket_camera() {
        anyhow::bail!(
            "replay contains Pocket Camera input but current ROM is not a Pocket Camera game"
        );
    }
    for (frame_index, len) in player.host_camera_frame_lengths() {
        if len != zeff_emu_common::replay::POCKET_CAMERA_FRAME_BYTES {
            anyhow::bail!(
                "replay Pocket Camera frame {frame_index} has {len} bytes, expected {}",
                zeff_emu_common::replay::POCKET_CAMERA_FRAME_BYTES
            );
        }
    }
    Ok(())
}
