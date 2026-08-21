use std::path::Path;
use std::time::Instant;

use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use crate::link::RemoteLink;
use crate::link::transport::LocalLinkTransport;
use zeff_emu_common::replay::{ReplayEvent, ReplayPlayer};
use zeff_firmware::sha256_hex;

use super::HeadlessOptions;
use super::diagnostics::compare_game_boy_link_event_prefix;
use super::paired_direct::run_direct_paired_replay;
use super::paired_lease::{PairedGameBoyFrameLease, PairedGameBoyFrameLeaseOutcome};
use super::paired_plan::validate_paired_transfer_plan;
use super::paired_transport::{
    game_boy_replay_inbound_schedule, game_boy_replay_state_schedule, local_game_boy_link_pair,
};
use super::timeline::{
    PairedGameBoyReplayTimeline, first_game_boy_peer_present_state, paired_game_boy_replay_timeline,
};
use super::validation::{
    digest_hex, validate_game_boy_replay_start_tick, validate_replay_checkpoint,
    validate_replay_playback,
};

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct LiveLinkPairedGameBoyReplaySummary {
    frames: usize,
    left_start_offset: usize,
    right_start_offset: usize,
    link_activation_frame: usize,
    left_link_activation_frame: usize,
    right_link_activation_frame: usize,
    left_link_activation_tick: Option<u64>,
    right_link_activation_tick: Option<u64>,
    left_replay_frames: usize,
    right_replay_frames: usize,
    replay_tail_frames: usize,
    left_recorded_link_events: usize,
    right_recorded_link_events: usize,
    left_link_events: usize,
    right_link_events: usize,
    stalled_rounds: usize,
    left_final_state_hash: String,
    right_final_state_hash: String,
    left_final_framebuffer_hash: String,
    right_final_framebuffer_hash: String,
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) struct LiveLinkReplayLoad<'a> {
    pub(super) source_path: &'a Path,
    pub(super) rom_path: &'a Path,
    pub(super) rom_data: Vec<u8>,
    pub(super) system: ActiveSystem,
    pub(super) firmware_search_dirs: Vec<std::path::PathBuf>,
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn run_live_link_paired_game_boy_replay_headless(
    load: LiveLinkReplayLoad<'_>,
    left_player: ReplayPlayer,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    if load.system != ActiveSystem::GameBoy {
        anyhow::bail!("--replay-peer-live-link is only supported for GB/GBC replays");
    }
    if opts.expect_replay_final_hash.is_some() {
        anyhow::bail!("--expect-replay-final-hash is not supported with --replay-peer");
    }

    let peer_path = opts
        .replay_peer_path
        .as_ref()
        .expect("run_live_link_paired_game_boy_replay_headless requires replay_peer_path");
    let right_player = ReplayPlayer::load(peer_path)?;
    if opts.replay_tail_frames == 0 {
        ensure_replay_metadata_has_expected_gb_link_events("left", &left_player, opts)?;
        ensure_replay_metadata_has_expected_gb_link_events("right", &right_player, opts)?;
    }
    let left_loaded = load_backend_from_rom_source(
        load.system,
        load.source_path,
        load.rom_path,
        Some(load.rom_data.clone()),
        BackendLoadConfig {
            firmware_search_dirs: load.firmware_search_dirs.clone(),
            gb_use_external_boot_rom: super::replay_uses_external_gb_boot_rom(&left_player),
            ..BackendLoadConfig::default()
        },
    )?;
    let right_loaded = load_backend_from_rom_source(
        load.system,
        load.source_path,
        load.rom_path,
        Some(load.rom_data),
        BackendLoadConfig {
            firmware_search_dirs: load.firmware_search_dirs,
            gb_use_external_boot_rom: super::replay_uses_external_gb_boot_rom(&right_player),
            ..BackendLoadConfig::default()
        },
    )?;

    let start = Instant::now();
    let summary = run_loaded_paired_game_boy_replay_live_link_headless(
        left_loaded.backend,
        right_loaded.backend,
        left_player,
        right_player,
        opts,
    )?;
    println!(
        "[headless] gb-replay-pair-live-link frames={} left_replay_frames={} right_replay_frames={} replay_tail_frames={} left_recorded_link_events={} right_recorded_link_events={} left_link_events={} right_link_events={} stalled_rounds={} left_final_state_sha256={} right_final_state_sha256={} left_final_framebuffer_sha256={} right_final_framebuffer_sha256={} elapsed_ms={}",
        summary.frames,
        summary.left_replay_frames,
        summary.right_replay_frames,
        summary.replay_tail_frames,
        summary.left_recorded_link_events,
        summary.right_recorded_link_events,
        summary.left_link_events,
        summary.right_link_events,
        summary.stalled_rounds,
        summary.left_final_state_hash,
        summary.right_final_state_hash,
        summary.left_final_framebuffer_hash,
        summary.right_final_framebuffer_hash,
        start.elapsed().as_millis()
    );
    if summary.left_start_offset != 0
        || summary.right_start_offset != 0
        || summary.link_activation_frame != 0
    {
        println!(
            "[headless] gb-replay-pair-timeline left_start_offset={} right_start_offset={} link_activation_frame={}",
            summary.left_start_offset, summary.right_start_offset, summary.link_activation_frame
        );
        println!(
            "[headless] gb-replay-pair-link-activation left_frame={} right_frame={}",
            summary.left_link_activation_frame, summary.right_link_activation_frame
        );
        if let (Some(left_tick), Some(right_tick)) = (
            summary.left_link_activation_tick,
            summary.right_link_activation_tick,
        ) {
            println!(
                "[headless] gb-replay-pair-activation-ticks left={} right={}",
                left_tick, right_tick
            );
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn ensure_replay_metadata_has_expected_gb_link_events(
    side: &str,
    player: &ReplayPlayer,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    if opts.expect_gb_link_events == 0 {
        return Ok(());
    }
    let recorded = recorded_game_boy_link_event_count(player);
    if recorded < opts.expect_gb_link_events as usize {
        anyhow::bail!(
            "{side} replay metadata has only {recorded} GB link events; expected at least {}. This replay is not a full trade self-test artifact.",
            opts.expect_gb_link_events
        );
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn recorded_game_boy_link_event_count(player: &ReplayPlayer) -> usize {
    player
        .metadata()
        .events
        .iter()
        .filter(|event| matches!(event, ReplayEvent::GameBoyLink { .. }))
        .count()
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn validate_live_link_paired_final_state_hashes(
    left_player: &ReplayPlayer,
    right_player: &ReplayPlayer,
    timeline: &PairedGameBoyReplayTimeline,
    left_recorded_link_events: usize,
    right_recorded_link_events: usize,
    left_generated_link_events: &[ReplayEvent],
    right_generated_link_events: &[ReplayEvent],
    activation_diagnostics: &[String],
    left_final_state_hash: &str,
    right_final_state_hash: &str,
) -> anyhow::Result<()> {
    let left_expected = left_player.metadata().final_state_sha256.map(|hash| {
        let hex = digest_hex(&hash);
        ("left replay", hex, left_final_state_hash)
    });
    let right_expected = right_player.metadata().final_state_sha256.map(|hash| {
        let hex = digest_hex(&hash);
        ("right replay", hex, right_final_state_hash)
    });
    let mismatches: Vec<_> = [left_expected, right_expected]
        .into_iter()
        .flatten()
        .filter(|(_, expected, actual)| !actual.eq_ignore_ascii_case(expected))
        .map(|(label, expected, actual)| format!("{label}: expected {expected}, got {actual}"))
        .collect();
    if mismatches.is_empty() {
        return Ok(());
    }

    anyhow::bail!(
        "GB live paired replay embedded final state hash mismatch: {}; timeline left_start_offset={} right_start_offset={} link_activation_frame={} total_global_frames={} left_link_events={}/{} right_link_events={}/{} left_frames={} right_frames={}; {}; {}; activation={}",
        mismatches.join("; "),
        timeline.left_start_offset,
        timeline.right_start_offset,
        timeline.link_activation_frame,
        timeline.total_global_frames,
        left_generated_link_events.len(),
        left_recorded_link_events,
        right_generated_link_events.len(),
        right_recorded_link_events,
        left_player.total_frames(),
        right_player.total_frames(),
        compare_game_boy_link_event_prefix("left", left_player, left_generated_link_events),
        compare_game_boy_link_event_prefix("right", right_player, right_generated_link_events),
        activation_diagnostics.join(" | ")
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn validate_live_link_replay_checkpoint(
    side: &str,
    player: &ReplayPlayer,
    backend: &EmuBackend,
    generated_link_events: &[ReplayEvent],
    peer_player: &ReplayPlayer,
    peer_generated_link_events: &[ReplayEvent],
) -> anyhow::Result<()> {
    validate_replay_checkpoint(player, backend).map_err(|error| {
        anyhow::anyhow!(
            "GB live paired replay {side} checkpoint mismatch: {error}; {}; {}",
            compare_game_boy_link_event_prefix(side, player, generated_link_events),
            compare_game_boy_link_event_prefix("peer", peer_player, peer_generated_link_events)
        )
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn run_loaded_paired_game_boy_replay_live_link_headless(
    mut left: EmuBackend,
    mut right: EmuBackend,
    mut left_player: ReplayPlayer,
    mut right_player: ReplayPlayer,
    opts: &HeadlessOptions,
) -> anyhow::Result<LiveLinkPairedGameBoyReplaySummary> {
    let direct_plan = validate_paired_transfer_plan(&left_player, &right_player)?;
    validate_replay_playback(&left_player, &left)?;
    validate_replay_playback(&right_player, &right)?;
    left.load_state_from_bytes(left_player.save_state().to_vec())?;
    right.load_state_from_bytes(right_player.save_state().to_vec())?;
    validate_game_boy_replay_start_tick("left replay", &left, &left_player)?;
    validate_game_boy_replay_start_tick("right replay", &right, &right_player)?;
    let left_frame_base = left.frame_count();
    let right_frame_base = right.frame_count();
    let replay_tail_frames = usize::try_from(opts.replay_tail_frames).unwrap_or(usize::MAX);
    let timeline = paired_game_boy_replay_timeline(&left_player, &right_player, replay_tail_frames);
    if std::env::var_os("ZEFF_BOY_PAIRED_REPLAY_LEGACY").is_none() {
        let direct = run_direct_paired_replay(
            left,
            right,
            left_player,
            right_player,
            direct_plan,
            replay_tail_frames,
        )?;
        if opts.expect_gb_link_events > 0 {
            let expected = opts.expect_gb_link_events as usize;
            if direct.left_generated_link_events.len() < expected
                || direct.right_generated_link_events.len() < expected
            {
                anyhow::bail!(
                    "GB direct paired replay link event count below expectation: expected at least {expected} per side, got left={} right={}",
                    direct.left_generated_link_events.len(),
                    direct.right_generated_link_events.len()
                );
            }
        }
        validate_live_link_paired_final_state_hashes(
            &direct.left_player,
            &direct.right_player,
            &timeline,
            direct.left_recorded_link_events,
            direct.right_recorded_link_events,
            &direct.left_generated_link_events,
            &direct.right_generated_link_events,
            &[],
            &direct.left_final_state_hash,
            &direct.right_final_state_hash,
        )?;
        return Ok(LiveLinkPairedGameBoyReplaySummary {
            frames: timeline.total_global_frames.max(direct.frames),
            left_start_offset: timeline.left_start_offset,
            right_start_offset: timeline.right_start_offset,
            link_activation_frame: timeline.link_activation_frame,
            left_link_activation_frame: timeline.left_link_activation_frame,
            right_link_activation_frame: timeline.right_link_activation_frame,
            left_link_activation_tick: timeline.left_link_activation_tick,
            right_link_activation_tick: timeline.right_link_activation_tick,
            left_replay_frames: direct.left_replay_frames,
            right_replay_frames: direct.right_replay_frames,
            replay_tail_frames,
            left_recorded_link_events: direct.left_recorded_link_events,
            right_recorded_link_events: direct.right_recorded_link_events,
            left_link_events: direct.left_generated_link_events.len(),
            right_link_events: direct.right_generated_link_events.len(),
            stalled_rounds: 0,
            left_final_state_hash: direct.left_final_state_hash,
            right_final_state_hash: direct.right_final_state_hash,
            left_final_framebuffer_hash: direct.left_final_framebuffer_hash,
            right_final_framebuffer_hash: direct.right_final_framebuffer_hash,
        });
    }
    restore_live_pair_replay_link_start_state(&mut left, &left_player);
    restore_live_pair_replay_link_start_state(&mut right, &right_player);
    let left_tick_base = left_player
        .metadata()
        .game_boy_link_start_tick
        .map(|_| left.game_boy_cpu_cycles().unwrap_or(0))
        .unwrap_or(0);
    let right_tick_base = right_player
        .metadata()
        .game_boy_link_start_tick
        .map(|_| right.game_boy_cpu_cycles().unwrap_or(0))
        .unwrap_or(0);
    let (mut left_link, mut right_link) = local_game_boy_link_pair(
        game_boy_replay_inbound_schedule(&left_player, left_frame_base, left_tick_base),
        game_boy_replay_inbound_schedule(&right_player, right_frame_base, right_tick_base),
        game_boy_replay_state_schedule(&left_player, left_frame_base, left_tick_base),
        game_boy_replay_state_schedule(&right_player, right_frame_base, right_tick_base),
    );
    let left_peer_present_start_state = first_game_boy_peer_present_state(&left_player);
    let right_peer_present_start_state = first_game_boy_peer_present_state(&right_player);
    let left_recorded_frames = left_player.total_frames();
    let right_recorded_frames = right_player.total_frames();
    let left_recorded_link_events = recorded_game_boy_link_event_count(&left_player);
    let right_recorded_link_events = recorded_game_boy_link_event_count(&right_player);
    let mut left_frames = 0usize;
    let mut right_frames = 0usize;
    let mut left_final_state_hash = None;
    let mut right_final_state_hash = None;
    let mut left_final_framebuffer_hash = None;
    let mut right_final_framebuffer_hash = None;
    let mut left_generated_link_events = Vec::new();
    let mut right_generated_link_events = Vec::new();
    let mut stalled_rounds = 0usize;
    let mut activation_diagnostics = Vec::new();
    let mut captured_left_activation_before = false;
    let mut captured_right_activation_before = false;
    let mut left_frame_lease = PairedGameBoyFrameLease::default();
    let mut right_frame_lease = PairedGameBoyFrameLease::default();

    let mut global_frame = 0usize;
    while left_frames < timeline.left_target_frames || right_frames < timeline.right_target_frames {
        capture_paired_replay_final_hash_if_due(
            &left,
            left_frames,
            left_recorded_frames,
            &mut left_final_state_hash,
            &mut left_final_framebuffer_hash,
        )?;
        capture_paired_replay_final_hash_if_due(
            &right,
            right_frames,
            right_recorded_frames,
            &mut right_final_state_hash,
            &mut right_final_framebuffer_hash,
        )?;

        let left_should_step =
            global_frame >= timeline.left_start_offset && left_frames < timeline.left_target_frames;
        let right_should_step = global_frame >= timeline.right_start_offset
            && right_frames < timeline.right_target_frames;
        let left_link_enabled =
            left_should_step && global_frame >= timeline.left_link_activation_frame;
        let right_link_enabled =
            right_should_step && global_frame >= timeline.right_link_activation_frame;
        let left_activation_tick = (left_link_enabled
            && global_frame == timeline.left_link_activation_frame)
            .then_some(())
            .and(timeline.left_link_activation_tick)
            .map(|tick| left_tick_base.saturating_add(tick));
        let right_activation_tick = (right_link_enabled
            && global_frame == timeline.right_link_activation_frame)
            .then_some(())
            .and(timeline.right_link_activation_tick)
            .map(|tick| right_tick_base.saturating_add(tick));

        if left_should_step && left_frame_lease.needs_frame_setup() {
            apply_paired_game_boy_frame_boundary_events(&mut left, &mut left_player)?;
        }
        if left_should_step
            && left_frame_lease.needs_frame_setup()
            && global_frame == timeline.left_link_activation_frame
            && let Some((_, tick, state)) = left_peer_present_start_state
            && tick.is_none()
        {
            left.restore_game_boy_link_replay_state(state);
        }
        if left_should_step
            && left_frame_lease.needs_frame_setup()
            && global_frame == timeline.left_link_activation_frame
            && !captured_left_activation_before
        {
            activation_diagnostics.push(format!(
                "left_before_activation global_frame={} replay_frame={} core_frame={} replay_link_state={:?} link_state={:?}",
                global_frame,
                left_frames,
                left.frame_count(),
                left.game_boy_link_replay_state(),
                left.game_boy_link_state()
            ));
            captured_left_activation_before = true;
        }
        let mut left_advanced = step_live_link_replay_side(
            &mut left,
            left_link_enabled.then_some(&mut left_link),
            &mut left_player,
            &mut left_frame_lease,
            left_should_step,
            left_activation_tick,
        )?;
        if left_should_step && left_advanced == 0 && right_link_enabled {
            poll_live_link_replay_side(&mut right, &mut right_link);
            poll_live_link_replay_side(&mut left, &mut left_link);
            left_advanced = step_live_link_replay_side(
                &mut left,
                Some(&mut left_link),
                &mut left_player,
                &mut left_frame_lease,
                true,
                None,
            )?;
        }
        left_frames = left_frames.saturating_add(left_advanced);
        if left_advanced > 0 {
            validate_live_link_replay_checkpoint(
                "left",
                &left_player,
                &left,
                &left_generated_link_events,
                &right_player,
                &right_generated_link_events,
            )?;
        }
        left_generated_link_events.extend(drain_game_boy_link_events(
            &mut left_link,
            left_frame_base,
            left_tick_base,
        ));
        ensure_live_pair_link_connected(&left_link)?;

        if right_should_step && right_frame_lease.needs_frame_setup() {
            apply_paired_game_boy_frame_boundary_events(&mut right, &mut right_player)?;
        }
        if right_should_step
            && right_frame_lease.needs_frame_setup()
            && global_frame == timeline.right_link_activation_frame
            && let Some((_, tick, state)) = right_peer_present_start_state
            && tick.is_none()
        {
            right.restore_game_boy_link_replay_state(state);
        }
        if right_should_step
            && right_frame_lease.needs_frame_setup()
            && global_frame == timeline.right_link_activation_frame
            && !captured_right_activation_before
        {
            activation_diagnostics.push(format!(
                "right_before_activation global_frame={} replay_frame={} core_frame={} replay_link_state={:?} link_state={:?}",
                global_frame,
                right_frames,
                right.frame_count(),
                right.game_boy_link_replay_state(),
                right.game_boy_link_state()
            ));
            captured_right_activation_before = true;
        }
        let mut right_advanced = step_live_link_replay_side(
            &mut right,
            right_link_enabled.then_some(&mut right_link),
            &mut right_player,
            &mut right_frame_lease,
            right_should_step,
            right_activation_tick,
        )?;
        if right_should_step && right_advanced == 0 && left_link_enabled {
            poll_live_link_replay_side(&mut left, &mut left_link);
            poll_live_link_replay_side(&mut right, &mut right_link);
            right_advanced = step_live_link_replay_side(
                &mut right,
                Some(&mut right_link),
                &mut right_player,
                &mut right_frame_lease,
                true,
                None,
            )?;
        }
        right_frames = right_frames.saturating_add(right_advanced);
        if right_advanced > 0 {
            validate_live_link_replay_checkpoint(
                "right",
                &right_player,
                &right,
                &right_generated_link_events,
                &left_player,
                &left_generated_link_events,
            )?;
        }
        right_generated_link_events.extend(drain_game_boy_link_events(
            &mut right_link,
            right_frame_base,
            right_tick_base,
        ));
        ensure_live_pair_link_connected(&right_link)?;

        capture_paired_replay_final_hash_if_due(
            &left,
            left_frames,
            left_recorded_frames,
            &mut left_final_state_hash,
            &mut left_final_framebuffer_hash,
        )?;
        capture_paired_replay_final_hash_if_due(
            &right,
            right_frames,
            right_recorded_frames,
            &mut right_final_state_hash,
            &mut right_final_framebuffer_hash,
        )?;

        if (left_should_step || right_should_step) && left_advanced == 0 && right_advanced == 0 {
            stalled_rounds = stalled_rounds.saturating_add(1);
            if stalled_rounds > 10_000 {
                let left_link_events = left_generated_link_events.len();
                let right_link_events = right_generated_link_events.len();
                anyhow::bail!(
                    "live paired replay made no progress for {stalled_rounds} rounds; left_events={left_link_events} right_events={right_link_events} left_frames={left_frames}/{} right_frames={right_frames}/{} global_frame={global_frame}/{}; {}; {}; activation={}",
                    timeline.left_target_frames,
                    timeline.right_target_frames,
                    timeline.total_global_frames,
                    compare_game_boy_link_event_prefix(
                        "left",
                        &left_player,
                        &left_generated_link_events
                    ),
                    compare_game_boy_link_event_prefix(
                        "right",
                        &right_player,
                        &right_generated_link_events
                    ),
                    activation_diagnostics.join(" | ")
                );
            }
        } else {
            stalled_rounds = 0;
        }
        if left_advanced > 0 || right_advanced > 0 {
            global_frame = global_frame.saturating_add(1);
        }
    }

    if opts.expect_gb_link_events > 0 {
        let expected = opts.expect_gb_link_events as usize;
        let left_link_events = left_generated_link_events.len();
        let right_link_events = right_generated_link_events.len();
        if left_link_events < expected || right_link_events < expected {
            anyhow::bail!(
                "GB live paired replay link event count below expectation: expected at least {expected} per side, got left={left_link_events} right={right_link_events}"
            );
        }
    }

    capture_paired_replay_final_hash_if_due(
        &left,
        left_frames,
        left_recorded_frames,
        &mut left_final_state_hash,
        &mut left_final_framebuffer_hash,
    )?;
    capture_paired_replay_final_hash_if_due(
        &right,
        right_frames,
        right_recorded_frames,
        &mut right_final_state_hash,
        &mut right_final_framebuffer_hash,
    )?;

    let left_final_state_hash = left_final_state_hash
        .ok_or_else(|| anyhow::anyhow!("left replay did not reach its recorded final frame"))?;
    let right_final_state_hash = right_final_state_hash
        .ok_or_else(|| anyhow::anyhow!("right replay did not reach its recorded final frame"))?;
    let left_final_framebuffer_hash =
        left_final_framebuffer_hash.expect("state and framebuffer hashes are captured together");
    let right_final_framebuffer_hash =
        right_final_framebuffer_hash.expect("state and framebuffer hashes are captured together");
    validate_live_link_paired_final_state_hashes(
        &left_player,
        &right_player,
        &timeline,
        left_recorded_link_events,
        right_recorded_link_events,
        &left_generated_link_events,
        &right_generated_link_events,
        &activation_diagnostics,
        &left_final_state_hash,
        &right_final_state_hash,
    )?;

    Ok(LiveLinkPairedGameBoyReplaySummary {
        frames: timeline.total_global_frames,
        left_start_offset: timeline.left_start_offset,
        right_start_offset: timeline.right_start_offset,
        link_activation_frame: timeline.link_activation_frame,
        left_link_activation_frame: timeline.left_link_activation_frame,
        right_link_activation_frame: timeline.right_link_activation_frame,
        left_link_activation_tick: timeline.left_link_activation_tick,
        right_link_activation_tick: timeline.right_link_activation_tick,
        left_replay_frames: left_player
            .total_frames()
            .saturating_sub(left_player.remaining()),
        right_replay_frames: right_player
            .total_frames()
            .saturating_sub(right_player.remaining()),
        replay_tail_frames,
        left_recorded_link_events,
        right_recorded_link_events,
        left_link_events: left_generated_link_events.len(),
        right_link_events: right_generated_link_events.len(),
        stalled_rounds,
        left_final_state_hash,
        right_final_state_hash,
        left_final_framebuffer_hash,
        right_final_framebuffer_hash,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn restore_live_pair_replay_link_start_state(backend: &mut EmuBackend, player: &ReplayPlayer) {
    if let Some(state) = player.metadata().game_boy_link_start_state {
        backend.restore_game_boy_link_replay_state(state);
    } else {
        backend.set_link_peer_present(false);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn apply_paired_game_boy_frame_boundary_events(
    backend: &mut EmuBackend,
    player: &mut ReplayPlayer,
) -> anyhow::Result<()> {
    for event in player.take_events_at_cursor() {
        match event {
            ReplayEvent::GameBoyLinkState { state, .. } => {
                backend.restore_game_boy_link_replay_state(state);
            }
            ReplayEvent::GameBoyLinkStateAtTick { .. } => {}
            ReplayEvent::FdsDiskSide { side, .. } => {
                backend.set_fds_disk_side(side)?;
            }
            ReplayEvent::Media { event, .. } => {
                backend.apply_media_event(&event)?;
            }
            ReplayEvent::GameBoyLink { .. } | ReplayEvent::WonderSwanLink { .. } => {}
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn step_live_link_replay_side<T: crate::link::LinkTransport>(
    backend: &mut EmuBackend,
    link: Option<&mut RemoteLink<T>>,
    player: &mut ReplayPlayer,
    lease: &mut PairedGameBoyFrameLease,
    should_step: bool,
    activation_tick: Option<u64>,
) -> anyhow::Result<usize> {
    if !should_step {
        return Ok(0);
    }

    if lease.needs_frame_setup() {
        let frame = player
            .peek_joypad_frames(0, 1)
            .into_iter()
            .next()
            .unwrap_or_default();
        backend.apply_replay_input(&frame);
        lease.begin(backend, activation_tick).map_err(|error| {
            anyhow::anyhow!("failed to begin paired replay frame lease: {error:?}")
        })?;
    }

    let outcome = match link {
        Some(link) => lease
            .step(backend, Some(&mut *link))
            .map_err(|error| anyhow::anyhow!("live paired replay link failed: {error}"))?,
        None => lease
            .step::<LocalLinkTransport>(backend, None)
            .map_err(|error| anyhow::anyhow!("paired replay frame step failed: {error}"))?,
    };
    match outcome {
        PairedGameBoyFrameLeaseOutcome::FrameComplete => {
            if !player.is_finished() {
                player.advance_frames(1);
            }
            lease.commit_frame();
            Ok(1)
        }
        PairedGameBoyFrameLeaseOutcome::Boundary => Ok(0),
        PairedGameBoyFrameLeaseOutcome::Suspended => {
            anyhow::bail!("live paired replay side suspended before completing its frame")
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn poll_live_link_replay_side<T: crate::link::LinkTransport>(
    backend: &mut EmuBackend,
    link: &mut RemoteLink<T>,
) {
    let result = match link {
        RemoteLink::GameBoy(game_boy_link) => game_boy_link.poll_backend(backend),
        RemoteLink::WonderSwan(_) => unreachable!("paired GB replay only uses Game Boy links"),
    };
    if result.is_err() {
        link.disconnect();
        backend.set_link_peer_present(false);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn capture_paired_replay_final_hash_if_due(
    backend: &EmuBackend,
    advanced_frames: usize,
    recorded_frames: usize,
    final_state_hash: &mut Option<String>,
    final_framebuffer_hash: &mut Option<String>,
) -> anyhow::Result<()> {
    if final_state_hash.is_none() && advanced_frames >= recorded_frames {
        *final_state_hash = Some(sha256_hex(&backend.encode_replay_hash_state_bytes()?));
        *final_framebuffer_hash = Some(sha256_hex(backend.framebuffer()));
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn drain_game_boy_link_events<T: crate::link::LinkTransport>(
    link: &mut RemoteLink<T>,
    frame_base: u64,
    tick_base: u64,
) -> Vec<ReplayEvent> {
    link.take_replay_events()
        .into_iter()
        .filter_map(|event| match event {
            ReplayEvent::GameBoyLink { frame, tick, event } => Some(ReplayEvent::GameBoyLink {
                frame: frame.saturating_sub(frame_base),
                tick: tick.saturating_sub(tick_base),
                event,
            }),
            _ => None,
        })
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn ensure_live_pair_link_connected<T: crate::link::LinkTransport>(
    link: &RemoteLink<T>,
) -> anyhow::Result<()> {
    if link.state() == crate::link::LinkConnectionState::Disconnected {
        anyhow::bail!("live paired replay link disconnected");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
