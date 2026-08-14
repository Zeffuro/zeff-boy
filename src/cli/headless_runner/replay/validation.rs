use crate::emu_backend::{ActiveSystem, EmuBackend};
use zeff_emu_common::replay::{ReplayMetadata, ReplayPlayer};

pub(super) fn digest_hex(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

pub(super) fn validate_embedded_final_state_hash(
    label: &str,
    expected: Option<[u8; 32]>,
    final_state_hash: &str,
) -> anyhow::Result<()> {
    if let Some(expected) = expected {
        let expected = digest_hex(&expected);
        if !final_state_hash.eq_ignore_ascii_case(&expected) {
            anyhow::bail!(
                "{label} embedded final state hash mismatch: expected {expected}, got {final_state_hash}"
            );
        }
    }
    Ok(())
}

pub(super) fn validate_replay_playback(
    player: &ReplayPlayer,
    backend: &EmuBackend,
) -> anyhow::Result<()> {
    validate_replay_metadata(player.metadata(), backend)?;
    validate_replay_input_devices(player, backend)
}

pub(super) fn validate_game_boy_replay_start_tick(
    label: &str,
    backend: &EmuBackend,
    player: &ReplayPlayer,
) -> anyhow::Result<()> {
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

fn validate_replay_metadata(metadata: &ReplayMetadata, backend: &EmuBackend) -> anyhow::Result<()> {
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
    if metadata.rom_sha256 != current.rom_sha256 {
        anyhow::bail!("replay ROM hash differs");
    }
    if metadata.firmware != current.firmware {
        anyhow::bail!("replay firmware differs");
    }
    if metadata.cheat_sha256.is_some() {
        anyhow::bail!(
            "replay requires an enabled cheat set; headless replay cheat restoration is not wired"
        );
    }

    Ok(())
}

fn validate_replay_input_devices(
    player: &ReplayPlayer,
    backend: &EmuBackend,
) -> anyhow::Result<()> {
    if player.uses_zapper_input() && backend.system() != ActiveSystem::Nes {
        anyhow::bail!("replay contains NES Zapper input but current ROM is not a NES game");
    }
    if player.uses_game_boy_link_events() && backend.system() != ActiveSystem::GameBoy {
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
    validate_replay_host_input_frame_shapes(player)?;
    Ok(())
}

fn validate_replay_host_input_frame_shapes(player: &ReplayPlayer) -> anyhow::Result<()> {
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
