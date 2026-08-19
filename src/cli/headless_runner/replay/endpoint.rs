use crate::emu_backend::EmuBackend;
use zeff_emu_common::replay::{ReplayEvent, ReplayPlayer};
use zeff_firmware::sha256_hex;

use super::HeadlessOptions;
use super::validation::{
    validate_embedded_final_state_hash, validate_game_boy_replay_start_tick,
    validate_replay_checkpoint, validate_replay_playback, validate_wonder_swan_replay_start_tick,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ReplayHeadlessSummary {
    pub(super) frames: usize,
    pub(super) events_applied: usize,
    pub(super) game_boy_link_events_delivered: usize,
    pub(super) game_boy_link_events_total: usize,
    pub(super) final_state_hash: String,
    pub(super) final_framebuffer_hash: String,
}

pub(super) fn run_loaded_replay_headless(
    mut backend: EmuBackend,
    mut player: ReplayPlayer,
    opts: &HeadlessOptions,
) -> anyhow::Result<ReplayHeadlessSummary> {
    validate_replay_playback(&player, &backend)?;
    let has_game_boy_link_events = player.uses_game_boy_link_events();
    backend.load_state_from_bytes(player.save_state().to_vec())?;
    validate_game_boy_replay_start_tick("replay", &backend, &player)?;
    validate_wonder_swan_replay_start_tick("replay", &backend, &player)?;
    #[cfg(not(target_arch = "wasm32"))]
    if player.uses_game_boy_link_events()
        && let Some(state) = player.metadata().game_boy_link_start_state
    {
        backend.restore_game_boy_link_replay_state(state);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let mut game_boy_replay_link = {
        let mut link = crate::link::gb::GameBoyReplayLink::try_new(
            player.metadata().events.clone(),
            backend.frame_count(),
            player.metadata().game_boy_link_start_tick,
            backend.game_boy_cpu_cycles().unwrap_or(0),
        )?;
        link.set_strict_local_reply_validation(!opts.allow_gb_link_replay_divergence);
        (!link.is_empty()).then_some(link)
    };
    #[cfg(not(target_arch = "wasm32"))]
    let mut wonder_swan_replay_link = {
        let link = crate::link::ws_replay::WonderSwanReplayLink::try_new(
            player.metadata().events.clone(),
            backend.frame_count(),
            player.metadata().wonder_swan_link_start_tick,
            backend.wonder_swan_cpu_cycles().unwrap_or(0),
        )?;
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
        } else if let Some(link) = wonder_swan_replay_link.as_mut() {
            if let Err(err) = backend.step_wonder_swan_frame_with_replay_link(link) {
                anyhow::bail!(
                    "replay WonderSwan link event failed: {err:?}; {}",
                    link.debug_summary()
                );
            }
        } else {
            backend.step_frame();
        }
        #[cfg(target_arch = "wasm32")]
        backend.step_frame();
        if backend.frame_count() != frame_count_before {
            player.advance_frames(1);
            handle_game_boy_link_validation(
                validate_replay_checkpoint(&player, &backend),
                has_game_boy_link_events,
                opts.allow_gb_link_replay_divergence,
            )?;
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
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(link) = wonder_swan_replay_link.as_ref() {
                    anyhow::bail!(
                        "replay made no frame progress for {stalled_slices} WonderSwan link slices; {}",
                        link.debug_summary()
                    );
                }
                anyhow::bail!("replay made no frame progress for {stalled_slices} slices");
            }
        }
    }
    apply_replay_events_at_cursor(&mut backend, &mut player, &mut events_applied)?;

    #[cfg(not(target_arch = "wasm32"))]
    let game_boy_link_event_progress = if let Some(link) = game_boy_replay_link.as_ref() {
        if !link.all_events_delivered() {
            anyhow::bail!(
                "replay ended with undelivered Game Boy link events; {}",
                link.debug_summary()
            );
        }
        link.event_progress()
    } else {
        (0, 0)
    };
    #[cfg(target_arch = "wasm32")]
    let game_boy_link_event_progress = (0, 0);

    let final_state = backend.encode_replay_hash_state_bytes()?;
    let final_state_hash = sha256_hex(&final_state);
    let final_framebuffer_hash = sha256_hex(backend.framebuffer());

    handle_game_boy_link_validation(
        validate_embedded_final_state_hash(
            "replay",
            player.metadata().final_state_sha256,
            &final_state_hash,
        ),
        has_game_boy_link_events,
        opts.allow_gb_link_replay_divergence,
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
        game_boy_link_events_delivered: game_boy_link_event_progress.0,
        game_boy_link_events_total: game_boy_link_event_progress.1,
        final_state_hash,
        final_framebuffer_hash,
    })
}

fn handle_game_boy_link_validation(
    result: anyhow::Result<()>,
    has_game_boy_link_events: bool,
    allow_divergence: bool,
) -> anyhow::Result<()> {
    if has_game_boy_link_events && allow_divergence {
        if let Err(err) = result {
            log::warn!("{err}");
        }
        return Ok(());
    }
    result
}

#[cfg(test)]
pub(super) fn validate_game_boy_link_replay_result_for_test(
    result: anyhow::Result<()>,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    handle_game_boy_link_validation(result, true, opts.allow_gb_link_replay_divergence)
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
            ReplayEvent::GameBoyLinkStateAtTick { .. } => {}
            ReplayEvent::GameBoyLink { .. } | ReplayEvent::WonderSwanLink { .. } => {}
        }
    }
    Ok(())
}
