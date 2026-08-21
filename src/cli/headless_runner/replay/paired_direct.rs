use std::fmt;

use zeff_emu_common::replay::{
    ReplayEvent, ReplayGameBoyLinkAction, ReplayGameBoyLinkEvent, ReplayGameBoyLinkReply,
    ReplayGameBoyLinkState, ReplayPlayer,
};
use zeff_firmware::sha256_hex;
use zeff_gb_core::hardware::bus::{
    GameBoyLinkAction, GameBoyLinkExchangePreview, GameBoyLinkPreparedTransfer, GameBoyLinkReply,
};

use crate::emu_backend::EmuBackend;

use super::paired_lease::{
    PairedGameBoyFrameLease, PairedGameBoyFrameLeaseOutcome, PairedGameBoyPointRelation,
};
use super::paired_plan::{LocatedEvent, PairedTransferPlan, Point, Side, Transfer};
use super::validation::validate_replay_checkpoint;

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
    for (side, player) in [(Side::Left, left), (Side::Right, right)] {
        if player
            .metadata()
            .game_boy_link_start_state
            .is_some_and(replay_state_has_owned_transfer)
        {
            return Err(DirectCoordinatorError::UnsafeStartState { side });
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
                && replay_state_has_owned_transfer(*state)
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
    let expected_action = replay_action(transfer.start(transfer.master).event);
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
    let expected_reply = replay_reply(transfer.master_reply.event);
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
    let left_action = replay_action(left_master.left_start.event);
    let right_action = replay_action(right_master.right_start.event);

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
    let left_reply = replay_reply(left_master.master_reply.event);
    let right_reply = replay_reply(right_master.master_reply.event);
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
    let expected_action = core_action(replay_action(transfer.start(transfer.master).event));
    let expected_reply = core_reply(replay_reply(transfer.master_reply.event));
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
            expected: replay_action(transfer.start(transfer.master).event),
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
            expected: replay_reply(transfer.master_reply.event),
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

struct DirectSide {
    side: Side,
    backend: EmuBackend,
    player: ReplayPlayer,
    replay_start_tick: u64,
    lease: PairedGameBoyFrameLease,
    expected_semantics: Vec<(usize, ReplayEvent)>,
    semantic_cursor: usize,
    source_cursor: usize,
    generated: Vec<ReplayEvent>,
    recorded_frames: usize,
    target_frames: usize,
    advanced_frames: usize,
    pending_frame_complete: bool,
    owned_transfer_until_tick: Option<u64>,
    final_state_hash: Option<String>,
    final_framebuffer_hash: Option<String>,
}

impl DirectSide {
    fn new(
        side: Side,
        mut backend: EmuBackend,
        player: ReplayPlayer,
        tail_frames: usize,
    ) -> Result<Self, DirectCoordinatorError> {
        let recorded_frames = player.total_frames();
        let target_frames = recorded_frames
            .checked_add(tail_frames)
            .ok_or(DirectCoordinatorError::FrameOverflow { side })?;
        let expected_semantics = player
            .metadata()
            .events
            .iter()
            .enumerate()
            .filter(|(_, event)| matches!(event, ReplayEvent::GameBoyLink { .. }))
            .map(|(ordinal, event)| (ordinal, event.clone()))
            .collect();
        let replay_start_tick = player
            .metadata()
            .game_boy_link_start_tick
            .ok_or(DirectCoordinatorError::MissingStartTick { side })?;
        if let Some(state) = player.metadata().game_boy_link_start_state {
            backend.restore_game_boy_link_replay_state(state);
        } else {
            backend.set_link_peer_present(false);
        }
        let mut owner = Self {
            side,
            backend,
            player,
            replay_start_tick,
            lease: PairedGameBoyFrameLease::default(),
            expected_semantics,
            semantic_cursor: 0,
            source_cursor: 0,
            generated: Vec::new(),
            recorded_frames,
            target_frames,
            advanced_frames: 0,
            pending_frame_complete: false,
            owned_transfer_until_tick: None,
            final_state_hash: None,
            final_framebuffer_hash: None,
        };
        owner.capture_final_if_due()?;
        Ok(owner)
    }

    fn reach(
        &mut self,
        event: &LocatedEvent,
        expected_action: Option<ReplayGameBoyLinkAction>,
    ) -> Result<(), DirectCoordinatorError> {
        self.apply_state_events_before(event.ordinal)?;
        self.reach_exact_point(
            event.point.frame,
            event.point.absolute_tick,
            expected_action,
        )
    }

    fn reach_exact_point(
        &mut self,
        target_frame: u64,
        target_tick: u64,
        expected_action: Option<ReplayGameBoyLinkAction>,
    ) -> Result<(), DirectCoordinatorError> {
        let target_frame = usize::try_from(target_frame)
            .map_err(|_| DirectCoordinatorError::FrameOverflow { side: self.side })?;
        while self.advanced_frames < target_frame {
            if self.pending_frame_complete {
                self.settle_frame()?;
            } else {
                self.finish_frame_without_event()?;
            }
        }
        if self.advanced_frames != target_frame {
            return Err(DirectCoordinatorError::FrameOrder {
                side: self.side,
                expected: target_frame,
                actual: self.advanced_frames,
            });
        }
        if self.pending_frame_complete {
            let actual_tick = self.backend.game_boy_cpu_cycles().unwrap_or(u64::MAX);
            if actual_tick != target_tick {
                return Err(DirectCoordinatorError::FrameCommitBlocked { side: self.side });
            }
            return self.validate_point_action(expected_action);
        }
        self.begin_frame()?;
        let progress = self
            .lease
            .step_direct_until(&mut self.backend, Some(target_tick), true)
            .map_err(|error| DirectCoordinatorError::Backend(error.to_string()))?;
        match progress.point {
            PairedGameBoyPointRelation::Exact => {}
            PairedGameBoyPointRelation::Overshot => {
                return Err(DirectCoordinatorError::Overshot {
                    side: self.side,
                    target: target_tick,
                    actual: self.backend.game_boy_cpu_cycles().unwrap_or(u64::MAX),
                });
            }
            PairedGameBoyPointRelation::Before | PairedGameBoyPointRelation::NotRequested => {
                return Err(DirectCoordinatorError::EarlyBoundary { side: self.side });
            }
        }
        self.validate_point_action(expected_action)?;
        match progress.outcome {
            PairedGameBoyFrameLeaseOutcome::FrameComplete => {
                self.pending_frame_complete = true;
            }
            PairedGameBoyFrameLeaseOutcome::Boundary => {}
            PairedGameBoyFrameLeaseOutcome::Suspended => {
                return Err(DirectCoordinatorError::Suspended { side: self.side });
            }
        }
        Ok(())
    }

    fn reach_reply_observation(
        &mut self,
        event: &LocatedEvent,
    ) -> Result<(), DirectCoordinatorError> {
        self.apply_state_events_before(event.ordinal)?;
        self.validate_reply_observation_shape(event)
    }

    fn validate_reply_observation_shape(
        &self,
        event: &LocatedEvent,
    ) -> Result<(), DirectCoordinatorError> {
        let target_frame = usize::try_from(event.point.frame)
            .map_err(|_| DirectCoordinatorError::FrameOverflow { side: self.side })?;
        let current_tick = self.backend.game_boy_cpu_cycles();
        if target_frame == self.advanced_frames && current_tick == Some(event.point.absolute_tick) {
            return Ok(());
        }
        if self
            .advanced_frames
            .checked_add(1)
            .is_some_and(|frame| frame == target_frame)
            && self.pending_frame_complete
            && current_tick == Some(event.point.absolute_tick)
        {
            return Ok(());
        }
        Err(DirectCoordinatorError::ReplyObservationRequiresStep {
            side: self.side,
            frame: event.point.frame,
        })
    }

    fn validate_point_action(
        &self,
        expected_action: Option<ReplayGameBoyLinkAction>,
    ) -> Result<(), DirectCoordinatorError> {
        let actual = self
            .backend
            .game_boy_link_replay_state()
            .and_then(|state| state.queued_master_action);
        match (expected_action, actual) {
            (Some(expected), Some(actual)) if expected == actual => Ok(()),
            (Some(_), None) => Ok(()),
            (Some(expected), Some(actual)) => Err(DirectCoordinatorError::ActionMismatch {
                side: self.side,
                expected,
                actual: Some(core_action(actual)),
            }),
            (None, Some(_)) => Err(DirectCoordinatorError::UnexpectedAction { side: self.side }),
            (None, None) => Ok(()),
        }
    }

    fn apply_state_events_before(
        &mut self,
        target_ordinal: usize,
    ) -> Result<(), DirectCoordinatorError> {
        while self.source_cursor < target_ordinal {
            let ordinal = self.source_cursor;
            let event = self
                .player
                .metadata()
                .events
                .get(ordinal)
                .cloned()
                .ok_or(DirectCoordinatorError::GeneratedOrder { side: self.side })?;
            match event {
                ReplayEvent::GameBoyLinkState { frame, state } => {
                    self.apply_frame_state(ordinal, frame, state)?;
                }
                ReplayEvent::GameBoyLinkStateAtTick { frame, tick, state } => {
                    self.apply_timed_state(ordinal, frame, tick, state)?;
                }
                _ => return Err(DirectCoordinatorError::GeneratedOrder { side: self.side }),
            }
            self.source_cursor += 1;
        }
        if self.source_cursor != target_ordinal {
            return Err(DirectCoordinatorError::GeneratedOrder { side: self.side });
        }
        Ok(())
    }

    fn apply_frame_state(
        &mut self,
        ordinal: usize,
        frame: u64,
        state: ReplayGameBoyLinkState,
    ) -> Result<(), DirectCoordinatorError> {
        let target_frame = usize::try_from(frame)
            .map_err(|_| DirectCoordinatorError::FrameOverflow { side: self.side })?;
        while self.advanced_frames < target_frame {
            if self.pending_frame_complete {
                self.settle_frame()?;
            } else {
                self.finish_frame_without_event()?;
            }
        }
        if self.advanced_frames != target_frame
            || self.pending_frame_complete
            || !self.lease.needs_frame_setup()
        {
            return Err(DirectCoordinatorError::StateNotAtFrameBoundary {
                side: self.side,
                ordinal,
            });
        }
        self.ensure_state_restore_safe(ordinal)?;
        self.backend.restore_game_boy_link_replay_state(state);
        Ok(())
    }

    fn apply_timed_state(
        &mut self,
        ordinal: usize,
        frame: u64,
        tick: u64,
        state: ReplayGameBoyLinkState,
    ) -> Result<(), DirectCoordinatorError> {
        let absolute_tick = self.replay_start_tick.checked_add(tick).ok_or(
            DirectCoordinatorError::StateTickOverflow {
                side: self.side,
                ordinal,
            },
        )?;
        self.reach_exact_point(frame, absolute_tick, None)?;
        self.ensure_state_restore_safe(ordinal)?;
        self.backend.restore_game_boy_link_replay_state(state);
        Ok(())
    }

    fn ensure_state_restore_safe(&mut self, ordinal: usize) -> Result<(), DirectCoordinatorError> {
        let tick = self.backend.game_boy_cpu_cycles().unwrap_or(u64::MAX);
        if self
            .owned_transfer_until_tick
            .is_some_and(|until| tick >= until)
        {
            self.owned_transfer_until_tick = None;
        }
        let runtime_busy = self
            .backend
            .game_boy_link_state()
            .is_none_or(|state| !state.is_idle());
        let replay_busy = self
            .backend
            .game_boy_link_replay_state()
            .is_none_or(|state| {
                state.pending_master_byte.is_some()
                    || state.pending_master_response.is_some()
                    || state.pending_master_completion_ready
                    || state.queued_master_action.is_some()
            });
        if self.owned_transfer_until_tick.is_some() || runtime_busy || replay_busy {
            return Err(DirectCoordinatorError::StateOverwritesTransfer {
                side: self.side,
                ordinal,
                tick,
            });
        }
        Ok(())
    }

    fn mark_owned_transfer(&mut self, period: Option<u64>) -> Result<(), DirectCoordinatorError> {
        let Some(period) = period else {
            return Ok(());
        };
        let tick = self.backend.game_boy_cpu_cycles().unwrap_or(u64::MAX);
        self.owned_transfer_until_tick = Some(
            tick.checked_add(period)
                .ok_or(DirectCoordinatorError::TransferTickOverflow { side: self.side })?,
        );
        Ok(())
    }

    fn begin_frame(&mut self) -> Result<(), DirectCoordinatorError> {
        if !self.lease.needs_frame_setup() {
            return Ok(());
        }
        let frame = self
            .player
            .peek_joypad_frames(0, 1)
            .into_iter()
            .next()
            .unwrap_or_default();
        self.backend.set_input(frame.buttons, frame.dpad);
        self.backend.set_input_p2(frame.buttons_p2, frame.dpad_p2);
        let zapper = crate::emu_thread::ZapperInput::from(frame.zapper);
        self.backend.set_zapper_state(
            zapper.enabled,
            zapper.trigger,
            zapper.hit,
            zapper.screen_pos,
        );
        self.backend.set_replay_host_tilt(frame.host_tilt);
        if let Some(camera_frame) = frame.camera_frame.as_deref() {
            self.backend.set_replay_camera_frame(camera_frame);
        }
        self.lease
            .begin(&self.backend, None)
            .map_err(|error| DirectCoordinatorError::Backend(format!("{error:?}")))
    }

    fn finish_frame_without_event(&mut self) -> Result<(), DirectCoordinatorError> {
        self.begin_frame()?;
        let progress = self
            .lease
            .step_direct_until(&mut self.backend, None, true)
            .map_err(|error| DirectCoordinatorError::Backend(error.to_string()))?;
        if progress.queued_master_action.is_some() {
            return Err(DirectCoordinatorError::UnexpectedAction { side: self.side });
        }
        match progress.outcome {
            PairedGameBoyFrameLeaseOutcome::FrameComplete => {
                self.pending_frame_complete = true;
                self.settle_frame()
            }
            PairedGameBoyFrameLeaseOutcome::Boundary => {
                Err(DirectCoordinatorError::NoProgress { side: self.side })
            }
            PairedGameBoyFrameLeaseOutcome::Suspended => {
                Err(DirectCoordinatorError::Suspended { side: self.side })
            }
        }
    }

    fn settle_frame(&mut self) -> Result<(), DirectCoordinatorError> {
        if !self.pending_frame_complete {
            return Ok(());
        }
        if !self.player.is_finished() {
            self.player.advance_frames(1);
        }
        self.validate_checkpoint_window()?;
        self.lease.commit_frame();
        self.pending_frame_complete = false;
        self.advanced_frames = self
            .advanced_frames
            .checked_add(1)
            .ok_or(DirectCoordinatorError::FrameOverflow { side: self.side })?;
        self.capture_final_if_due()
    }

    fn record_validated(&mut self, event: LocatedEvent) -> Result<(), DirectCoordinatorError> {
        let Some((ordinal, expected)) = self.expected_semantics.get(self.semantic_cursor) else {
            return Err(DirectCoordinatorError::GeneratedOrder { side: self.side });
        };
        let generated = located_event(event);
        if (*ordinal, expected) != (event.ordinal, &generated)
            || self.source_cursor != event.ordinal
        {
            return Err(DirectCoordinatorError::GeneratedOrder { side: self.side });
        }
        self.generated.push(generated);
        self.semantic_cursor += 1;
        self.source_cursor += 1;
        Ok(())
    }

    fn apply_reply(
        &mut self,
        token: GameBoyLinkPreparedTransfer,
    ) -> Result<(), DirectCoordinatorError> {
        self.backend
            .try_apply_prepared_game_boy_link_reply(token)
            .map(|_| ())
            .map_err(|error| DirectCoordinatorError::Exchange(error.to_string()))
    }

    fn finish(&mut self) -> Result<(), DirectCoordinatorError> {
        self.apply_state_events_before(self.player.metadata().events.len())?;
        self.settle_frame()?;
        while self.advanced_frames < self.target_frames {
            self.finish_frame_without_event()?;
        }
        self.capture_final_if_due()
    }

    fn capture_final_if_due(&mut self) -> Result<(), DirectCoordinatorError> {
        if self.final_state_hash.is_none() && self.advanced_frames == self.recorded_frames {
            self.final_state_hash = Some(sha256_hex(
                &self
                    .backend
                    .encode_replay_hash_state_bytes()
                    .map_err(|error| DirectCoordinatorError::Backend(error.to_string()))?,
            ));
            self.final_framebuffer_hash = Some(sha256_hex(self.backend.framebuffer()));
        }
        Ok(())
    }

    fn verify_generated_complete(&self) -> Result<(), DirectCoordinatorError> {
        if self.semantic_cursor != self.expected_semantics.len()
            || self.source_cursor != self.player.metadata().events.len()
        {
            return Err(DirectCoordinatorError::GeneratedOrder { side: self.side });
        }
        Ok(())
    }

    fn validate_checkpoint_window(&mut self) -> Result<(), DirectCoordinatorError> {
        validate_replay_checkpoint(&self.player, &self.backend).map_err(|error| {
            DirectCoordinatorError::Checkpoint {
                side: self.side,
                message: format!(
                    "{error}; semantic={}/{} previous={:?} next={:?} replay_frame={} core_frame={} tick={} link_state={:?}",
                    self.semantic_cursor,
                    self.expected_semantics.len(),
                    self.semantic_cursor
                        .checked_sub(1)
                        .and_then(|index| self.expected_semantics.get(index)),
                    self.expected_semantics.get(self.semantic_cursor),
                    self.player.cursor(),
                    self.backend.frame_count(),
                    self.backend.game_boy_cpu_cycles().unwrap_or(u64::MAX),
                    self.backend.game_boy_link_replay_state()
                ),
            }
        })
    }
}

impl Transfer {
    fn start(&self, side: Side) -> LocatedEvent {
        match side {
            Side::Left => self.left_start,
            Side::Right => self.right_start,
        }
    }
}

impl Side {
    fn peer(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

fn replay_action(event: ReplayGameBoyLinkEvent) -> ReplayGameBoyLinkAction {
    match event {
        ReplayGameBoyLinkEvent::LocalMasterStart {
            out_byte,
            clock_period_t_cycles,
            serial_generation,
            ..
        }
        | ReplayGameBoyLinkEvent::RemoteMasterStart {
            out_byte,
            clock_period_t_cycles,
            serial_generation,
            ..
        } => ReplayGameBoyLinkAction {
            out_byte,
            clock_period_t_cycles,
            serial_generation,
        },
        ReplayGameBoyLinkEvent::RemoteReply { .. } => {
            unreachable!("transfer start is not a reply")
        }
    }
}

fn replay_reply(event: ReplayGameBoyLinkEvent) -> ReplayGameBoyLinkReply {
    match event {
        ReplayGameBoyLinkEvent::RemoteReply {
            out_byte,
            passive,
            serial_generation,
            ..
        } => ReplayGameBoyLinkReply {
            out_byte,
            passive,
            serial_generation,
        },
        _ => unreachable!("transfer reply is a reply event"),
    }
}

fn replay_state_has_owned_transfer(state: ReplayGameBoyLinkState) -> bool {
    state.pending_master_byte.is_some()
        || state.pending_master_response.is_some()
        || state.pending_master_completion_ready
        || state.queued_master_action.is_some()
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

#[derive(Debug)]
pub(super) enum DirectCoordinatorError {
    UnsupportedStateOrEvent {
        side: Side,
        ordinal: usize,
    },
    UnsupportedCrossedBatch {
        transfers: usize,
    },
    InvalidCrossedBatch,
    MissingStartTick {
        side: Side,
    },
    StateDuringPreparedTransfer {
        side: Side,
        id: u64,
        ordinal: usize,
    },
    UnsafeStatePayload {
        side: Side,
        ordinal: usize,
    },
    UnsafeStartState {
        side: Side,
    },
    StateNotAtFrameBoundary {
        side: Side,
        ordinal: usize,
    },
    StateTickOverflow {
        side: Side,
        ordinal: usize,
    },
    StateOverwritesTransfer {
        side: Side,
        ordinal: usize,
        tick: u64,
    },
    TransferTickOverflow {
        side: Side,
    },
    ReplyObservationRequiresStep {
        side: Side,
        frame: u64,
    },
    DelayedReply {
        side: Side,
        id: u64,
        start: Point,
        reply: Point,
    },
    FrameOverflow {
        side: Side,
    },
    FrameOrder {
        side: Side,
        expected: usize,
        actual: usize,
    },
    FrameCommitBlocked {
        side: Side,
    },
    Overshot {
        side: Side,
        target: u64,
        actual: u64,
    },
    EarlyBoundary {
        side: Side,
    },
    NoProgress {
        side: Side,
    },
    Suspended {
        side: Side,
    },
    UnexpectedAction {
        side: Side,
    },
    ActionMismatch {
        side: Side,
        expected: ReplayGameBoyLinkAction,
        actual: Option<GameBoyLinkAction>,
    },
    ReplyMismatch {
        side: Side,
        expected: ReplayGameBoyLinkReply,
        actual: GameBoyLinkReply,
    },
    MissingPreparedToken {
        side: Side,
    },
    UnexpectedPreparedToken {
        side: Side,
    },
    IncompatibleBackends,
    GeneratedOrder {
        side: Side,
    },
    Checkpoint {
        side: Side,
        message: String,
    },
    Exchange(String),
    Backend(String),
}

impl fmt::Display for DirectCoordinatorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedStateOrEvent { side, ordinal } => write!(
                f,
                "direct paired replay does not yet support {side} non-semantic event at ordinal {ordinal}"
            ),
            Self::UnsupportedCrossedBatch { transfers } => write!(
                f,
                "direct paired replay does not yet support a crossed batch of {transfers} transfers"
            ),
            Self::InvalidCrossedBatch => {
                f.write_str("direct paired replay crossed batch does not have one master per side")
            }
            Self::MissingStartTick { side } => {
                write!(
                    f,
                    "direct paired replay is missing the {side} link start tick"
                )
            }
            Self::StateDuringPreparedTransfer { side, id, ordinal } => write!(
                f,
                "{side} replay state event at ordinal {ordinal} would split prepared transfer {id:#018X}"
            ),
            Self::UnsafeStatePayload { side, ordinal } => write!(
                f,
                "{side} replay state event at ordinal {ordinal} contains an owned transfer"
            ),
            Self::UnsafeStartState { side } => {
                write!(
                    f,
                    "{side} replay link start state contains an owned transfer"
                )
            }
            Self::StateNotAtFrameBoundary { side, ordinal } => write!(
                f,
                "{side} replay frame state at ordinal {ordinal} is not at an idle frame boundary"
            ),
            Self::StateTickOverflow { side, ordinal } => write!(
                f,
                "{side} replay state event at ordinal {ordinal} overflows its absolute tick"
            ),
            Self::StateOverwritesTransfer {
                side,
                ordinal,
                tick,
            } => write!(
                f,
                "{side} replay state event at ordinal {ordinal} would overwrite an in-flight transfer at tick {tick}"
            ),
            Self::TransferTickOverflow { side } => {
                write!(f, "{side} replay transfer completion tick overflowed")
            }
            Self::ReplyObservationRequiresStep { side, frame } => write!(
                f,
                "{side} replay reply observation at frame {frame} would require advancing a prepared master"
            ),
            Self::DelayedReply {
                side,
                id,
                start,
                reply,
            } => write!(
                f,
                "direct paired replay does not support {side} transfer {id:#018X} reply observation: start={start:?} reply={reply:?}"
            ),
            Self::FrameOverflow { side } => write!(f, "{side} replay frame count overflows usize"),
            Self::FrameOrder {
                side,
                expected,
                actual,
            } => write!(
                f,
                "{side} replay frame order diverged: expected {expected}, got {actual}"
            ),
            Self::FrameCommitBlocked { side } => {
                write!(
                    f,
                    "{side} replay point crosses an uncommitted completed frame"
                )
            }
            Self::Overshot {
                side,
                target,
                actual,
            } => write!(
                f,
                "{side} replay overshot exact tick {target} at instruction boundary {actual}"
            ),
            Self::EarlyBoundary { side } => {
                write!(f, "{side} replay reached a link boundary before its target")
            }
            Self::NoProgress { side } => write!(f, "{side} replay made no typed frame progress"),
            Self::Suspended { side } => write!(f, "{side} replay side suspended"),
            Self::UnexpectedAction { side } => {
                write!(f, "{side} replay produced an unexpected local link action")
            }
            Self::ActionMismatch {
                side,
                expected,
                actual,
            } => write!(
                f,
                "{side} replay link action mismatch: expected {expected:?}, got {actual:?}"
            ),
            Self::ReplyMismatch {
                side,
                expected,
                actual,
            } => write!(
                f,
                "{side} replay link reply mismatch: expected {expected:?}, got {actual:?}"
            ),
            Self::MissingPreparedToken { side } => {
                write!(f, "{side} replay exchange did not prepare its master token")
            }
            Self::UnexpectedPreparedToken { side } => {
                write!(
                    f,
                    "{side} replay exchange prepared an unexpected master token"
                )
            }
            Self::IncompatibleBackends => {
                f.write_str("direct paired replay requires two Game Boy backends")
            }
            Self::GeneratedOrder { side } => {
                write!(
                    f,
                    "{side} replay generated semantic events out of source order"
                )
            }
            Self::Checkpoint { side, message } => {
                write!(f, "{side} replay checkpoint mismatch: {message}")
            }
            Self::Exchange(message) => write!(f, "direct GB exchange failed: {message}"),
            Self::Backend(message) => write!(f, "direct GB replay backend failed: {message}"),
        }
    }
}

impl std::error::Error for DirectCoordinatorError {}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use zeff_emu_common::replay::{ReplayCheckpoint, ReplayMetadata, ReplayRecorder};
    use zeff_firmware::sha256_bytes;
    use zeff_gb_core::hardware::types::constants::{SERIAL_SB, SERIAL_SC};
    use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

    use super::*;
    use crate::cli::headless_runner::replay::paired_plan::PairedTransferPlan;

    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(1);
    const TRANSFER_ID: u64 = 0x0100_0000_0000_0001;
    const RIGHT_TRANSFER_ID: u64 = 0x0200_0000_0000_0001;

    fn backend(path: &str) -> EmuBackend {
        backend_from_rom(path, vec![0u8; 0x8000])
    }

    fn backend_from_rom(path: &str, rom: Vec<u8>) -> EmuBackend {
        let emulator =
            zeff_gb_core::emulator::Emulator::from_rom_data(&rom, HardwareModePreference::Auto)
                .unwrap();
        EmuBackend::from_gb(emulator, PathBuf::from(path))
    }

    fn player(
        events: Vec<ReplayEvent>,
        tick: u64,
        label: &str,
        checkpoint: Option<[u8; 32]>,
    ) -> ReplayPlayer {
        let id = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "zeff-paired-direct-{label}-{}-{id}.zrpl",
            std::process::id()
        ));
        let metadata = ReplayMetadata {
            events,
            game_boy_link_start_tick: Some(tick),
            checkpoints: checkpoint
                .map(|state_sha256| {
                    vec![ReplayCheckpoint {
                        frame: 1,
                        state_sha256,
                    }]
                })
                .unwrap_or_default(),
            ..ReplayMetadata::default()
        };
        let mut recorder = ReplayRecorder::new_with_metadata(path.clone(), Vec::new(), metadata);
        recorder.record_frame(0, 0);
        recorder.finish().unwrap();
        let player = ReplayPlayer::load(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        player
    }

    fn configured_pair() -> (EmuBackend, EmuBackend) {
        let mut left = backend("left.gb");
        let mut right = backend("right.gb");
        let EmuBackend::Gb(left_gb) = &mut left else {
            unreachable!();
        };
        left_gb.emu.write_byte(SERIAL_SB, 0xAB);
        left_gb.emu.write_byte(SERIAL_SC, 0x81);
        let EmuBackend::Gb(right_gb) = &mut right else {
            unreachable!();
        };
        right_gb.emu.write_byte(SERIAL_SB, 0x34);
        right_gb.emu.write_byte(SERIAL_SC, 0x80);
        (left, right)
    }

    fn configured_crossed_pair() -> (EmuBackend, EmuBackend) {
        let mut left = backend("crossed-left.gb");
        let mut right = backend("crossed-right.gb");
        let EmuBackend::Gb(left_gb) = &mut left else {
            unreachable!();
        };
        left_gb.emu.write_byte(SERIAL_SB, 0xAB);
        left_gb.emu.write_byte(SERIAL_SC, 0x81);
        let EmuBackend::Gb(right_gb) = &mut right else {
            unreachable!();
        };
        right_gb.emu.write_byte(SERIAL_SB, 0x34);
        right_gb.emu.write_byte(SERIAL_SC, 0x81);
        (left, right)
    }

    fn crossed_fixture() -> (EmuBackend, EmuBackend, ReplayPlayer, ReplayPlayer) {
        let (left, right) = configured_crossed_pair();

        let start_tick = left.game_boy_cpu_cycles().unwrap();
        assert_eq!(right.game_boy_cpu_cycles(), Some(start_tick));
        let preview = left.preview_game_boy_link_peer(&right).unwrap();
        let left_action = preview.local_action.unwrap();
        let right_action = preview.peer_action.unwrap();
        assert!(!preview.local_reply.passive);
        assert!(!preview.peer_reply.passive);

        let left_events = vec![
            ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 0,
                event: ReplayGameBoyLinkEvent::LocalMasterStart {
                    transfer_id: TRANSFER_ID,
                    clock_period_t_cycles: left_action.clock_period_t_cycles,
                    out_byte: left_action.out_byte,
                    serial_generation: left_action.serial_generation,
                },
            },
            ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 0,
                event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                    transfer_id: RIGHT_TRANSFER_ID,
                    clock_period_t_cycles: right_action.clock_period_t_cycles,
                    out_byte: right_action.out_byte,
                    serial_generation: right_action.serial_generation,
                    local_reply: Some(ReplayGameBoyLinkReply {
                        out_byte: preview.local_reply.out_byte,
                        passive: preview.local_reply.passive,
                        serial_generation: preview.local_reply.serial_generation,
                    }),
                },
            },
            ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 0,
                event: ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: TRANSFER_ID,
                    out_byte: preview.peer_reply.out_byte,
                    passive: preview.peer_reply.passive,
                    serial_generation: preview.peer_reply.serial_generation,
                },
            },
        ];
        let right_events = vec![
            ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 0,
                event: ReplayGameBoyLinkEvent::LocalMasterStart {
                    transfer_id: RIGHT_TRANSFER_ID,
                    clock_period_t_cycles: right_action.clock_period_t_cycles,
                    out_byte: right_action.out_byte,
                    serial_generation: right_action.serial_generation,
                },
            },
            ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 0,
                event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                    transfer_id: TRANSFER_ID,
                    clock_period_t_cycles: left_action.clock_period_t_cycles,
                    out_byte: left_action.out_byte,
                    serial_generation: left_action.serial_generation,
                    local_reply: Some(ReplayGameBoyLinkReply {
                        out_byte: preview.peer_reply.out_byte,
                        passive: preview.peer_reply.passive,
                        serial_generation: preview.peer_reply.serial_generation,
                    }),
                },
            },
            ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 0,
                event: ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: RIGHT_TRANSFER_ID,
                    out_byte: preview.local_reply.out_byte,
                    passive: preview.local_reply.passive,
                    serial_generation: preview.local_reply.serial_generation,
                },
            },
        ];
        (
            left,
            right,
            player(left_events, start_tick, "crossed-left", None),
            player(right_events, start_tick, "crossed-right", None),
        )
    }

    fn fixture(
        event_tick: u64,
        reply_tick: u64,
        checkpoints: (Option<[u8; 32]>, Option<[u8; 32]>),
    ) -> (EmuBackend, EmuBackend, ReplayPlayer, ReplayPlayer) {
        let (left, right) = configured_pair();
        let start_tick = left.game_boy_cpu_cycles().unwrap();
        assert_eq!(right.game_boy_cpu_cycles(), Some(start_tick));
        let preview = left.preview_game_boy_link_peer(&right).unwrap();
        let action = preview.local_action.unwrap();
        let reply = preview.peer_reply;
        let left_events = vec![
            ReplayEvent::GameBoyLink {
                frame: 0,
                tick: event_tick,
                event: ReplayGameBoyLinkEvent::LocalMasterStart {
                    transfer_id: TRANSFER_ID,
                    clock_period_t_cycles: action.clock_period_t_cycles,
                    out_byte: action.out_byte,
                    serial_generation: action.serial_generation,
                },
            },
            ReplayEvent::GameBoyLink {
                frame: 0,
                tick: reply_tick,
                event: ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: TRANSFER_ID,
                    out_byte: reply.out_byte,
                    passive: reply.passive,
                    serial_generation: reply.serial_generation,
                },
            },
        ];
        let right_events = vec![ReplayEvent::GameBoyLink {
            frame: 0,
            tick: event_tick,
            event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                transfer_id: TRANSFER_ID,
                clock_period_t_cycles: action.clock_period_t_cycles,
                out_byte: action.out_byte,
                serial_generation: action.serial_generation,
                local_reply: Some(ReplayGameBoyLinkReply {
                    out_byte: reply.out_byte,
                    passive: reply.passive,
                    serial_generation: reply.serial_generation,
                }),
            },
        }];
        (
            left,
            right,
            player(left_events, start_tick, "left", checkpoints.0),
            player(right_events, start_tick, "right", checkpoints.1),
        )
    }

    fn run_fixture() -> DirectPairedReplayResult {
        let (left, right, left_player, right_player) = fixture(0, 0, (None, None));
        let plan = PairedTransferPlan::build(
            &left_player.metadata().events,
            left_player.metadata().game_boy_link_start_tick,
            &right_player.metadata().events,
            right_player.metadata().game_boy_link_start_tick,
        )
        .unwrap();
        run_direct_paired_replay(left, right, left_player, right_player, plan, 0).unwrap()
    }

    #[test]
    fn singleton_execution_is_exact_and_deterministic() {
        let first = run_fixture();
        let second = run_fixture();
        assert_eq!(first.left_generated_link_events.len(), 2);
        assert_eq!(first.right_generated_link_events.len(), 1);
        assert_eq!(
            first.left_generated_link_events,
            second.left_generated_link_events
        );
        assert_eq!(
            first.right_generated_link_events,
            second.right_generated_link_events
        );
        assert_eq!(first.left_final_state_hash, second.left_final_state_hash);
        assert_eq!(first.right_final_state_hash, second.right_final_state_hash);
    }

    #[test]
    fn crossed_master_result_matches_core_exchange_oracle() {
        let (left, right, left_player, right_player) = crossed_fixture();
        let plan = PairedTransferPlan::build(
            &left_player.metadata().events,
            left_player.metadata().game_boy_link_start_tick,
            &right_player.metadata().events,
            right_player.metadata().game_boy_link_start_tick,
        )
        .unwrap();
        let result =
            run_direct_paired_replay(left, right, left_player, right_player, plan, 0).unwrap();

        let (mut oracle_left, mut oracle_right) = configured_crossed_pair();
        let (EmuBackend::Gb(left_gb), EmuBackend::Gb(right_gb)) =
            (&mut oracle_left, &mut oracle_right)
        else {
            unreachable!();
        };
        left_gb
            .emu
            .try_sync_game_boy_link_peer(&mut right_gb.emu)
            .unwrap();
        oracle_left.step_frame();
        oracle_right.step_frame();

        assert_eq!(
            result.left_final_state_hash,
            sha256_hex(&oracle_left.encode_replay_hash_state_bytes().unwrap())
        );
        assert_eq!(
            result.right_final_state_hash,
            sha256_hex(&oracle_right.encode_replay_hash_state_bytes().unwrap())
        );
        assert_eq!(
            result.left_final_framebuffer_hash,
            sha256_hex(oracle_left.framebuffer())
        );
        assert_eq!(
            result.right_final_framebuffer_hash,
            sha256_hex(oracle_right.framebuffer())
        );
    }

    #[test]
    fn crossed_masters_execute_atomically_and_deterministically() {
        fn run() -> DirectPairedReplayResult {
            let (left, right, left_player, right_player) = crossed_fixture();
            let plan = PairedTransferPlan::build(
                &left_player.metadata().events,
                left_player.metadata().game_boy_link_start_tick,
                &right_player.metadata().events,
                right_player.metadata().game_boy_link_start_tick,
            )
            .unwrap();
            assert_eq!(plan.batches.len(), 1);
            assert_eq!(plan.batches[0].transfers.len(), 2);
            run_direct_paired_replay(left, right, left_player, right_player, plan, 0).unwrap()
        }

        let first = run();
        let second = run();
        assert_eq!(first.left_generated_link_events.len(), 3);
        assert_eq!(first.right_generated_link_events.len(), 3);
        assert_eq!(
            first.left_generated_link_events,
            second.left_generated_link_events
        );
        assert_eq!(
            first.right_generated_link_events,
            second.right_generated_link_events
        );
        assert_eq!(first.left_final_state_hash, second.left_final_state_hash);
        assert_eq!(first.right_final_state_hash, second.right_final_state_hash);
    }

    #[test]
    fn crossed_master_batch_rejects_delayed_remote_observation() {
        let (left, right, left_player, right_player) = crossed_fixture();
        let mut left_events = left_player.metadata().events.clone();
        for event in &mut left_events[1..=2] {
            let ReplayEvent::GameBoyLink { frame, tick, .. } = event else {
                unreachable!();
            };
            *frame = 1;
            *tick = 4;
        }
        let start_tick = left_player.metadata().game_boy_link_start_tick.unwrap();
        let left_player = player(left_events, start_tick, "crossed-delayed-left", None);
        let plan = PairedTransferPlan::build(
            &left_player.metadata().events,
            left_player.metadata().game_boy_link_start_tick,
            &right_player.metadata().events,
            right_player.metadata().game_boy_link_start_tick,
        )
        .unwrap();

        assert!(matches!(
            run_direct_paired_replay(left, right, left_player, right_player, plan, 0),
            Err(DirectCoordinatorError::DelayedReply {
                side: Side::Left,
                id: TRANSFER_ID,
                ..
            })
        ));
    }

    #[test]
    fn delayed_reply_is_rejected_before_execution() {
        let (left, right, left_player, right_player) = fixture(0, 4, (None, None));
        let plan = PairedTransferPlan::build(
            &left_player.metadata().events,
            left_player.metadata().game_boy_link_start_tick,
            &right_player.metadata().events,
            right_player.metadata().game_boy_link_start_tick,
        )
        .unwrap();
        assert!(matches!(
            run_direct_paired_replay(left, right, left_player, right_player, plan, 0),
            Err(DirectCoordinatorError::DelayedReply { .. })
        ));
    }

    #[test]
    fn frame_advanced_reply_requires_a_frame_complete_start_without_mutation() {
        let (left, right, left_player, right_player) = fixture(0, 0, (None, None));
        let mut left_events = left_player.metadata().events.clone();
        let ReplayEvent::GameBoyLink { frame, .. } = &mut left_events[1] else {
            unreachable!();
        };
        *frame = 1;
        let start_tick = left_player.metadata().game_boy_link_start_tick.unwrap();
        let left_player = player(left_events, start_tick, "boundary-required-left", None);
        let plan = PairedTransferPlan::build(
            &left_player.metadata().events,
            left_player.metadata().game_boy_link_start_tick,
            &right_player.metadata().events,
            right_player.metadata().game_boy_link_start_tick,
        )
        .unwrap();
        let transfer = plan.batches[0].transfers[0].clone();
        let expected_action = replay_action(transfer.start(transfer.master).event);
        let mut left = DirectSide::new(Side::Left, left, left_player, 0).unwrap();
        let mut right = DirectSide::new(Side::Right, right, right_player, 0).unwrap();
        left.reach(&transfer.left_start, Some(expected_action))
            .unwrap();
        right.reach(&transfer.right_start, None).unwrap();
        let left_before = left.backend.encode_replay_hash_state_bytes().unwrap();
        let right_before = right.backend.encode_replay_hash_state_bytes().unwrap();

        assert!(matches!(
            left.validate_reply_observation_shape(&transfer.master_reply),
            Err(DirectCoordinatorError::ReplyObservationRequiresStep {
                side: Side::Left,
                frame: 1
            })
        ));
        assert_eq!(
            left.backend.encode_replay_hash_state_bytes().unwrap(),
            left_before
        );
        assert_eq!(
            right.backend.encode_replay_hash_state_bytes().unwrap(),
            right_before
        );
    }

    #[test]
    fn timed_idle_state_is_applied_at_its_exact_point() {
        let backend = backend("timed-state.gb");
        let start_tick = backend.game_boy_cpu_cycles().unwrap();
        let state = ReplayGameBoyLinkState {
            peer_present: true,
            pending_master_byte: None,
            pending_master_response: None,
            pending_master_completion_ready: false,
            queued_master_action: None,
            serial_generation: 7,
        };
        let player = player(
            vec![ReplayEvent::GameBoyLinkStateAtTick {
                frame: 0,
                tick: 4,
                state,
            }],
            start_tick,
            "timed-state",
            None,
        );
        let mut side = DirectSide::new(Side::Left, backend, player, 0).unwrap();

        side.apply_state_events_before(1).unwrap();

        assert_eq!(side.source_cursor, 1);
        assert_eq!(side.backend.game_boy_cpu_cycles(), Some(start_tick + 4));
        assert_eq!(side.backend.game_boy_link_replay_state(), Some(state));
        assert!(!side.lease.needs_frame_setup());
    }

    #[test]
    fn frame_complete_state_restore_precedes_checkpoint_and_commit() {
        let state = ReplayGameBoyLinkState {
            peer_present: true,
            pending_master_byte: None,
            pending_master_response: None,
            pending_master_completion_ready: false,
            queued_master_action: None,
            serial_generation: 9,
        };
        let mut expected = backend("state-checkpoint-expected.gb");
        let start_tick = expected.game_boy_cpu_cycles().unwrap();
        let mut expected_lease = PairedGameBoyFrameLease::default();
        expected_lease.begin(&expected, None).unwrap();
        assert_eq!(
            expected_lease
                .step_direct_until(&mut expected, None, false)
                .unwrap()
                .outcome,
            PairedGameBoyFrameLeaseOutcome::FrameComplete
        );
        let event_tick = expected.game_boy_cpu_cycles().unwrap() - start_tick;
        expected.restore_game_boy_link_replay_state(state);
        let checkpoint = sha256_bytes(&expected.encode_replay_hash_state_bytes().unwrap());
        let player = player(
            vec![ReplayEvent::GameBoyLinkStateAtTick {
                frame: 0,
                tick: event_tick,
                state,
            }],
            start_tick,
            "state-checkpoint",
            Some(checkpoint),
        );
        let mut side =
            DirectSide::new(Side::Left, backend("state-checkpoint.gb"), player, 0).unwrap();

        side.apply_state_events_before(1).unwrap();

        assert!(side.pending_frame_complete);
        assert_eq!(side.backend.game_boy_link_replay_state(), Some(state));
        side.settle_frame().unwrap();
        assert_eq!(side.advanced_frames, 1);
        assert!(side.lease.needs_frame_setup());
    }

    #[test]
    fn state_payload_with_owned_transfer_is_rejected_in_preflight() {
        let (left, right, left_player, right_player) = fixture(0, 0, (None, None));
        let mut left_events = left_player.metadata().events.clone();
        left_events.insert(
            0,
            ReplayEvent::GameBoyLinkStateAtTick {
                frame: 0,
                tick: 0,
                state: ReplayGameBoyLinkState {
                    peer_present: true,
                    pending_master_byte: Some(0xAB),
                    pending_master_response: None,
                    pending_master_completion_ready: false,
                    queued_master_action: None,
                    serial_generation: 4,
                },
            },
        );
        let start_tick = left_player.metadata().game_boy_link_start_tick.unwrap();
        let left_player = player(left_events, start_tick, "unsafe-state", None);
        let plan = PairedTransferPlan::build(
            &left_player.metadata().events,
            left_player.metadata().game_boy_link_start_tick,
            &right_player.metadata().events,
            right_player.metadata().game_boy_link_start_tick,
        )
        .unwrap();

        assert!(matches!(
            run_direct_paired_replay(left, right, left_player, right_player, plan, 0),
            Err(DirectCoordinatorError::UnsafeStatePayload {
                side: Side::Left,
                ordinal: 0
            })
        ));
    }

    #[test]
    fn timed_state_does_not_overwrite_active_external_serial() {
        let mut backend = backend("busy-state.gb");
        let start_tick = backend.game_boy_cpu_cycles().unwrap();
        let EmuBackend::Gb(gb) = &mut backend else {
            unreachable!();
        };
        gb.emu.write_byte(SERIAL_SB, 0x34);
        gb.emu.write_byte(SERIAL_SC, 0x80);
        let state = ReplayGameBoyLinkState {
            peer_present: true,
            pending_master_byte: None,
            pending_master_response: None,
            pending_master_completion_ready: false,
            queued_master_action: None,
            serial_generation: 1,
        };
        let player = player(
            vec![ReplayEvent::GameBoyLinkStateAtTick {
                frame: 0,
                tick: 0,
                state,
            }],
            start_tick,
            "busy-state",
            None,
        );
        let mut side = DirectSide::new(Side::Left, backend, player, 0).unwrap();

        assert!(matches!(
            side.apply_state_events_before(1),
            Err(DirectCoordinatorError::StateOverwritesTransfer {
                side: Side::Left,
                ordinal: 0,
                ..
            })
        ));
        assert_eq!(side.source_cursor, 0);
        assert_ne!(side.backend.game_boy_link_replay_state(), Some(state));
    }

    #[test]
    fn frame_complete_link_work_precedes_checkpoint_and_commit() {
        let mut probe = backend("probe.gb");
        let start_tick = probe.game_boy_cpu_cycles().unwrap();
        let mut cursor = probe.begin_game_boy_frame_slice().unwrap();
        let completion = probe
            .step_game_boy_frame_slice_until(&mut cursor, None, false)
            .unwrap();
        assert_eq!(
            completion.outcome,
            zeff_gb_core::emulator::FrameSliceOutcome::FrameComplete
        );
        let event_tick = probe.game_boy_cpu_cycles().unwrap() - start_tick;
        let nop_count = usize::try_from((event_tick - 20) / 4).unwrap();
        let mut rom = vec![0u8; 0x8000];
        let action_offset = 0x100 + nop_count;
        rom[action_offset..action_offset + 4].copy_from_slice(&[0x3E, 0x81, 0xE0, 0x02]);

        let boundary_pair = || {
            let mut left = backend_from_rom("boundary-left.gb", rom.clone());
            let mut right = backend("boundary-right.gb");
            left.set_link_peer_present(true);
            right.set_link_peer_present(true);
            let EmuBackend::Gb(left_gb) = &mut left else {
                unreachable!();
            };
            left_gb.emu.write_byte(SERIAL_SB, 0xAB);
            let EmuBackend::Gb(right_gb) = &mut right else {
                unreachable!();
            };
            right_gb.emu.write_byte(SERIAL_SB, 0x34);
            right_gb.emu.write_byte(SERIAL_SC, 0x80);
            (left, right)
        };

        let (mut expected_left, mut expected_right) = boundary_pair();
        let target = expected_left.game_boy_cpu_cycles().unwrap() + event_tick;
        let mut left_lease = PairedGameBoyFrameLease::default();
        let mut right_lease = PairedGameBoyFrameLease::default();
        left_lease.begin(&expected_left, None).unwrap();
        right_lease.begin(&expected_right, None).unwrap();
        let left_progress = left_lease
            .step_direct_until(&mut expected_left, Some(target), true)
            .unwrap();
        assert_eq!(
            left_progress.outcome,
            PairedGameBoyFrameLeaseOutcome::FrameComplete
        );
        assert!(left_progress.boundary_reached);
        assert!(left_progress.queued_master_action.is_some());
        assert_eq!(
            right_lease
                .step_direct_until(&mut expected_right, Some(target), true)
                .unwrap()
                .outcome,
            PairedGameBoyFrameLeaseOutcome::FrameComplete
        );
        let preview = expected_left
            .preview_game_boy_link_peer(&expected_right)
            .unwrap();
        let action = preview.local_action.unwrap();
        let reply = preview.peer_reply;
        let mut prepared = expected_left
            .try_prepare_game_boy_link_peer(&mut expected_right)
            .unwrap();
        expected_left
            .try_apply_prepared_game_boy_link_reply(prepared.local_action.take().unwrap())
            .unwrap();
        let left_checkpoint =
            sha256_bytes(&expected_left.encode_replay_hash_state_bytes().unwrap());
        let right_checkpoint =
            sha256_bytes(&expected_right.encode_replay_hash_state_bytes().unwrap());
        let left_events = vec![
            ReplayEvent::GameBoyLink {
                frame: 0,
                tick: event_tick,
                event: ReplayGameBoyLinkEvent::LocalMasterStart {
                    transfer_id: TRANSFER_ID,
                    clock_period_t_cycles: action.clock_period_t_cycles,
                    out_byte: action.out_byte,
                    serial_generation: action.serial_generation,
                },
            },
            ReplayEvent::GameBoyLink {
                frame: 0,
                tick: event_tick,
                event: ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: TRANSFER_ID,
                    out_byte: reply.out_byte,
                    passive: reply.passive,
                    serial_generation: reply.serial_generation,
                },
            },
        ];
        let right_events = vec![ReplayEvent::GameBoyLink {
            frame: 0,
            tick: event_tick,
            event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                transfer_id: TRANSFER_ID,
                clock_period_t_cycles: action.clock_period_t_cycles,
                out_byte: action.out_byte,
                serial_generation: action.serial_generation,
                local_reply: Some(ReplayGameBoyLinkReply {
                    out_byte: reply.out_byte,
                    passive: reply.passive,
                    serial_generation: reply.serial_generation,
                }),
            },
        }];
        let (left, right) = boundary_pair();
        let left_player = player(
            left_events.clone(),
            start_tick,
            "boundary-left",
            Some(left_checkpoint),
        );
        let right_player = player(
            right_events.clone(),
            start_tick,
            "boundary-right",
            Some(right_checkpoint),
        );
        let plan = PairedTransferPlan::build(
            &left_player.metadata().events,
            left_player.metadata().game_boy_link_start_tick,
            &right_player.metadata().events,
            right_player.metadata().game_boy_link_start_tick,
        )
        .unwrap();
        let result =
            run_direct_paired_replay(left, right, left_player, right_player, plan, 0).unwrap();
        assert_eq!(result.frames, 1);
        assert_eq!(result.left_generated_link_events.len(), 2);
        assert_eq!(result.right_generated_link_events.len(), 1);

        let mut delayed_left_events = left_events;
        let ReplayEvent::GameBoyLink { frame, .. } = &mut delayed_left_events[1] else {
            unreachable!();
        };
        *frame = 1;
        let (left, right) = boundary_pair();
        let left_player = player(
            delayed_left_events,
            start_tick,
            "boundary-delayed-left",
            Some(left_checkpoint),
        );
        let right_player = player(right_events, start_tick, "boundary-delayed-right", None);
        let plan = PairedTransferPlan::build(
            &left_player.metadata().events,
            left_player.metadata().game_boy_link_start_tick,
            &right_player.metadata().events,
            right_player.metadata().game_boy_link_start_tick,
        )
        .unwrap();

        let result =
            run_direct_paired_replay(left, right, left_player, right_player, plan, 0).unwrap();
        assert_eq!(result.frames, 2);
        assert_eq!(result.left_generated_link_events[1].frame(), 1);
    }
}
