use zeff_emu_common::replay::{
    ReplayEvent, ReplayGameBoyLinkAction, ReplayGameBoyLinkReply, ReplayGameBoyLinkState,
    ReplayPlayer,
};
use zeff_gb_core::hardware::bus::{
    GameBoyLinkAction, GameBoyLinkExchangePreview, GameBoyLinkPreparedTransfer, GameBoyLinkReply,
};

use crate::emu_backend::EmuBackend;

use super::paired_plan::{
    InitialMasterContinuation, LocatedEvent, PairedTransferPlan, Side, Transfer,
};

mod error;
mod side;
#[cfg(test)]
mod tests;

pub(super) use error::DirectCoordinatorError;
use side::DirectSide;

pub(super) struct DirectPairedReplayResult {
    pub(super) frames: usize,
    pub(super) left_replay_frames: usize,
    pub(super) right_replay_frames: usize,
    pub(super) left_recorded_link_events: usize,
    pub(super) right_recorded_link_events: usize,
    pub(super) left_generated_link_events: Vec<ReplayEvent>,
    pub(super) right_generated_link_events: Vec<ReplayEvent>,
    pub(super) left_final_state_hash: String,
    pub(super) right_final_state_hash: String,
    pub(super) left_final_framebuffer_hash: String,
    pub(super) right_final_framebuffer_hash: String,
    pub(super) left_player: ReplayPlayer,
    pub(super) right_player: ReplayPlayer,
}

pub(super) fn run_direct_paired_replay(
    left: EmuBackend,
    right: EmuBackend,
    left_player: ReplayPlayer,
    right_player: ReplayPlayer,
    plan: PairedTransferPlan,
    tail_frames: usize,
) -> Result<DirectPairedReplayResult, DirectCoordinatorError> {
    validate_supported_plan(&left_player, &right_player, &plan)?;
    validate_planned_sequence(&left_player, &plan, Side::Left)?;
    validate_planned_sequence(&right_player, &plan, Side::Right)?;
    let mut left = DirectSide::new(Side::Left, left, left_player, tail_frames)?;
    let mut right = DirectSide::new(Side::Right, right, right_player, tail_frames)?;

    if let Some(initial) = plan.initial_master {
        execute_initial_master_continuation(&mut left, &mut right, initial)?;
    }

    for batch in &plan.batches {
        match batch.transfers.as_slice() {
            [transfer] => execute_singleton_transfer(&mut left, &mut right, transfer)?,
            [first, second] => execute_crossed_transfers(&mut left, &mut right, first, second)?,
            transfers => {
                return Err(DirectCoordinatorError::UnsupportedCrossedBatch {
                    transfers: transfers.len(),
                });
            }
        }
    }

    left.finish()?;
    right.finish()?;
    left.verify_generated_complete()?;
    right.verify_generated_complete()?;

    let frames = left.advanced_frames.max(right.advanced_frames);
    Ok(DirectPairedReplayResult {
        frames,
        left_replay_frames: left.recorded_frames,
        right_replay_frames: right.recorded_frames,
        left_recorded_link_events: left.expected_semantics.len(),
        right_recorded_link_events: right.expected_semantics.len(),
        left_generated_link_events: left.generated,
        right_generated_link_events: right.generated,
        left_final_state_hash: left
            .final_state_hash
            .expect("finished side captured its final state hash"),
        right_final_state_hash: right
            .final_state_hash
            .expect("finished side captured its final state hash"),
        left_final_framebuffer_hash: left
            .final_framebuffer_hash
            .expect("finished side captured its final framebuffer hash"),
        right_final_framebuffer_hash: right
            .final_framebuffer_hash
            .expect("finished side captured its final framebuffer hash"),
        left_player: left.player,
        right_player: right.player,
    })
}

fn validate_supported_plan(
    left: &ReplayPlayer,
    right: &ReplayPlayer,
    plan: &PairedTransferPlan,
) -> Result<(), DirectCoordinatorError> {
    if left
        .metadata()
        .game_boy_link_start_state
        .is_some_and(|state| state.pending_passive_completion.is_some())
        && right
            .metadata()
            .game_boy_link_start_state
            .is_some_and(|state| state.pending_passive_completion.is_some())
    {
        return Err(DirectCoordinatorError::ConflictingPassiveStartStates);
    }
    for (side, player) in [(Side::Left, left), (Side::Right, right)] {
        if let Some(state) = player.metadata().game_boy_link_start_state {
            let coordinator = player.metadata().game_boy_link_coordinator_start_state;
            let valid = state.validate().is_ok()
                && if state.has_master_owned_transfer() {
                    coordinator
                        .map(|coordinator| coordinator.validate_against(state).is_ok())
                        .unwrap_or_else(|| state.queued_master_action.is_some())
                } else {
                    coordinator.is_none()
                };
            if !valid {
                return Err(DirectCoordinatorError::UnsafeStartState { side });
            }
        }
        for (ordinal, event) in player.metadata().events.iter().enumerate() {
            if !matches!(
                event,
                ReplayEvent::GameBoyLink { .. }
                    | ReplayEvent::GameBoyLinkState { .. }
                    | ReplayEvent::GameBoyLinkStateAtTick { .. }
            ) {
                return Err(DirectCoordinatorError::UnsupportedStateOrEvent { side, ordinal });
            }
            if let ReplayEvent::GameBoyLinkState { state, .. }
            | ReplayEvent::GameBoyLinkStateAtTick { state, .. } = event
                && (state.validate().is_err() || state.has_master_owned_transfer())
            {
                return Err(DirectCoordinatorError::UnsafeStatePayload { side, ordinal });
            }
        }
    }
    for batch in &plan.batches {
        match batch.transfers.as_slice() {
            [transfer] => validate_singleton_shape(transfer)?,
            [first, second] => validate_crossed_shape(first, second)?,
            transfers => {
                return Err(DirectCoordinatorError::UnsupportedCrossedBatch {
                    transfers: transfers.len(),
                });
            }
        }
    }
    Ok(())
}

fn validate_planned_sequence(
    player: &ReplayPlayer,
    plan: &PairedTransferPlan,
    side: Side,
) -> Result<(), DirectCoordinatorError> {
    let expected: Vec<_> = player
        .metadata()
        .events
        .iter()
        .enumerate()
        .filter(|(_, event)| matches!(event, ReplayEvent::GameBoyLink { .. }))
        .map(|(ordinal, event)| (ordinal, event.clone()))
        .collect();
    let mut planned = Vec::new();
    if let Some(initial) = plan.initial_master
        && initial.master == side
        && let Some(reply) = initial.reply_event
    {
        planned.push((reply.ordinal, located_event(reply)));
    }
    for batch in &plan.batches {
        for transfer in &batch.transfers {
            let start = transfer.start(side);
            planned.push((start.ordinal, located_event(start)));
            if transfer.master == side {
                planned.push((
                    transfer.master_reply.ordinal,
                    located_event(transfer.master_reply),
                ));
            }
        }
    }
    planned.sort_by_key(|(ordinal, _)| *ordinal);
    if planned != expected {
        return Err(DirectCoordinatorError::GeneratedOrder { side });
    }
    Ok(())
}

fn execute_initial_master_continuation(
    left: &mut DirectSide,
    right: &mut DirectSide,
    initial: InitialMasterContinuation,
) -> Result<(), DirectCoordinatorError> {
    let expected_reply = initial
        .reply_event
        .map(LocatedEvent::reply)
        .or(initial.state.reply)
        .ok_or(DirectCoordinatorError::UnsafeStartState {
            side: initial.master,
        })?;

    if let Some(reply_event) = initial.reply_event {
        let master: &mut DirectSide = match initial.master {
            Side::Left => &mut *left,
            Side::Right => &mut *right,
        };
        if reply_event.ordinal != 0 {
            return Err(DirectCoordinatorError::StateDuringPreparedTransfer {
                side: initial.master,
                id: initial.state.transfer_id,
                ordinal: 0,
            });
        }
        if !master.backend.apply_game_boy_link_reply(expected_reply) {
            return Err(DirectCoordinatorError::Exchange(
                "initial GB master continuation rejected its recorded reply".to_string(),
            ));
        }
        master.record_validated(reply_event)?;
    }
    left.settle_frame()?;
    right.settle_frame()?;
    Ok(())
}

fn validate_singleton_shape(transfer: &Transfer) -> Result<(), DirectCoordinatorError> {
    let local_start = transfer.start(transfer.master);
    validate_observation_point(
        transfer.master,
        transfer.id,
        local_start,
        transfer.master_reply,
    )?;
    let expected_reply_ordinal =
        local_start
            .ordinal
            .checked_add(1)
            .ok_or(DirectCoordinatorError::GeneratedOrder {
                side: transfer.master,
            })?;
    if transfer.master_reply.ordinal != expected_reply_ordinal {
        return Err(DirectCoordinatorError::StateDuringPreparedTransfer {
            side: transfer.master,
            id: transfer.id,
            ordinal: expected_reply_ordinal,
        });
    }
    Ok(())
}

fn validate_crossed_shape(
    first: &Transfer,
    second: &Transfer,
) -> Result<(), DirectCoordinatorError> {
    let (left_master, right_master) = crossed_masters(first, second)?;
    for (side, local, remote, reply) in [
        (
            Side::Left,
            left_master.left_start,
            right_master.left_start,
            left_master.master_reply,
        ),
        (
            Side::Right,
            right_master.right_start,
            left_master.right_start,
            right_master.master_reply,
        ),
    ] {
        validate_observation_point(
            side,
            local_id(left_master, right_master, side),
            local,
            remote,
        )?;
        validate_observation_point(
            side,
            local_id(left_master, right_master, side),
            local,
            reply,
        )?;
        let remote_ordinal = local
            .ordinal
            .checked_add(1)
            .ok_or(DirectCoordinatorError::GeneratedOrder { side })?;
        let reply_ordinal = local
            .ordinal
            .checked_add(2)
            .ok_or(DirectCoordinatorError::GeneratedOrder { side })?;
        if remote.ordinal != remote_ordinal || reply.ordinal != reply_ordinal {
            return Err(DirectCoordinatorError::StateDuringPreparedTransfer {
                side,
                id: local_id(left_master, right_master, side),
                ordinal: if remote.ordinal != remote_ordinal {
                    remote_ordinal
                } else {
                    reply_ordinal
                },
            });
        }
    }
    Ok(())
}

fn crossed_masters<'a>(
    first: &'a Transfer,
    second: &'a Transfer,
) -> Result<(&'a Transfer, &'a Transfer), DirectCoordinatorError> {
    match (first.master, second.master) {
        (Side::Left, Side::Right) => Ok((first, second)),
        (Side::Right, Side::Left) => Ok((second, first)),
        _ => Err(DirectCoordinatorError::InvalidCrossedBatch),
    }
}

fn local_id(left_master: &Transfer, right_master: &Transfer, side: Side) -> u64 {
    match side {
        Side::Left => left_master.id,
        Side::Right => right_master.id,
    }
}

fn validate_observation_point(
    side: Side,
    id: u64,
    start: LocatedEvent,
    observation: LocatedEvent,
) -> Result<(), DirectCoordinatorError> {
    let next_frame = start.point.frame.checked_add(1);
    let safe = observation.point.absolute_tick == start.point.absolute_tick
        && (observation.point.frame == start.point.frame
            || next_frame == Some(observation.point.frame));
    if safe {
        Ok(())
    } else {
        Err(DirectCoordinatorError::DelayedReply {
            side,
            id,
            start: start.point,
            reply: observation.point,
        })
    }
}

fn execute_singleton_transfer(
    left: &mut DirectSide,
    right: &mut DirectSide,
    transfer: &Transfer,
) -> Result<(), DirectCoordinatorError> {
    let expected_action = transfer.start(transfer.master).action();
    match transfer.master {
        Side::Left => {
            left.reach(&transfer.left_start, Some(expected_action))?;
            right.reach(&transfer.right_start, None)?;
        }
        Side::Right => {
            left.reach(&transfer.left_start, None)?;
            right.reach(&transfer.right_start, Some(expected_action))?;
        }
    }
    match transfer.master {
        Side::Left => left.validate_reply_observation_shape(&transfer.master_reply)?,
        Side::Right => right.validate_reply_observation_shape(&transfer.master_reply)?,
    }

    let preview = left
        .backend
        .preview_game_boy_link_peer(&right.backend)
        .ok_or(DirectCoordinatorError::IncompatibleBackends)?;
    validate_preview(preview, transfer)?;
    let expected_reply = transfer.master_reply.reply();
    let mut prepared = left
        .backend
        .try_prepare_game_boy_link_peer(&mut right.backend)
        .map_err(|error| DirectCoordinatorError::Exchange(error.to_string()))?;

    match transfer.master {
        Side::Left => {
            left.mark_owned_transfer(Some(expected_action.clock_period_t_cycles))?;
            right.mark_owned_transfer(
                expected_reply
                    .passive
                    .then_some(expected_action.clock_period_t_cycles),
            )?;
        }
        Side::Right => {
            left.mark_owned_transfer(
                expected_reply
                    .passive
                    .then_some(expected_action.clock_period_t_cycles),
            )?;
            right.mark_owned_transfer(Some(expected_action.clock_period_t_cycles))?;
        }
    }

    left.record_validated(transfer.left_start)?;
    right.record_validated(transfer.right_start)?;
    match transfer.master {
        Side::Left => {
            let token = prepared
                .local_action
                .take()
                .ok_or(DirectCoordinatorError::MissingPreparedToken { side: Side::Left })?;
            if prepared.peer_action.is_some() {
                return Err(DirectCoordinatorError::UnexpectedPreparedToken { side: Side::Right });
            }
            validate_token(&token, expected_action, expected_reply, Side::Left)?;
            left.reach_reply_observation(&transfer.master_reply)?;
            left.apply_reply(token)?;
            left.record_validated(transfer.master_reply)?;
        }
        Side::Right => {
            let token = prepared
                .peer_action
                .take()
                .ok_or(DirectCoordinatorError::MissingPreparedToken { side: Side::Right })?;
            if prepared.local_action.is_some() {
                return Err(DirectCoordinatorError::UnexpectedPreparedToken { side: Side::Left });
            }
            validate_token(&token, expected_action, expected_reply, Side::Right)?;
            right.reach_reply_observation(&transfer.master_reply)?;
            right.apply_reply(token)?;
            right.record_validated(transfer.master_reply)?;
        }
    }
    left.settle_frame()?;
    right.settle_frame()?;
    Ok(())
}

fn execute_crossed_transfers(
    left: &mut DirectSide,
    right: &mut DirectSide,
    first: &Transfer,
    second: &Transfer,
) -> Result<(), DirectCoordinatorError> {
    let (left_master, right_master) = crossed_masters(first, second)?;
    let left_action = left_master.left_start.action();
    let right_action = right_master.right_start.action();

    left.reach(&left_master.left_start, Some(left_action))?;
    right.reach(&right_master.right_start, Some(right_action))?;
    left.validate_reply_observation_shape(&right_master.left_start)?;
    left.validate_reply_observation_shape(&left_master.master_reply)?;
    right.validate_reply_observation_shape(&left_master.right_start)?;
    right.validate_reply_observation_shape(&right_master.master_reply)?;

    let preview = left
        .backend
        .preview_game_boy_link_peer(&right.backend)
        .ok_or(DirectCoordinatorError::IncompatibleBackends)?;
    let left_reply = left_master.master_reply.reply();
    let right_reply = right_master.master_reply.reply();
    validate_crossed_preview(preview, left_action, right_action, left_reply, right_reply)?;
    let mut prepared = left
        .backend
        .try_prepare_game_boy_link_peer(&mut right.backend)
        .map_err(|error| DirectCoordinatorError::Exchange(error.to_string()))?;

    left.mark_owned_transfer(Some(left_action.clock_period_t_cycles))?;
    right.mark_owned_transfer(Some(right_action.clock_period_t_cycles))?;
    left.record_validated(left_master.left_start)?;
    right.record_validated(right_master.right_start)?;
    left.record_validated(right_master.left_start)?;
    right.record_validated(left_master.right_start)?;

    let left_token = prepared
        .local_action
        .take()
        .ok_or(DirectCoordinatorError::MissingPreparedToken { side: Side::Left })?;
    let right_token = prepared
        .peer_action
        .take()
        .ok_or(DirectCoordinatorError::MissingPreparedToken { side: Side::Right })?;
    validate_token(&left_token, left_action, left_reply, Side::Left)?;
    validate_token(&right_token, right_action, right_reply, Side::Right)?;
    left.apply_reply(left_token)?;
    right.apply_reply(right_token)?;
    left.record_validated(left_master.master_reply)?;
    right.record_validated(right_master.master_reply)?;
    left.settle_frame()?;
    right.settle_frame()?;
    Ok(())
}

fn validate_crossed_preview(
    preview: GameBoyLinkExchangePreview,
    left_action: ReplayGameBoyLinkAction,
    right_action: ReplayGameBoyLinkAction,
    left_reply: ReplayGameBoyLinkReply,
    right_reply: ReplayGameBoyLinkReply,
) -> Result<(), DirectCoordinatorError> {
    for (side, expected, actual) in [
        (Side::Left, left_action, preview.local_action),
        (Side::Right, right_action, preview.peer_action),
    ] {
        if actual != Some(core_action(expected)) {
            return Err(DirectCoordinatorError::ActionMismatch {
                side,
                expected,
                actual,
            });
        }
    }
    for (side, expected, actual) in [
        (Side::Right, left_reply, preview.peer_reply),
        (Side::Left, right_reply, preview.local_reply),
    ] {
        if actual != core_reply(expected) {
            return Err(DirectCoordinatorError::ReplyMismatch {
                side,
                expected,
                actual,
            });
        }
    }
    Ok(())
}

fn validate_preview(
    preview: GameBoyLinkExchangePreview,
    transfer: &Transfer,
) -> Result<(), DirectCoordinatorError> {
    let expected_action = core_action(transfer.start(transfer.master).action());
    let expected_reply = core_reply(transfer.master_reply.reply());
    let (actual_action, peer_action, actual_reply) = match transfer.master {
        Side::Left => (
            preview.local_action,
            preview.peer_action,
            preview.peer_reply,
        ),
        Side::Right => (
            preview.peer_action,
            preview.local_action,
            preview.local_reply,
        ),
    };
    if actual_action != Some(expected_action) {
        return Err(DirectCoordinatorError::ActionMismatch {
            side: transfer.master,
            expected: transfer.start(transfer.master).action(),
            actual: actual_action,
        });
    }
    if peer_action.is_some() {
        return Err(DirectCoordinatorError::UnexpectedAction {
            side: transfer.master.peer(),
        });
    }
    if actual_reply != expected_reply {
        return Err(DirectCoordinatorError::ReplyMismatch {
            side: transfer.master.peer(),
            expected: transfer.master_reply.reply(),
            actual: actual_reply,
        });
    }
    Ok(())
}

fn validate_token(
    token: &GameBoyLinkPreparedTransfer,
    action: ReplayGameBoyLinkAction,
    reply: ReplayGameBoyLinkReply,
    side: Side,
) -> Result<(), DirectCoordinatorError> {
    if token.action() != core_action(action) {
        return Err(DirectCoordinatorError::ActionMismatch {
            side,
            expected: action,
            actual: Some(token.action()),
        });
    }
    if token.reply() != core_reply(reply) {
        return Err(DirectCoordinatorError::ReplyMismatch {
            side: side.peer(),
            expected: reply,
            actual: token.reply(),
        });
    }
    Ok(())
}

fn passive_completion_deadline(
    backend: &EmuBackend,
    state: ReplayGameBoyLinkState,
    side: Side,
) -> Result<Option<u64>, DirectCoordinatorError> {
    state
        .pending_passive_completion
        .map(|completion| {
            backend
                .game_boy_cpu_cycles()
                .unwrap_or(u64::MAX)
                .checked_add(completion.remaining_t_cycles)
                .ok_or(DirectCoordinatorError::TransferTickOverflow { side })
        })
        .transpose()
}

fn core_action(action: ReplayGameBoyLinkAction) -> GameBoyLinkAction {
    GameBoyLinkAction {
        out_byte: action.out_byte,
        clock_period_t_cycles: action.clock_period_t_cycles,
        serial_generation: action.serial_generation,
    }
}

fn core_reply(reply: ReplayGameBoyLinkReply) -> GameBoyLinkReply {
    GameBoyLinkReply {
        out_byte: reply.out_byte,
        passive: reply.passive,
        serial_generation: reply.serial_generation,
    }
}

fn located_event(event: LocatedEvent) -> ReplayEvent {
    ReplayEvent::GameBoyLink {
        frame: event.point.frame,
        tick: event.point.tick,
        event: event.event,
    }
}
