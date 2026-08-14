use zeff_emu_common::replay::{ReplayEvent, ReplayGameBoyLinkEvent, ReplayPlayer};

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PairedGameBoyReplayTimeline {
    pub(super) left_start_offset: usize,
    pub(super) right_start_offset: usize,
    pub(super) link_activation_frame: usize,
    pub(super) left_link_activation_frame: usize,
    pub(super) right_link_activation_frame: usize,
    pub(super) left_link_activation_tick: Option<u64>,
    pub(super) right_link_activation_tick: Option<u64>,
    pub(super) left_target_frames: usize,
    pub(super) right_target_frames: usize,
    pub(super) total_global_frames: usize,
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn paired_game_boy_replay_timeline(
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
pub(super) fn first_game_boy_peer_present_state(
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
