use std::time::Instant;

use crate::emu_backend::{ActiveSystem, canonicalize_state_bytes_for_replay_hash};
use zeff_emu_common::replay::ReplayPlayer;
use zeff_firmware::sha256_hex;
use zeff_gb_core::emulator::Emulator as GbEmulator;

use super::HeadlessOptions;
use super::validation::validate_embedded_final_state_hash;

#[derive(Clone, Debug, PartialEq, Eq)]
struct PairedGameBoyReplaySummary {
    frames: usize,
    left_link_completions: usize,
    right_link_completions: usize,
    left_final_state_hash: String,
    right_final_state_hash: String,
    left_final_framebuffer_hash: String,
    right_final_framebuffer_hash: String,
}
pub(super) fn run_paired_game_boy_replay_headless(
    rom_data: Vec<u8>,
    system: ActiveSystem,
    left_player: ReplayPlayer,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    if system != ActiveSystem::GameBoy {
        anyhow::bail!("--replay-peer is only supported for GB/GBC replays");
    }
    if opts.expect_replay_final_hash.is_some() {
        anyhow::bail!("--expect-replay-final-hash is not supported with --replay-peer");
    }

    let peer_path = opts
        .replay_peer_path
        .as_ref()
        .expect("run_paired_game_boy_replay_headless requires replay_peer_path");
    let right_player = ReplayPlayer::load(peer_path)?;
    let start = Instant::now();
    let summary = run_loaded_paired_game_boy_replay_headless(&rom_data, left_player, right_player)?;
    println!(
        "[headless] gb-replay-pair frames={} left_link_completions={} right_link_completions={} left_final_state_sha256={} right_final_state_sha256={} left_final_framebuffer_sha256={} right_final_framebuffer_sha256={} elapsed_ms={}",
        summary.frames,
        summary.left_link_completions,
        summary.right_link_completions,
        summary.left_final_state_hash,
        summary.right_final_state_hash,
        summary.left_final_framebuffer_hash,
        summary.right_final_framebuffer_hash,
        start.elapsed().as_millis()
    );
    Ok(())
}

fn run_loaded_paired_game_boy_replay_headless(
    rom_data: &[u8],
    mut left_player: ReplayPlayer,
    mut right_player: ReplayPlayer,
) -> anyhow::Result<PairedGameBoyReplaySummary> {
    if !left_player.metadata().is_empty() || !right_player.metadata().is_empty() {
        anyhow::bail!("paired GB replay currently expects legacy input-only replay files");
    }

    let mut left = GbEmulator::from_rom_data(
        rom_data,
        zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::Auto,
    )?;
    let mut right = GbEmulator::from_rom_data(
        rom_data,
        zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::Auto,
    )?;
    left.load_state_from_bytes(left_player.save_state().to_vec())?;
    right.load_state_from_bytes(right_player.save_state().to_vec())?;
    left.set_game_boy_link_peer_present(true);
    right.set_game_boy_link_peer_present(true);

    let max_frames = left_player.total_frames().max(right_player.total_frames());
    let mut left_link_completions = 0usize;
    let mut right_link_completions = 0usize;
    for _ in 0..max_frames {
        let left_frame = left_player.next_joypad_frame().unwrap_or_default();
        let right_frame = right_player.next_joypad_frame().unwrap_or_default();
        left.set_input(left_frame.buttons, left_frame.dpad);
        right.set_input(right_frame.buttons, right_frame.dpad);
        left.step_frame();
        right.step_frame();
        if left.sync_game_boy_remote_link_peer(right.game_boy_link_state()) {
            left_link_completions += 1;
        }
        if right.sync_game_boy_remote_link_peer(left.game_boy_link_state()) {
            right_link_completions += 1;
        }
    }

    let mut left_final_state = left.encode_state_bytes()?;
    let mut right_final_state = right.encode_state_bytes()?;
    canonicalize_state_bytes_for_replay_hash(ActiveSystem::GameBoy, &mut left_final_state);
    canonicalize_state_bytes_for_replay_hash(ActiveSystem::GameBoy, &mut right_final_state);
    let left_final_state_hash = sha256_hex(&left_final_state);
    let right_final_state_hash = sha256_hex(&right_final_state);
    validate_embedded_final_state_hash(
        "left replay",
        left_player.metadata().final_state_sha256,
        &left_final_state_hash,
    )?;
    validate_embedded_final_state_hash(
        "right replay",
        right_player.metadata().final_state_sha256,
        &right_final_state_hash,
    )?;

    Ok(PairedGameBoyReplaySummary {
        frames: max_frames,
        left_link_completions,
        right_link_completions,
        left_final_state_hash,
        right_final_state_hash,
        left_final_framebuffer_hash: sha256_hex(left.framebuffer()),
        right_final_framebuffer_hash: sha256_hex(right.framebuffer()),
    })
}
