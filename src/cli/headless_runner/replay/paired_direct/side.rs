use zeff_emu_common::replay::{
    ReplayEvent, ReplayGameBoyLinkAction, ReplayGameBoyLinkState, ReplayPlayer,
};
use zeff_firmware::sha256_hex;
use zeff_gb_core::hardware::bus::GameBoyLinkPreparedTransfer;

use crate::emu_backend::EmuBackend;

use super::super::paired_lease::{
    PairedGameBoyFrameLease, PairedGameBoyFrameLeaseOutcome, PairedGameBoyPointRelation,
};
use super::super::paired_plan::{LocatedEvent, Side};
use super::super::validation::validate_replay_checkpoint;
use super::{DirectCoordinatorError, core_action, located_event, passive_completion_deadline};

pub(super) struct DirectSide {
    pub(super) side: Side,
    pub(super) backend: EmuBackend,
    pub(super) player: ReplayPlayer,
    pub(super) replay_start_tick: u64,
    pub(super) lease: PairedGameBoyFrameLease,
    pub(super) expected_semantics: Vec<(usize, ReplayEvent)>,
    pub(super) semantic_cursor: usize,
    pub(super) source_cursor: usize,
    pub(super) generated: Vec<ReplayEvent>,
    pub(super) recorded_frames: usize,
    pub(super) target_frames: usize,
    pub(super) advanced_frames: usize,
    pub(super) pending_frame_complete: bool,
    pub(super) owned_transfer_until_tick: Option<u64>,
    pub(super) final_state_hash: Option<String>,
    pub(super) final_framebuffer_hash: Option<String>,
}

impl DirectSide {
    pub(super) fn new(
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
        let mut owned_transfer_until_tick = None;
        if let Some(state) = player.metadata().game_boy_link_start_state {
            if !backend.restore_game_boy_link_replay_state(state) {
                return Err(DirectCoordinatorError::UnsafeStartState { side });
            }
            owned_transfer_until_tick = passive_completion_deadline(&backend, state, side)?;
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
            owned_transfer_until_tick,
            final_state_hash: None,
            final_framebuffer_hash: None,
        };
        owner.capture_final_if_due()?;
        Ok(owner)
    }

    pub(super) fn reach(
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

    pub(super) fn reach_exact_point(
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

    pub(super) fn reach_reply_observation(
        &mut self,
        event: &LocatedEvent,
    ) -> Result<(), DirectCoordinatorError> {
        self.apply_state_events_before(event.ordinal)?;
        self.validate_reply_observation_shape(event)
    }

    pub(super) fn validate_reply_observation_shape(
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

    pub(super) fn validate_point_action(
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

    pub(super) fn apply_state_events_before(
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

    pub(super) fn apply_frame_state(
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
        if !self.backend.restore_game_boy_link_replay_state(state) {
            return Err(DirectCoordinatorError::UnsafeStatePayload {
                side: self.side,
                ordinal,
            });
        }
        self.owned_transfer_until_tick =
            passive_completion_deadline(&self.backend, state, self.side)?;
        Ok(())
    }

    pub(super) fn apply_timed_state(
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
        if !self.backend.restore_game_boy_link_replay_state(state) {
            return Err(DirectCoordinatorError::UnsafeStatePayload {
                side: self.side,
                ordinal,
            });
        }
        self.owned_transfer_until_tick =
            passive_completion_deadline(&self.backend, state, self.side)?;
        Ok(())
    }

    pub(super) fn ensure_state_restore_safe(
        &mut self,
        ordinal: usize,
    ) -> Result<(), DirectCoordinatorError> {
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
                    || state.pending_passive_completion.is_some()
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

    pub(super) fn mark_owned_transfer(
        &mut self,
        period: Option<u64>,
    ) -> Result<(), DirectCoordinatorError> {
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

    pub(super) fn begin_frame(&mut self) -> Result<(), DirectCoordinatorError> {
        if !self.lease.needs_frame_setup() {
            return Ok(());
        }
        let frame = self
            .player
            .peek_joypad_frames(0, 1)
            .into_iter()
            .next()
            .unwrap_or_default();
        self.backend.apply_replay_input(&frame);
        self.lease
            .begin(&self.backend, None)
            .map_err(|error| DirectCoordinatorError::Backend(format!("{error:?}")))
    }

    pub(super) fn finish_frame_without_event(&mut self) -> Result<(), DirectCoordinatorError> {
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

    pub(super) fn settle_frame(&mut self) -> Result<(), DirectCoordinatorError> {
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

    pub(super) fn record_validated(
        &mut self,
        event: LocatedEvent,
    ) -> Result<(), DirectCoordinatorError> {
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

    pub(super) fn apply_reply(
        &mut self,
        token: GameBoyLinkPreparedTransfer,
    ) -> Result<(), DirectCoordinatorError> {
        self.backend
            .try_apply_prepared_game_boy_link_reply(token)
            .map(|_| ())
            .map_err(|error| DirectCoordinatorError::Exchange(error.to_string()))
    }

    pub(super) fn finish(&mut self) -> Result<(), DirectCoordinatorError> {
        self.apply_state_events_before(self.player.metadata().events.len())?;
        self.settle_frame()?;
        while self.advanced_frames < self.target_frames {
            self.finish_frame_without_event()?;
        }
        self.capture_final_if_due()
    }

    pub(super) fn capture_final_if_due(&mut self) -> Result<(), DirectCoordinatorError> {
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

    pub(super) fn verify_generated_complete(&self) -> Result<(), DirectCoordinatorError> {
        if self.semantic_cursor != self.expected_semantics.len()
            || self.source_cursor != self.player.metadata().events.len()
        {
            return Err(DirectCoordinatorError::GeneratedOrder { side: self.side });
        }
        Ok(())
    }

    pub(super) fn validate_checkpoint_window(&mut self) -> Result<(), DirectCoordinatorError> {
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
