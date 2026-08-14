use std::path::Path;
use std::time::Instant;

use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, canonicalize_state_bytes_for_replay_hash,
    load_backend_from_rom_source,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::link::transport::TcpLinkTransport;
#[cfg(not(target_arch = "wasm32"))]
use crate::link::{LinkEndpointId, LinkSession, LinkSystemType, RemoteLink};
use zeff_emu_common::replay::{ReplayEvent, ReplayGameBoyLinkEvent, ReplayMetadata, ReplayPlayer};
use zeff_firmware::sha256_hex;
use zeff_gb_core::emulator::Emulator as GbEmulator;

use super::HeadlessOptions;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReplayHeadlessSummary {
    frames: usize,
    events_applied: usize,
    final_state_hash: String,
    final_framebuffer_hash: String,
}

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
    if opts.replay_peer_path.is_some() {
        if opts.replay_peer_live_link {
            #[cfg(not(target_arch = "wasm32"))]
            return run_live_link_paired_game_boy_replay_headless(
                LiveLinkReplayLoad {
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
        return run_paired_game_boy_replay_headless(rom_data, system, player, opts);
    }
    let loaded = load_backend_from_rom_source(
        system,
        source_path,
        rom_path,
        Some(rom_data),
        BackendLoadConfig {
            firmware_search_dirs,
            ..BackendLoadConfig::default()
        },
    )?;

    let start = Instant::now();
    let summary = run_loaded_replay_headless(loaded.backend, player, opts)?;
    println!(
        "[headless] replay frames={} events={} final_state_sha256={} final_framebuffer_sha256={} elapsed_ms={}",
        summary.frames,
        summary.events_applied,
        summary.final_state_hash,
        summary.final_framebuffer_hash,
        start.elapsed().as_millis()
    );

    Ok(())
}

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
struct LiveLinkReplayLoad<'a> {
    source_path: &'a Path,
    rom_path: &'a Path,
    rom_data: Vec<u8>,
    system: ActiveSystem,
    firmware_search_dirs: Vec<std::path::PathBuf>,
}

#[cfg(not(target_arch = "wasm32"))]
fn run_live_link_paired_game_boy_replay_headless(
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
fn ensure_replay_metadata_has_expected_gb_link_events(
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
fn recorded_game_boy_link_event_count(player: &ReplayPlayer) -> usize {
    player
        .metadata()
        .events
        .iter()
        .filter(|event| matches!(event, ReplayEvent::GameBoyLink { .. }))
        .count()
}

fn run_paired_game_boy_replay_headless(
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
fn compare_game_boy_link_event_prefix(
    label: &str,
    player: &ReplayPlayer,
    generated_events: &[ReplayEvent],
) -> String {
    let recorded_events = recorded_game_boy_link_events(player);
    let compare_len = recorded_events.len().min(generated_events.len());
    if let Some(index) = (0..compare_len).find(|&index| {
        game_boy_link_event_signature(&recorded_events[index])
            != game_boy_link_event_signature(&generated_events[index])
    }) {
        return format!(
            "{label}_first_link_event_mismatch index={} recorded={} generated={}",
            index,
            format_game_boy_link_event_for_diagnostic(&recorded_events[index]),
            format_game_boy_link_event_for_diagnostic(&generated_events[index])
        );
    }

    if generated_events.len() != recorded_events.len() {
        let next_recorded = recorded_events
            .get(compare_len)
            .map(format_game_boy_link_event_for_diagnostic)
            .unwrap_or_else(|| "none".to_string());
        let next_generated = generated_events
            .get(compare_len)
            .map(format_game_boy_link_event_for_diagnostic)
            .unwrap_or_else(|| "none".to_string());
        return format!(
            "{label}_link_event_prefix_match generated={} recorded={} next_recorded={} next_generated={}",
            generated_events.len(),
            recorded_events.len(),
            next_recorded,
            next_generated
        );
    }

    format!("{label}_link_events_match count={}", generated_events.len())
}

#[cfg(not(target_arch = "wasm32"))]
fn recorded_game_boy_link_events(player: &ReplayPlayer) -> Vec<ReplayEvent> {
    player
        .metadata()
        .events
        .iter()
        .filter(|event| matches!(event, ReplayEvent::GameBoyLink { .. }))
        .cloned()
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn game_boy_link_event_signature(event: &ReplayEvent) -> String {
    let ReplayEvent::GameBoyLink { event, .. } = event else {
        return "not_gb_link".to_string();
    };
    normalized_game_boy_link_event_signature(*event)
}

#[cfg(not(target_arch = "wasm32"))]
fn format_game_boy_link_event_for_diagnostic(event: &ReplayEvent) -> String {
    let ReplayEvent::GameBoyLink { frame, tick, event } = event else {
        return "not_gb_link".to_string();
    };
    format!(
        "frame={} tick={} {}",
        frame,
        tick,
        normalized_game_boy_link_event_signature(*event)
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn normalized_game_boy_link_event_signature(event: ReplayGameBoyLinkEvent) -> String {
    match event {
        ReplayGameBoyLinkEvent::LocalMasterStart {
            transfer_id,
            clock_period_t_cycles,
            out_byte,
            serial_generation,
        } => format!(
            "local_master transfer={} out={:02X} period={} gen={}",
            normalized_game_boy_transfer_id(transfer_id),
            out_byte,
            clock_period_t_cycles,
            serial_generation
        ),
        ReplayGameBoyLinkEvent::RemoteMasterStart {
            transfer_id,
            clock_period_t_cycles,
            out_byte,
            serial_generation,
            local_reply,
        } => format!(
            "remote_master transfer={} out={:02X} period={} gen={} reply={}",
            normalized_game_boy_transfer_id(transfer_id),
            out_byte,
            clock_period_t_cycles,
            serial_generation,
            local_reply
                .map(format_game_boy_link_reply_for_diagnostic)
                .unwrap_or_else(|| "none".to_string())
        ),
        ReplayGameBoyLinkEvent::RemoteReply {
            transfer_id,
            out_byte,
            passive,
            serial_generation,
        } => format!(
            "remote_reply transfer={} out={:02X} passive={} gen={}",
            normalized_game_boy_transfer_id(transfer_id),
            out_byte,
            passive,
            serial_generation
        ),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn normalized_game_boy_transfer_id(transfer_id: u64) -> String {
    let endpoint = transfer_id >> 56;
    let counter = transfer_id & 0x00FF_FFFF_FFFF_FFFF;
    format!("ep{endpoint}:{}", counter)
}

#[cfg(not(target_arch = "wasm32"))]
fn format_game_boy_link_reply_for_diagnostic(
    reply: zeff_emu_common::replay::ReplayGameBoyLinkReply,
) -> String {
    format!(
        "out={:02X}/passive={}/gen={}",
        reply.out_byte, reply.passive, reply.serial_generation
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn run_loaded_paired_game_boy_replay_live_link_headless(
    mut left: EmuBackend,
    mut right: EmuBackend,
    mut left_player: ReplayPlayer,
    mut right_player: ReplayPlayer,
    opts: &HeadlessOptions,
) -> anyhow::Result<LiveLinkPairedGameBoyReplaySummary> {
    validate_replay_playback(&left_player, &left)?;
    validate_replay_playback(&right_player, &right)?;
    left.load_state_from_bytes(left_player.save_state().to_vec())?;
    right.load_state_from_bytes(right_player.save_state().to_vec())?;
    validate_game_boy_replay_start_tick("left replay", &left, &left_player)?;
    validate_game_boy_replay_start_tick("right replay", &right, &right_player)?;
    restore_live_pair_replay_link_start_state(&mut left, &left_player);
    restore_live_pair_replay_link_start_state(&mut right, &right_player);

    let (mut left_link, mut right_link) = tcp_game_boy_link_pair()?;
    let replay_tail_frames = usize::try_from(opts.replay_tail_frames).unwrap_or(usize::MAX);
    let timeline = paired_game_boy_replay_timeline(&left_player, &right_player, replay_tail_frames);
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

        if left_should_step {
            apply_paired_game_boy_frame_boundary_events(&mut left, &mut left_player)?;
        }
        if left_should_step
            && global_frame == timeline.left_link_activation_frame
            && let Some((_, state)) = left_peer_present_start_state
        {
            left.restore_game_boy_link_replay_state(state);
        }
        if left_should_step
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
        let left_advanced = step_live_link_replay_side(
            &mut left,
            left_link_enabled.then_some(&mut left_link),
            &mut left_player,
            left_should_step,
            left_activation_tick,
        );
        left_frames = left_frames.saturating_add(left_advanced);
        left_generated_link_events.extend(drain_game_boy_link_events(&mut left_link));
        ensure_live_pair_link_connected(&left_link)?;

        if right_should_step {
            apply_paired_game_boy_frame_boundary_events(&mut right, &mut right_player)?;
        }
        if right_should_step
            && global_frame == timeline.right_link_activation_frame
            && let Some((_, state)) = right_peer_present_start_state
        {
            right.restore_game_boy_link_replay_state(state);
        }
        if right_should_step
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
        let right_advanced = step_live_link_replay_side(
            &mut right,
            right_link_enabled.then_some(&mut right_link),
            &mut right_player,
            right_should_step,
            right_activation_tick,
        );
        right_frames = right_frames.saturating_add(right_advanced);
        right_generated_link_events.extend(drain_game_boy_link_events(&mut right_link));
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
            ReplayEvent::GameBoyLinkState { .. } => {}
            ReplayEvent::FdsDiskSide { side, .. } => {
                backend.set_fds_disk_side(side)?;
            }
            ReplayEvent::GameBoyLink { .. } | ReplayEvent::WonderSwanLink { .. } => {}
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn step_live_link_replay_side(
    backend: &mut EmuBackend,
    link: Option<&mut RemoteLink<TcpLinkTransport>>,
    player: &mut ReplayPlayer,
    should_step: bool,
    activation_tick: Option<u64>,
) -> usize {
    if !should_step {
        return 0;
    }
    let frame = player
        .peek_joypad_frames(0, 1)
        .into_iter()
        .next()
        .unwrap_or_default();
    backend.set_input(frame.buttons, frame.dpad);
    backend.set_input_p2(frame.buttons_p2, frame.dpad_p2);
    let zapper = crate::emu_thread::ZapperInput::from(frame.zapper);
    backend.set_zapper_state(
        zapper.enabled,
        zapper.trigger,
        zapper.hit,
        zapper.screen_pos,
    );
    backend.set_replay_host_tilt(frame.host_tilt);
    if let Some(camera_frame) = frame.camera_frame.as_deref() {
        backend.set_replay_camera_frame(camera_frame);
    }

    let frame_count_before = backend.frame_count();
    if let Some(link) = link {
        let result = match link {
            RemoteLink::GameBoy(game_boy_link) => {
                if let Some(activation_tick) = activation_tick {
                    backend.step_game_boy_frame_with_remote_link_after_tick(
                        game_boy_link,
                        activation_tick,
                    )
                } else {
                    backend.step_game_boy_frame_with_remote_link(game_boy_link)
                }
            }
            RemoteLink::WonderSwan(_) => backend.step_frame_with_remote_link(link),
        };
        if result.is_err() {
            link.disconnect();
            backend.set_link_peer_present(false);
            backend.step_frame();
        }
    } else {
        backend.set_link_peer_present(false);
        backend.step_frame();
    }

    let advanced = usize::from(backend.frame_count() != frame_count_before);
    if advanced > 0 && !player.is_finished() {
        player.advance_frames(1);
    }
    advanced
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PairedGameBoyReplayTimeline {
    left_start_offset: usize,
    right_start_offset: usize,
    link_activation_frame: usize,
    left_link_activation_frame: usize,
    right_link_activation_frame: usize,
    left_link_activation_tick: Option<u64>,
    right_link_activation_tick: Option<u64>,
    left_target_frames: usize,
    right_target_frames: usize,
    total_global_frames: usize,
}

#[cfg(not(target_arch = "wasm32"))]
fn paired_game_boy_replay_timeline(
    left_player: &ReplayPlayer,
    right_player: &ReplayPlayer,
    replay_tail_frames: usize,
) -> PairedGameBoyReplayTimeline {
    let common_transfer = first_common_game_boy_transfer_frames(left_player, right_player);
    let (left_start_offset, right_start_offset, transfer_anchor_frame) = match common_transfer {
        Some((left_frame, _, _, right_frame, _, _)) if left_frame >= right_frame => {
            (0, left_frame - right_frame, left_frame)
        }
        Some((left_frame, _, _, right_frame, _, _)) => (right_frame - left_frame, 0, right_frame),
        None => (0, 0, 0),
    };
    let both_streams_running_frame = left_start_offset.max(right_start_offset);
    let left_peer_present_state_frame =
        first_game_boy_peer_present_state(left_player).map(|(frame, _)| {
            left_start_offset
                .saturating_add(frame)
                .max(both_streams_running_frame)
        });
    let right_peer_present_state_frame =
        first_game_boy_peer_present_state(right_player).map(|(frame, _)| {
            right_start_offset
                .saturating_add(frame)
                .max(both_streams_running_frame)
        });
    let (
        left_link_activation_frame,
        right_link_activation_frame,
        left_link_activation_tick,
        right_link_activation_tick,
    ) = match common_transfer {
        Some((_, _, left_event, _, right_tick, right_event))
            if matches!(left_event, ReplayGameBoyLinkEvent::LocalMasterStart { .. })
                && matches!(
                    right_event,
                    ReplayGameBoyLinkEvent::RemoteMasterStart { .. }
                        | ReplayGameBoyLinkEvent::RemoteReply { .. }
                ) =>
        {
            (
                left_peer_present_state_frame.unwrap_or(transfer_anchor_frame),
                transfer_anchor_frame,
                None,
                Some(right_tick),
            )
        }
        Some((_, left_tick, left_event, _, _, right_event))
            if matches!(
                left_event,
                ReplayGameBoyLinkEvent::RemoteMasterStart { .. }
                    | ReplayGameBoyLinkEvent::RemoteReply { .. }
            ) && matches!(right_event, ReplayGameBoyLinkEvent::LocalMasterStart { .. }) =>
        {
            (
                transfer_anchor_frame,
                right_peer_present_state_frame.unwrap_or(transfer_anchor_frame),
                Some(left_tick),
                None,
            )
        }
        Some(_) | None => (
            both_streams_running_frame,
            both_streams_running_frame,
            None,
            None,
        ),
    };
    let link_activation_frame = left_link_activation_frame.min(right_link_activation_frame);
    let left_target_frames = left_player
        .total_frames()
        .saturating_add(replay_tail_frames);
    let right_target_frames = right_player
        .total_frames()
        .saturating_add(replay_tail_frames);
    let total_global_frames = left_start_offset
        .saturating_add(left_target_frames)
        .max(right_start_offset.saturating_add(right_target_frames));

    PairedGameBoyReplayTimeline {
        left_start_offset,
        right_start_offset,
        link_activation_frame,
        left_link_activation_frame,
        right_link_activation_frame,
        left_link_activation_tick,
        right_link_activation_tick,
        left_target_frames,
        right_target_frames,
        total_global_frames,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn first_common_game_boy_transfer_frames(
    left_player: &ReplayPlayer,
    right_player: &ReplayPlayer,
) -> Option<(
    usize,
    u64,
    ReplayGameBoyLinkEvent,
    usize,
    u64,
    ReplayGameBoyLinkEvent,
)> {
    let mut left_transfers = std::collections::HashMap::new();
    for (frame, tick, event) in left_player.game_boy_link_events() {
        left_transfers
            .entry(game_boy_link_transfer_id(event))
            .or_insert((frame, tick, event));
    }
    for (right_frame, right_tick, event) in right_player.game_boy_link_events() {
        let transfer_id = game_boy_link_transfer_id(event);
        if let Some((left_frame, left_tick, left_event)) = left_transfers.get(&transfer_id) {
            return Some((
                *left_frame as usize,
                *left_tick,
                *left_event,
                right_frame as usize,
                right_tick,
                event,
            ));
        }
    }
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn first_game_boy_peer_present_state(
    player: &ReplayPlayer,
) -> Option<(usize, zeff_emu_common::replay::ReplayGameBoyLinkState)> {
    player.metadata().events.iter().find_map(|event| {
        if let ReplayEvent::GameBoyLinkState { frame, state } = event
            && state.peer_present
        {
            usize::try_from(*frame).ok().map(|frame| (frame, *state))
        } else {
            None
        }
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn game_boy_link_transfer_id(event: ReplayGameBoyLinkEvent) -> u64 {
    match event {
        ReplayGameBoyLinkEvent::LocalMasterStart { transfer_id, .. }
        | ReplayGameBoyLinkEvent::RemoteMasterStart { transfer_id, .. }
        | ReplayGameBoyLinkEvent::RemoteReply { transfer_id, .. } => transfer_id,
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
fn drain_game_boy_link_events(link: &mut RemoteLink<TcpLinkTransport>) -> Vec<ReplayEvent> {
    link.take_replay_events()
        .into_iter()
        .filter(|event| matches!(event, ReplayEvent::GameBoyLink { .. }))
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn ensure_live_pair_link_connected(link: &RemoteLink<TcpLinkTransport>) -> anyhow::Result<()> {
    if link.state() == crate::link::LinkConnectionState::Disconnected {
        anyhow::bail!("live paired replay link disconnected");
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn tcp_game_boy_link_pair()
-> anyhow::Result<(RemoteLink<TcpLinkTransport>, RemoteLink<TcpLinkTransport>)> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let host_thread = std::thread::spawn(move || TcpLinkTransport::accept_once(listener));
    let client = TcpLinkTransport::connect(addr)?;
    let host = host_thread
        .join()
        .map_err(|_| anyhow::anyhow!("TCP link accept worker panicked"))??;
    Ok((
        RemoteLink::GameBoy(crate::link::gb::GameBoyRemoteLink::new(LinkSession::new(
            host,
            LinkSystemType::GameBoy,
            LinkEndpointId(1),
        ))),
        RemoteLink::GameBoy(crate::link::gb::GameBoyRemoteLink::new(LinkSession::new(
            client,
            LinkSystemType::GameBoy,
            LinkEndpointId(2),
        ))),
    ))
}

fn run_loaded_replay_headless(
    mut backend: EmuBackend,
    mut player: ReplayPlayer,
    opts: &HeadlessOptions,
) -> anyhow::Result<ReplayHeadlessSummary> {
    validate_replay_playback(&player, &backend)?;
    backend.load_state_from_bytes(player.save_state().to_vec())?;
    validate_game_boy_replay_start_tick("replay", &backend, &player)?;
    #[cfg(not(target_arch = "wasm32"))]
    if player.uses_game_boy_link_events()
        && let Some(state) = player.metadata().game_boy_link_start_state
    {
        backend.restore_game_boy_link_replay_state(state);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let mut game_boy_replay_link = {
        let link = crate::link::gb::GameBoyReplayLink::new(
            player.metadata().events.clone(),
            backend.frame_count(),
            player.metadata().game_boy_link_start_tick,
            backend.game_boy_cpu_cycles().unwrap_or(0),
        );
        (!link.is_empty()).then_some(link)
    };

    let mut events_applied = 0usize;
    let mut stalled_slices = 0usize;
    while !player.is_finished() {
        apply_replay_events_at_cursor(&mut backend, &mut player, &mut events_applied)?;
        let Some(frame) = player.peek_joypad_frames(0, 1).into_iter().next() else {
            break;
        };
        backend.set_input(frame.buttons, frame.dpad);
        backend.set_input_p2(frame.buttons_p2, frame.dpad_p2);
        backend.set_zapper_state(
            frame.zapper.enabled,
            frame.zapper.trigger,
            frame.zapper.hit,
            frame.zapper.screen_pos,
        );
        backend.set_replay_host_tilt(frame.host_tilt);
        if let Some(camera_frame) = frame.camera_frame.as_deref() {
            backend.set_replay_camera_frame(camera_frame);
        }
        let frame_count_before = backend.frame_count();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(link) = game_boy_replay_link.as_mut() {
            if let Err(err) = backend.step_game_boy_frame_with_replay_link(link) {
                let summary = link.debug_summary();
                anyhow::bail!("replay Game Boy link event failed: {err:?}; {summary}");
            }
        } else {
            backend.step_frame();
        }
        #[cfg(target_arch = "wasm32")]
        backend.step_frame();
        if backend.frame_count() != frame_count_before {
            player.advance_frames(1);
            stalled_slices = 0;
        } else {
            stalled_slices += 1;
            if stalled_slices > 10_000 {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(link) = game_boy_replay_link.as_ref() {
                    anyhow::bail!(
                        "replay made no frame progress for {stalled_slices} link slices; {}",
                        link.debug_summary()
                    );
                }
                anyhow::bail!("replay made no frame progress for {stalled_slices} slices");
            }
        }
    }
    apply_replay_events_at_cursor(&mut backend, &mut player, &mut events_applied)?;

    let final_state = backend.encode_replay_hash_state_bytes()?;
    let final_state_hash = sha256_hex(&final_state);
    let final_framebuffer_hash = sha256_hex(backend.framebuffer());

    validate_embedded_final_state_hash(
        "replay",
        player.metadata().final_state_sha256,
        &final_state_hash,
    )?;

    if let Some(expected) = opts.expect_replay_final_hash.as_deref()
        && !final_state_hash.eq_ignore_ascii_case(expected)
    {
        anyhow::bail!(
            "replay final state hash mismatch: expected {expected}, got {final_state_hash}"
        );
    }

    Ok(ReplayHeadlessSummary {
        frames: player.total_frames(),
        events_applied,
        final_state_hash,
        final_framebuffer_hash,
    })
}

fn digest_hex(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

fn validate_embedded_final_state_hash(
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

fn validate_replay_playback(player: &ReplayPlayer, backend: &EmuBackend) -> anyhow::Result<()> {
    validate_replay_metadata(player.metadata(), backend)?;
    validate_replay_input_devices(player, backend)
}

fn validate_game_boy_replay_start_tick(
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

fn apply_replay_events_at_cursor(
    backend: &mut EmuBackend,
    player: &mut ReplayPlayer,
    events_applied: &mut usize,
) -> anyhow::Result<()> {
    for event in player.take_events_at_cursor() {
        match event {
            ReplayEvent::FdsDiskSide { side, .. } => {
                backend.set_fds_disk_side(side)?;
                *events_applied += 1;
            }
            ReplayEvent::GameBoyLinkState { state, .. } => {
                backend.restore_game_boy_link_replay_state(state);
                *events_applied += 1;
            }
            ReplayEvent::GameBoyLink { .. } | ReplayEvent::WonderSwanLink { .. } => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
    use zeff_emu_common::replay::{
        ReplayEvent, ReplayGameBoyLinkEvent, ReplayGameBoyLinkState, ReplayJoypadFrame,
        ReplayMetadata, ReplayPlayer, ReplayRecorder,
    };
    use zeff_firmware::sha256_hex;
    use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

    use super::*;

    static TEST_FDS_BIOS: [u8; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE] =
        [0xFF; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE];

    struct TestTempDir {
        path: PathBuf,
    }

    impl TestTempDir {
        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestTempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn test_temp_dir(prefix: &str) -> anyhow::Result<TestTempDir> {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), suffix));
        std::fs::create_dir(&path)?;
        Ok(TestTempDir { path })
    }

    fn build_fds_test_image() -> Vec<u8> {
        let mut side_a = vec![0xA1; zeff_nes_core::hardware::cartridge::mappers::FDS_SIDE_SIZE];
        let mut side_b = vec![0xB2; zeff_nes_core::hardware::cartridge::mappers::FDS_SIDE_SIZE];
        side_a[0] = 0x01;
        side_b[0] = 0x02;
        side_a.extend_from_slice(&side_b);
        side_a
    }

    fn load_fds_test_backend(rom_path: &Path) -> anyhow::Result<EmuBackend> {
        Ok(load_backend_from_rom_source(
            ActiveSystem::Nes,
            rom_path,
            rom_path,
            Some(build_fds_test_image()),
            BackendLoadConfig {
                fds_bios_override: Some(&TEST_FDS_BIOS),
                ..BackendLoadConfig::default()
            },
        )?
        .backend)
    }

    fn build_nes_test_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[5] = 1;
        let prg = 16;
        rom[prg] = 0xA9;
        rom[prg + 1] = 0x42;
        rom[prg + 2] = 0x85;
        rom[prg + 3] = 0x00;
        rom[prg + 4] = 0xEA;
        rom[prg + 5] = 0xEA;
        rom[prg + 0x3FFC] = 0x00;
        rom[prg + 0x3FFD] = 0x80;
        rom
    }

    fn load_nes_test_backend(rom_path: &Path, rom_data: Vec<u8>) -> anyhow::Result<EmuBackend> {
        Ok(load_backend_from_rom_source(
            ActiveSystem::Nes,
            rom_path,
            rom_path,
            Some(rom_data),
            BackendLoadConfig::default(),
        )?
        .backend)
    }

    fn build_pocket_camera_test_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 0x8000];
        rom[0x134..0x143].copy_from_slice(b"POCKET CAM TEST");
        rom[0x143] = 0x00;
        rom[0x147] = 0xFC;
        rom[0x148] = 0x00;
        rom[0x149] = 0x04;
        rom[0x14A] = 0x01;
        rom[0x14B] = 0x33;
        rom[0x14C] = 0x00;
        let mut checksum = 0u8;
        for byte in &rom[0x134..=0x14C] {
            checksum = checksum.wrapping_sub(*byte).wrapping_sub(1);
        }
        rom[0x14D] = checksum;
        rom
    }

    fn load_pocket_camera_test_backend(rom_path: &Path) -> anyhow::Result<EmuBackend> {
        Ok(load_backend_from_rom_source(
            ActiveSystem::GameBoy,
            rom_path,
            rom_path,
            Some(build_pocket_camera_test_rom()),
            BackendLoadConfig::default(),
        )?
        .backend)
    }

    fn arm_pocket_camera_capture(backend: &mut EmuBackend) {
        let EmuBackend::Gb(gb) = backend else {
            panic!("expected GB backend");
        };
        gb.emu.write_byte(0x0000, 0x0A);
        gb.emu.write_byte(0x4000, 0x10);
        gb.emu.write_byte(0xA002, 0x00);
        gb.emu.write_byte(0xA003, 0x01);
        gb.emu.write_byte(0xA000, 0x01);
    }

    fn replay_player_with_gb_events(
        dir: &Path,
        name: &str,
        frames: usize,
        events: Vec<ReplayEvent>,
    ) -> anyhow::Result<ReplayPlayer> {
        let path = dir.join(name);
        let metadata = ReplayMetadata {
            events,
            ..ReplayMetadata::default()
        };
        let mut recorder = ReplayRecorder::new_with_metadata(path.clone(), Vec::new(), metadata);
        for _ in 0..frames {
            recorder.record_joypad_frame(ReplayJoypadFrame::default());
        }
        recorder.finish()?;
        ReplayPlayer::load(&path)
    }

    #[test]
    fn paired_game_boy_replay_timeline_aligns_common_transfer_ids() -> anyhow::Result<()> {
        let temp = test_temp_dir("zeff_pair_timeline")?;
        let left = replay_player_with_gb_events(
            temp.path(),
            "left.zrpl",
            8_121,
            vec![ReplayEvent::GameBoyLink {
                frame: 1_333,
                tick: 2_206_540_680,
                event: ReplayGameBoyLinkEvent::LocalMasterStart {
                    transfer_id: 0x0100_0000_0000_0000,
                    clock_period_t_cycles: 4096,
                    out_byte: 0x01,
                    serial_generation: 9,
                },
            }],
        )?;
        let right = replay_player_with_gb_events(
            temp.path(),
            "right.zrpl",
            10_148,
            vec![ReplayEvent::GameBoyLink {
                frame: 273,
                tick: 2_147_632_556,
                event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                    transfer_id: 0x0100_0000_0000_0000,
                    clock_period_t_cycles: 4096,
                    out_byte: 0x01,
                    serial_generation: 9,
                    local_reply: None,
                },
            }],
        )?;

        let timeline = paired_game_boy_replay_timeline(&left, &right, 5);

        assert_eq!(
            timeline,
            PairedGameBoyReplayTimeline {
                left_start_offset: 0,
                right_start_offset: 1_060,
                link_activation_frame: 1_333,
                left_link_activation_frame: 1_333,
                right_link_activation_frame: 1_333,
                left_link_activation_tick: None,
                right_link_activation_tick: Some(2_147_632_556),
                left_target_frames: 8_126,
                right_target_frames: 10_153,
                total_global_frames: 11_213,
            }
        );
        Ok(())
    }

    #[test]
    fn paired_game_boy_replay_timeline_uses_recorded_link_state_frames() -> anyhow::Result<()> {
        let temp = test_temp_dir("zeff_pair_timeline_link_state")?;
        let state = ReplayGameBoyLinkState {
            peer_present: true,
            pending_master_byte: None,
            pending_master_response: None,
            pending_master_completion_ready: false,
            queued_master_action: None,
            serial_generation: 0,
        };
        let left = replay_player_with_gb_events(
            temp.path(),
            "left.zrpl",
            8_121,
            vec![
                ReplayEvent::GameBoyLinkState { frame: 100, state },
                ReplayEvent::GameBoyLink {
                    frame: 1_333,
                    tick: 2_206_540_680,
                    event: ReplayGameBoyLinkEvent::LocalMasterStart {
                        transfer_id: 0x0100_0000_0000_0000,
                        clock_period_t_cycles: 4096,
                        out_byte: 0x01,
                        serial_generation: 9,
                    },
                },
            ],
        )?;
        let right = replay_player_with_gb_events(
            temp.path(),
            "right.zrpl",
            10_148,
            vec![
                ReplayEvent::GameBoyLinkState { frame: 20, state },
                ReplayEvent::GameBoyLink {
                    frame: 273,
                    tick: 2_147_632_556,
                    event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                        transfer_id: 0x0100_0000_0000_0000,
                        clock_period_t_cycles: 4096,
                        out_byte: 0x01,
                        serial_generation: 9,
                        local_reply: None,
                    },
                },
            ],
        )?;

        let timeline = paired_game_boy_replay_timeline(&left, &right, 0);

        assert_eq!(timeline.left_link_activation_frame, 1_060);
        assert_eq!(timeline.right_link_activation_frame, 1_333);
        assert_eq!(timeline.left_link_activation_tick, None);
        assert_eq!(timeline.right_link_activation_tick, Some(2_147_632_556));
        assert_eq!(timeline.link_activation_frame, 1_060);
        Ok(())
    }

    #[test]
    fn paired_game_boy_replay_timeline_defaults_without_common_transfer_ids() -> anyhow::Result<()>
    {
        let temp = test_temp_dir("zeff_pair_timeline_no_common")?;
        let left = replay_player_with_gb_events(
            temp.path(),
            "left.zrpl",
            12,
            vec![ReplayEvent::GameBoyLink {
                frame: 4,
                tick: 100,
                event: ReplayGameBoyLinkEvent::LocalMasterStart {
                    transfer_id: 1,
                    clock_period_t_cycles: 4096,
                    out_byte: 0x01,
                    serial_generation: 0,
                },
            }],
        )?;
        let right = replay_player_with_gb_events(
            temp.path(),
            "right.zrpl",
            10,
            vec![ReplayEvent::GameBoyLink {
                frame: 2,
                tick: 50,
                event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                    transfer_id: 2,
                    clock_period_t_cycles: 4096,
                    out_byte: 0x01,
                    serial_generation: 0,
                    local_reply: None,
                },
            }],
        )?;

        let timeline = paired_game_boy_replay_timeline(&left, &right, 0);

        assert_eq!(
            timeline,
            PairedGameBoyReplayTimeline {
                left_start_offset: 0,
                right_start_offset: 0,
                link_activation_frame: 0,
                left_link_activation_frame: 0,
                right_link_activation_frame: 0,
                left_link_activation_tick: None,
                right_link_activation_tick: None,
                left_target_frames: 12,
                right_target_frames: 10,
                total_global_frames: 12,
            }
        );
        Ok(())
    }

    #[test]
    fn headless_replay_route_runs_rom_file_and_checks_final_state_hash() -> anyhow::Result<()> {
        let temp = test_temp_dir("zeff_headless_replay_route")?;
        let rom_path = temp.path().join("test.nes");
        let replay_path = temp.path().join("test.zrpl");
        let rom_data = build_nes_test_rom();
        std::fs::write(&rom_path, &rom_data)?;

        let mut expected_backend = load_nes_test_backend(&rom_path, rom_data)?;
        let start_state = expected_backend.encode_state_bytes()?;
        let metadata = expected_backend.replay_metadata();

        let input_frames = [
            ReplayJoypadFrame {
                buttons: 0x01,
                dpad: 0x02,
                buttons_p2: 0x04,
                dpad_p2: 0x08,
                zapper: Default::default(),
                host_tilt: (0.0, 0.0),
                camera_frame: None,
            },
            ReplayJoypadFrame {
                buttons: 0x03,
                dpad: 0x04,
                buttons_p2: 0x02,
                dpad_p2: 0x01,
                zapper: zeff_emu_common::replay::ReplayZapperFrame {
                    enabled: true,
                    trigger: true,
                    hit: false,
                    screen_pos: Some((128, 96)),
                },
                host_tilt: (0.0, 0.0),
                camera_frame: None,
            },
            ReplayJoypadFrame {
                buttons: 0x08,
                dpad: 0x01,
                buttons_p2: 0x00,
                dpad_p2: 0x04,
                zapper: Default::default(),
                host_tilt: (0.0, 0.0),
                camera_frame: None,
            },
        ];
        let mut recorder =
            ReplayRecorder::new_with_metadata(replay_path.clone(), start_state.clone(), metadata);
        for frame in &input_frames {
            recorder.record_joypad_frame(frame.clone());
        }
        recorder.finish()?;

        expected_backend.load_state_from_bytes(start_state)?;
        for frame in &input_frames {
            expected_backend.set_input(frame.buttons, frame.dpad);
            expected_backend.set_input_p2(frame.buttons_p2, frame.dpad_p2);
            expected_backend.set_zapper_state(
                frame.zapper.enabled,
                frame.zapper.trigger,
                frame.zapper.hit,
                frame.zapper.screen_pos,
            );
            expected_backend.set_replay_host_tilt(frame.host_tilt);
            if let Some(camera_frame) = frame.camera_frame.as_deref() {
                expected_backend.set_replay_camera_frame(camera_frame);
            }
            expected_backend.step_frame();
        }
        let expected_hash = sha256_hex(&expected_backend.encode_state_bytes()?);

        super::super::run_headless(
            &rom_path,
            HardwareModePreference::Auto,
            Vec::new(),
            &HeadlessOptions {
                replay_path: Some(replay_path),
                expect_replay_final_hash: Some(expected_hash),
                ..HeadlessOptions::default()
            },
        )?;

        Ok(())
    }

    #[test]
    fn loaded_replay_applies_pocket_camera_frames() -> anyhow::Result<()> {
        let temp = test_temp_dir("zeff_camera_replay")?;
        let replay_path = temp.path().join("camera.zrpl");
        let rom_path = temp.path().join("camera.gb");

        let mut expected_backend = load_pocket_camera_test_backend(&rom_path)?;
        let runner_backend = load_pocket_camera_test_backend(&rom_path)?;
        arm_pocket_camera_capture(&mut expected_backend);
        let start_state = expected_backend.encode_state_bytes()?;
        let metadata = expected_backend.replay_metadata();
        let camera_frame = (0..(128 * 112))
            .map(|i| ((i * 17) & 0xFF) as u8)
            .collect::<Vec<_>>();
        let input_frame = ReplayJoypadFrame {
            buttons: 0,
            dpad: 0,
            buttons_p2: 0,
            dpad_p2: 0,
            zapper: Default::default(),
            host_tilt: (0.0, 0.0),
            camera_frame: Some(camera_frame),
        };

        let mut recorder =
            ReplayRecorder::new_with_metadata(replay_path.clone(), start_state.clone(), metadata);
        recorder.record_joypad_frame(input_frame.clone());
        recorder.finish()?;

        expected_backend.load_state_from_bytes(start_state)?;
        expected_backend.set_replay_camera_frame(
            input_frame
                .camera_frame
                .as_deref()
                .expect("test frame should contain camera data"),
        );
        expected_backend.step_frame();

        let player = ReplayPlayer::load(&replay_path)?;
        let summary =
            run_loaded_replay_headless(runner_backend, player, &HeadlessOptions::default())?;

        assert_eq!(summary.frames, 1);
        assert_eq!(
            summary.final_state_hash,
            sha256_hex(&expected_backend.encode_state_bytes()?)
        );

        Ok(())
    }

    #[test]
    fn loaded_replay_rejects_pocket_camera_input_for_non_camera_rom() -> anyhow::Result<()> {
        let temp = test_temp_dir("zeff_camera_replay_reject")?;
        let replay_path = temp.path().join("camera-on-nes.zrpl");
        let rom_path = temp.path().join("test.nes");
        let rom_data = build_nes_test_rom();

        let backend = load_nes_test_backend(&rom_path, rom_data)?;
        let start_state = backend.encode_state_bytes()?;
        let metadata = backend.replay_metadata();

        let mut recorder =
            ReplayRecorder::new_with_metadata(replay_path.clone(), start_state, metadata);
        recorder.record_joypad_frame(ReplayJoypadFrame {
            buttons: 0,
            dpad: 0,
            buttons_p2: 0,
            dpad_p2: 0,
            zapper: Default::default(),
            host_tilt: (0.0, 0.0),
            camera_frame: Some(vec![0x10, 0x20, 0x30, 0x40]),
        });
        recorder.finish()?;

        let player = ReplayPlayer::load(&replay_path)?;
        let err = run_loaded_replay_headless(backend, player, &HeadlessOptions::default())
            .expect_err("Pocket Camera replay payload should require Pocket Camera hardware");
        assert!(
            err.to_string().contains("Pocket Camera input"),
            "unexpected error: {err}"
        );

        Ok(())
    }

    #[test]
    fn loaded_replay_rejects_zapper_input_for_non_nes_rom() -> anyhow::Result<()> {
        let temp = test_temp_dir("zeff_zapper_replay_reject")?;
        let replay_path = temp.path().join("zapper-on-gb.zrpl");
        let rom_path = temp.path().join("plain.gb");
        let rom_data = vec![0u8; 0x8000];

        let backend = load_backend_from_rom_source(
            ActiveSystem::GameBoy,
            &rom_path,
            &rom_path,
            Some(rom_data),
            BackendLoadConfig::default(),
        )?
        .backend;
        let start_state = backend.encode_state_bytes()?;
        let metadata = backend.replay_metadata();

        let mut recorder =
            ReplayRecorder::new_with_metadata(replay_path.clone(), start_state, metadata);
        recorder.record_joypad_frame(ReplayJoypadFrame {
            buttons: 0,
            dpad: 0,
            buttons_p2: 0,
            dpad_p2: 0,
            zapper: zeff_emu_common::replay::ReplayZapperFrame {
                enabled: true,
                trigger: true,
                hit: false,
                screen_pos: Some((128, 96)),
            },
            host_tilt: (0.0, 0.0),
            camera_frame: None,
        });
        recorder.finish()?;

        let player = ReplayPlayer::load(&replay_path)?;
        let err = run_loaded_replay_headless(backend, player, &HeadlessOptions::default())
            .expect_err("Zapper replay payload should require NES hardware");
        assert!(
            err.to_string().contains("NES Zapper input"),
            "unexpected error: {err}"
        );

        Ok(())
    }

    #[test]
    fn loaded_replay_rejects_mbc7_tilt_input_for_non_mbc7_rom() -> anyhow::Result<()> {
        let temp = test_temp_dir("zeff_tilt_replay_reject")?;
        let replay_path = temp.path().join("tilt-on-plain-gb.zrpl");
        let rom_path = temp.path().join("plain.gb");
        let rom_data = vec![0u8; 0x8000];

        let backend = load_backend_from_rom_source(
            ActiveSystem::GameBoy,
            &rom_path,
            &rom_path,
            Some(rom_data),
            BackendLoadConfig::default(),
        )?
        .backend;
        let start_state = backend.encode_state_bytes()?;
        let metadata = backend.replay_metadata();

        let mut recorder =
            ReplayRecorder::new_with_metadata(replay_path.clone(), start_state, metadata);
        recorder.record_joypad_frame(ReplayJoypadFrame {
            buttons: 0,
            dpad: 0,
            buttons_p2: 0,
            dpad_p2: 0,
            zapper: Default::default(),
            host_tilt: (0.25, -0.5),
            camera_frame: None,
        });
        recorder.finish()?;

        let player = ReplayPlayer::load(&replay_path)?;
        let err = run_loaded_replay_headless(backend, player, &HeadlessOptions::default())
            .expect_err("MBC7 tilt replay payload should require MBC7 hardware");
        assert!(
            err.to_string().contains("MBC7 tilt input"),
            "unexpected error: {err}"
        );

        Ok(())
    }

    #[test]
    fn loaded_replay_rejects_embedded_final_state_hash_mismatch() -> anyhow::Result<()> {
        let temp = test_temp_dir("zeff_replay_hash_mismatch")?;
        let replay_path = temp.path().join("hash-mismatch.zrpl");
        let rom_path = temp.path().join("test.nes");
        let rom_data = build_nes_test_rom();

        let backend = load_nes_test_backend(&rom_path, rom_data)?;
        let start_state = backend.encode_state_bytes()?;
        let mut metadata = backend.replay_metadata();
        metadata.final_state_sha256 = Some([0xA5; 32]);

        let mut recorder =
            ReplayRecorder::new_with_metadata(replay_path.clone(), start_state, metadata);
        recorder.record_frame(0, 0);
        recorder.finish()?;

        let player = ReplayPlayer::load(&replay_path)?;
        let err = run_loaded_replay_headless(backend, player, &HeadlessOptions::default())
            .expect_err("embedded hash mismatch should fail playback");
        assert!(
            err.to_string()
                .contains("replay embedded final state hash mismatch"),
            "unexpected error: {err}"
        );

        Ok(())
    }

    #[test]
    fn loaded_replay_rejects_cheat_dependent_metadata() -> anyhow::Result<()> {
        let temp = test_temp_dir("zeff_replay_cheat_metadata")?;
        let replay_path = temp.path().join("cheats.zrpl");
        let rom_path = temp.path().join("test.nes");
        let rom_data = build_nes_test_rom();

        let backend = load_nes_test_backend(&rom_path, rom_data)?;
        let start_state = backend.encode_state_bytes()?;
        let mut metadata = backend.replay_metadata();
        metadata.cheat_sha256 = Some([0x5A; 32]);

        let recorder =
            ReplayRecorder::new_with_metadata(replay_path.clone(), start_state, metadata);
        recorder.finish()?;

        let player = ReplayPlayer::load(&replay_path)?;
        let err = run_loaded_replay_headless(backend, player, &HeadlessOptions::default())
            .expect_err("headless replay should reject cheat-dependent metadata");
        assert!(
            err.to_string().contains("enabled cheat set"),
            "unexpected error: {err}"
        );

        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn live_link_event_floor_rejects_incomplete_replay_metadata() -> anyhow::Result<()> {
        let temp = test_temp_dir("zeff_replay_link_event_floor")?;
        let replay_path = temp.path().join("short-link.zrpl");
        let metadata = ReplayMetadata {
            events: vec![ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 0,
                event: ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: 1,
                    out_byte: 0x42,
                    passive: true,
                    serial_generation: 7,
                },
            }],
            ..ReplayMetadata::default()
        };
        let mut recorder =
            ReplayRecorder::new_with_metadata(replay_path.clone(), b"state".to_vec(), metadata);
        recorder.record_frame(0, 0);
        recorder.finish()?;

        let player = ReplayPlayer::load(&replay_path)?;
        let err = ensure_replay_metadata_has_expected_gb_link_events(
            "left",
            &player,
            &HeadlessOptions {
                expect_gb_link_events: 2,
                ..HeadlessOptions::default()
            },
        )
        .expect_err("short GB link replay should fail the event-count preflight");
        assert!(
            err.to_string().contains("only 1 GB link events"),
            "unexpected error: {err}"
        );

        Ok(())
    }

    #[test]
    fn loaded_replay_applies_fds_side_events_before_matching_frame() -> anyhow::Result<()> {
        let temp = test_temp_dir("zeff_fds_replay_event")?;
        let replay_path = temp.path().join("side-change.zrpl");
        let rom_path = temp.path().join("test.fds");

        let mut expected_backend = load_fds_test_backend(&rom_path)?;
        let runner_backend = load_fds_test_backend(&rom_path)?;
        let start_state = expected_backend.encode_state_bytes()?;
        let metadata = expected_backend.replay_metadata();

        let mut recorder =
            ReplayRecorder::new_with_metadata(replay_path.clone(), start_state.clone(), metadata);
        recorder.record_frame(0x00, 0x00);
        recorder.record_event(ReplayEvent::FdsDiskSide { frame: 1, side: 1 });
        recorder.record_frame(0x00, 0x00);
        recorder.finish()?;

        expected_backend.load_state_from_bytes(start_state)?;
        expected_backend.set_input(0x00, 0x00);
        expected_backend.step_frame();
        expected_backend.set_fds_disk_side(1)?;
        expected_backend.set_input(0x00, 0x00);
        expected_backend.step_frame();

        let expected_hash = sha256_hex(&expected_backend.encode_state_bytes()?);
        let player = ReplayPlayer::load(&replay_path)?;
        let summary =
            run_loaded_replay_headless(runner_backend, player, &HeadlessOptions::default())?;

        assert_eq!(summary.frames, 2);
        assert_eq!(summary.events_applied, 1);
        assert_eq!(summary.final_state_hash, expected_hash);
        assert_eq!(expected_backend.fds_disk_side(), Some(1));

        Ok(())
    }
}
