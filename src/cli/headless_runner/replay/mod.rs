use std::path::Path;
use std::time::Instant;

use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use zeff_emu_common::replay::ReplayPlayer;

pub(super) use super::HeadlessOptions;

#[cfg(not(target_arch = "wasm32"))]
mod diagnostics;
mod endpoint;
#[cfg(not(target_arch = "wasm32"))]
mod paired_direct;
#[cfg(not(target_arch = "wasm32"))]
mod paired_lease;
mod paired_legacy;
#[cfg(not(target_arch = "wasm32"))]
mod paired_live;
#[cfg(not(target_arch = "wasm32"))]
mod paired_plan;
#[cfg(not(target_arch = "wasm32"))]
mod paired_transport;
#[cfg(not(target_arch = "wasm32"))]
mod timeline;
mod validation;

#[cfg(test)]
mod tests;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use endpoint::run_loaded_replay_for_verification;

pub(super) fn run_replay_headless(
    source_path: &Path,
    rom_path: &Path,
    rom_data: Vec<u8>,
    system: ActiveSystem,
    firmware_search_dirs: Vec<std::path::PathBuf>,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    let replay_path = opts
        .replay_path
        .as_ref()
        .expect("run_replay_headless requires replay_path");
    let player = ReplayPlayer::load(replay_path)?;
    let gba_use_external_bios = player.metadata().firmware.iter().any(|firmware| {
        matches!(
            firmware,
            zeff_emu_common::replay::ReplayFirmwareManifest::External { firmware_id, .. }
                if firmware_id == "nintendo.gba.bios"
        )
    });
    let gb_use_external_boot_rom = replay_uses_external_gb_boot_rom(&player);
    let sega8_use_external_boot_rom = player.metadata().firmware.iter().any(|firmware| {
        matches!(
            firmware,
            zeff_emu_common::replay::ReplayFirmwareManifest::External { firmware_id, .. }
                if firmware_id == "sega.sms.boot" || firmware_id == "sega.gg.boot"
        )
    });
    let sega8_console_region =
        player
            .metadata()
            .firmware
            .iter()
            .find_map(|firmware| match firmware {
                zeff_emu_common::replay::ReplayFirmwareManifest::External {
                    firmware_id,
                    variant,
                    ..
                } if firmware_id == "sega.sms.boot" => Some(
                    if variant
                        .as_deref()
                        .is_some_and(|variant| variant.ends_with(".japan"))
                    {
                        zeff_sega8_core::hardware::region::Sega8Region::Japanese
                    } else {
                        zeff_sega8_core::hardware::region::Sega8Region::Export
                    },
                ),
                _ => None,
            });
    if opts.replay_peer_path.is_some() {
        if opts.replay_peer_live_link {
            #[cfg(not(target_arch = "wasm32"))]
            return paired_live::run_live_link_paired_game_boy_replay_headless(
                paired_live::LiveLinkReplayLoad {
                    source_path,
                    rom_path,
                    rom_data,
                    system,
                    firmware_search_dirs,
                },
                player,
                opts,
            );
            #[cfg(target_arch = "wasm32")]
            anyhow::bail!("--replay-peer-live-link is native-only");
        }
        return paired_legacy::run_paired_game_boy_replay_headless(rom_data, system, player, opts);
    }
    let loaded = load_backend_from_rom_source(
        system,
        source_path,
        rom_path,
        Some(rom_data),
        BackendLoadConfig {
            firmware_search_dirs,
            gb_use_external_boot_rom,
            gba_use_external_bios,
            sega8_use_external_boot_rom,
            sega8_console_region,
            ..BackendLoadConfig::default()
        },
    )?;

    let start = Instant::now();
    let summary = endpoint::run_loaded_replay_headless(loaded.backend, player, opts)?;
    println!(
        "[headless] replay frames={} events={} gb_link_events={}/{} final_state_sha256={} final_framebuffer_sha256={} elapsed_ms={}",
        summary.frames,
        summary.events_applied,
        summary.game_boy_link_events_delivered,
        summary.game_boy_link_events_total,
        summary.final_state_hash,
        summary.final_framebuffer_hash,
        start.elapsed().as_millis()
    );

    Ok(())
}

fn replay_uses_external_gb_boot_rom(player: &ReplayPlayer) -> bool {
    player.metadata().firmware.iter().any(|firmware| {
        matches!(
            firmware,
            zeff_emu_common::replay::ReplayFirmwareManifest::External { firmware_id, .. }
                if firmware_id == "nintendo.gb.boot.dmg"
                    || firmware_id == "nintendo.gb.boot.cgb"
        )
    })
}
