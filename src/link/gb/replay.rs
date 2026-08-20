use zeff_emu_common::replay::{ReplayEvent, ReplayGameBoyLinkEvent};
use zeff_gb_core::emulator::Emulator as GameBoyEmulator;
use zeff_gb_core::hardware::bus::{GameBoyLinkAction, GameBoyLinkReply};

use crate::link::LinkSessionError;

use super::diagnostics::{format_replay_reply, format_reply};

const PASSIVE_REARM_GRACE_T_CYCLES: u64 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PassiveDrainGate {
    completion_tick: u64,
    rearm_deadline_tick: u64,
}

pub(crate) struct GameBoyReplayLink {
    pub(super) events: Vec<ReplayGameBoyLinkRecord>,
    state_events: Vec<ReplayGameBoyLinkStateRecord>,
    first_undelivered_event: usize,
    local_master_indices: Vec<usize>,
    remote_master_indices: Vec<usize>,
    reply_indices: Vec<usize>,
    local_master_cursor: usize,
    remote_master_cursor: usize,
    reply_cursor: usize,
    pub(super) pending_master_transfer: Option<u64>,
    passive_drain_gate: Option<PassiveDrainGate>,
    link_active: bool,
    local_master_armed: bool,
    strict_local_reply_validation: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ReplayGameBoyLinkRecord {
    pub(super) frame: u64,
    pub(super) tick: u64,
    pub(super) event: ReplayGameBoyLinkEvent,
    pub(super) delivered: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReplayGameBoyLinkStateRecord {
    frame: u64,
    tick: u64,
    state: zeff_emu_common::replay::ReplayGameBoyLinkState,
    delivered: bool,
}

impl GameBoyReplayLink {
    pub(crate) fn new(
        events: Vec<ReplayEvent>,
        base_frame: u64,
        replay_start_tick: Option<u64>,
        playback_start_tick: u64,
    ) -> Self {
        Self::try_new(events, base_frame, replay_start_tick, playback_start_tick)
            .expect("Game Boy replay timestamp should fit playback timeline")
    }

    pub(crate) fn try_new(
        events: Vec<ReplayEvent>,
        base_frame: u64,
        replay_start_tick: Option<u64>,
        playback_start_tick: u64,
    ) -> anyhow::Result<Self> {
        let base_tick = replay_start_tick.map(|_| playback_start_tick).unwrap_or(0);
        let mut records = Vec::new();
        let mut state_records = Vec::new();
        for event in events {
            match event {
                ReplayEvent::GameBoyLink { frame, tick, event } => {
                    let frame = base_frame.checked_add(frame).ok_or_else(|| {
                        anyhow::anyhow!("replay GB event frame overflows playback timeline")
                    })?;
                    let tick = base_tick.checked_add(tick).ok_or_else(|| {
                        anyhow::anyhow!("replay GB event tick overflows playback timeline")
                    })?;
                    records.push(ReplayGameBoyLinkRecord {
                        frame,
                        tick,
                        event,
                        delivered: false,
                    });
                }
                ReplayEvent::GameBoyLinkStateAtTick { frame, tick, state } => {
                    let frame = base_frame.checked_add(frame).ok_or_else(|| {
                        anyhow::anyhow!("replay GB state frame overflows playback timeline")
                    })?;
                    let tick = base_tick.checked_add(tick).ok_or_else(|| {
                        anyhow::anyhow!("replay GB state tick overflows playback timeline")
                    })?;
                    state_records.push(ReplayGameBoyLinkStateRecord {
                        frame,
                        tick,
                        state,
                        delivered: false,
                    });
                }
                _ => {}
            }
        }
        records.sort_by_key(|record| {
            (
                record.frame,
                record.tick,
                gb_replay_event_sort_key(&record.event),
            )
        });
        state_records.sort_by_key(|record| (record.frame, record.tick));
        let mut local_master_indices = Vec::new();
        let mut remote_master_indices = Vec::new();
        let mut reply_indices = Vec::new();
        for (index, record) in records.iter().enumerate() {
            match record.event {
                ReplayGameBoyLinkEvent::LocalMasterStart { .. } => local_master_indices.push(index),
                ReplayGameBoyLinkEvent::RemoteMasterStart { .. } => {
                    remote_master_indices.push(index)
                }
                ReplayGameBoyLinkEvent::RemoteReply { .. } => reply_indices.push(index),
            }
        }
        Ok(Self {
            events: records,
            state_events: state_records,
            first_undelivered_event: 0,
            local_master_indices,
            remote_master_indices,
            reply_indices,
            local_master_cursor: 0,
            remote_master_cursor: 0,
            reply_cursor: 0,
            pending_master_transfer: None,
            passive_drain_gate: None,
            link_active: false,
            local_master_armed: false,
            strict_local_reply_validation: false,
        })
    }

    pub(crate) fn set_strict_local_reply_validation(&mut self, strict: bool) {
        self.strict_local_reply_validation = strict;
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.events.is_empty() && self.state_events.is_empty()
    }

    pub(crate) fn event_progress(&self) -> (usize, usize) {
        (
            self.events.iter().filter(|record| record.delivered).count(),
            self.events.len(),
        )
    }

    pub(crate) fn all_events_delivered(&self) -> bool {
        self.first_undelivered_event == self.events.len()
    }

    pub(crate) fn poll_emulator(
        &mut self,
        emulator: &mut GameBoyEmulator,
    ) -> Result<(), LinkSessionError> {
        self.apply_due_state_events(emulator);
        self.sync_peer_presence_for_next_event(emulator);
        self.apply_due_remote_master_starts(emulator)?;
        self.sync_peer_presence_for_next_event(emulator);
        self.send_pending_master_start(emulator)?;
        self.apply_due_events(emulator)?;
        if emulator.game_boy_link_waiting_at_completion_boundary() {
            self.apply_pending_reply_at_boundary(emulator)?;
        }
        Ok(())
    }

    pub(crate) fn trace_wait_boundary(&mut self, _cycle: u64, _context: &str) {}

    fn sync_peer_presence_for_next_event(&mut self, emulator: &mut GameBoyEmulator) {
        if self.pending_master_transfer.is_some() {
            self.set_replay_peer_present(emulator, true);
            return;
        }

        match self.next_undelivered_event() {
            Some(record)
                if matches!(
                    record.event,
                    ReplayGameBoyLinkEvent::LocalMasterStart { .. }
                ) =>
            {
                if self.event_frame_is_due(emulator, record) {
                    self.set_replay_peer_present(emulator, true);
                }
            }
            Some(record) if matches!(record.event, ReplayGameBoyLinkEvent::RemoteReply { .. }) => {
                if self.event_is_due(emulator, record)
                    || self.next_undelivered_local_master_start_index().is_none()
                {
                    self.set_replay_peer_present(emulator, true);
                }
            }
            Some(record)
                if matches!(
                    record.event,
                    ReplayGameBoyLinkEvent::RemoteMasterStart { .. }
                ) =>
            {
                // Remote-master timing does not own cable-present state.
            }
            None => {
                // Keep the last cable state after the event stream ends.
            }
            Some(_) => unreachable!("all Game Boy replay event variants are handled"),
        }
    }

    fn apply_due_state_events(&mut self, emulator: &mut GameBoyEmulator) {
        for record in &mut self.state_events {
            if record.delivered
                || record.frame > emulator.frame_count()
                || (record.frame == emulator.frame_count() && record.tick > emulator.cpu_cycles())
            {
                continue;
            }
            emulator.restore_game_boy_link_replay_state(record.state);
            record.delivered = true;
        }
    }

    fn event_is_due(&self, emulator: &GameBoyEmulator, record: ReplayGameBoyLinkRecord) -> bool {
        record.frame < emulator.frame_count()
            || (record.frame == emulator.frame_count() && record.tick <= emulator.cpu_cycles())
    }

    fn event_frame_is_due(
        &self,
        emulator: &GameBoyEmulator,
        record: ReplayGameBoyLinkRecord,
    ) -> bool {
        record.frame <= emulator.frame_count()
    }

    fn set_replay_peer_present(&mut self, emulator: &mut GameBoyEmulator, present: bool) {
        if present {
            emulator.set_game_boy_link_peer_present(true);
            self.local_master_armed = true;
        } else {
            emulator.restore_game_boy_link_peer_present_without_action(false);
            self.local_master_armed = false;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn debug_summary(&self) -> String {
        let (delivered, total) = self.event_progress();
        let next_master = self
            .next_undelivered_remote_master_start_index()
            .map(|index| format_replay_record(index, self.events[index]))
            .unwrap_or_else(|| "none".to_string());
        let next_reply = self
            .next_undelivered_reply_transfer_id()
            .map(|transfer_id| format!("id={transfer_id}"))
            .unwrap_or_else(|| "none".to_string());
        format!(
            "delivered={}/{} pending={:?} active={} next_master={} next_reply={}",
            delivered,
            total,
            self.pending_master_transfer,
            self.link_active,
            next_master,
            next_reply
        )
    }

    fn send_pending_master_start(
        &mut self,
        emulator: &mut GameBoyEmulator,
    ) -> Result<(), LinkSessionError> {
        let Some(action) = emulator.take_game_boy_link_action() else {
            return Ok(());
        };

        if self.pending_master_transfer.is_some() {
            return Err(LinkSessionError::MalformedPacketPayload);
        }

        let transfer_id = if let Some(transfer_id) =
            self.take_next_local_master_start(emulator, action)?
        {
            transfer_id
        } else if let Some(transfer_id) = self.next_undelivered_reply_transfer_id() {
            transfer_id
        } else {
            log::warn!(
                "GB replay local master has no recorded reply: tick={} out={:02X} period={} gen={}",
                emulator.cpu_cycles(),
                action.out_byte,
                action.clock_period_t_cycles,
                action.serial_generation
            );
            return Err(LinkSessionError::MalformedPacketPayload);
        };

        self.pending_master_transfer = Some(transfer_id);
        Ok(())
    }

    fn take_next_local_master_start(
        &mut self,
        emulator: &GameBoyEmulator,
        action: GameBoyLinkAction,
    ) -> Result<Option<u64>, LinkSessionError> {
        let Some(index) = self.next_undelivered_local_master_start_index() else {
            return Ok(None);
        };
        let record = self.events[index];
        if record.frame > emulator.frame_count() || record.tick > emulator.cpu_cycles() {
            log::warn!(
                "GB replay local master is earlier than recorded event: current frame={} tick={} next={}",
                emulator.frame_count(),
                emulator.cpu_cycles(),
                format_replay_record(index, record)
            );
            return Err(LinkSessionError::MalformedPacketPayload);
        }
        let ReplayGameBoyLinkEvent::LocalMasterStart {
            transfer_id,
            clock_period_t_cycles,
            out_byte,
            serial_generation,
        } = record.event
        else {
            unreachable!("next_undelivered_local_master_start_index must point to local start");
        };
        if out_byte != action.out_byte
            || clock_period_t_cycles != action.clock_period_t_cycles
            || serial_generation != action.serial_generation
        {
            log::warn!(
                "GB replay local master mismatch: expected {}, actual out={:02X} period={} gen={}",
                format_replay_record(index, record),
                action.out_byte,
                action.clock_period_t_cycles,
                action.serial_generation
            );
            return Err(LinkSessionError::MalformedPacketPayload);
        }
        self.mark_event_delivered(index);
        self.link_active = true;
        Ok(Some(transfer_id))
    }

    fn apply_due_remote_master_starts(
        &mut self,
        emulator: &mut GameBoyEmulator,
    ) -> Result<(), LinkSessionError> {
        loop {
            if !self.passive_completion_allows_next_remote_start(emulator) {
                return Ok(());
            }
            let Some(index) = self.next_undelivered_remote_master_start_index() else {
                return Ok(());
            };
            let record = self.events[index];
            if record.frame > emulator.frame_count() || record.tick > emulator.cpu_cycles() {
                return Ok(());
            }
            self.mark_event_delivered(index);
            self.link_active = true;
            self.apply_event(emulator, record.event)?;
        }
    }

    fn first_due_undelivered_event(
        &self,
        emulator: &GameBoyEmulator,
    ) -> Option<ReplayGameBoyLinkEvent> {
        self.events
            .iter()
            .skip(self.first_undelivered_event)
            .find_map(|record| {
                if record.delivered
                    || record.frame > emulator.frame_count()
                    || record.tick > emulator.cpu_cycles()
                {
                    return None;
                }
                Some(record.event)
            })
    }

    fn next_undelivered_event(&self) -> Option<ReplayGameBoyLinkRecord> {
        self.events
            .iter()
            .skip(self.first_undelivered_event)
            .copied()
            .find(|record| !record.delivered)
    }

    fn apply_due_events(&mut self, emulator: &mut GameBoyEmulator) -> Result<(), LinkSessionError> {
        let Some(transfer_id) = self.pending_master_transfer else {
            return Ok(());
        };
        loop {
            let Some(index) = self.find_pending_reply_index(transfer_id) else {
                return Ok(());
            };
            let record = self.events[index];
            self.mark_event_delivered(index);
            self.apply_event(emulator, record.event)?;
        }
    }

    fn apply_pending_reply_at_boundary(
        &mut self,
        emulator: &mut GameBoyEmulator,
    ) -> Result<(), LinkSessionError> {
        let Some(transfer_id) = self.pending_master_transfer else {
            return Ok(());
        };
        let Some(index) = self.find_pending_reply_index(transfer_id) else {
            return Ok(());
        };
        let event = self.events[index].event;
        self.mark_event_delivered(index);
        self.apply_event(emulator, event)
    }

    fn apply_event(
        &mut self,
        emulator: &mut GameBoyEmulator,
        event: ReplayGameBoyLinkEvent,
    ) -> Result<(), LinkSessionError> {
        match event {
            ReplayGameBoyLinkEvent::LocalMasterStart { .. } => {
                Err(LinkSessionError::MalformedPacketPayload)
            }
            ReplayGameBoyLinkEvent::RemoteMasterStart {
                clock_period_t_cycles,
                out_byte,
                local_reply,
                ..
            } => {
                let actual = emulator.game_boy_link_reply_to_master_start();
                if let Some(reply) = local_reply
                    && (actual.passive != reply.passive
                        || actual.out_byte != reply.out_byte
                        || actual.serial_generation != reply.serial_generation)
                {
                    if self.strict_local_reply_validation {
                        return Err(LinkSessionError::MalformedPacketPayload);
                    }
                    log::warn!(
                        "GB replay remote-master local reply mismatch: expected {}, actual {}",
                        format_replay_reply(reply),
                        format_reply(actual)
                    );
                }
                if actual.passive {
                    if clock_period_t_cycles == 0 {
                        return Err(LinkSessionError::MalformedPacketPayload);
                    }
                    let completion_tick = emulator
                        .cpu_cycles()
                        .checked_add(clock_period_t_cycles)
                        .ok_or(LinkSessionError::MalformedPacketPayload)?;
                    let rearm_deadline_tick = completion_tick
                        .checked_add(clock_period_t_cycles.max(PASSIVE_REARM_GRACE_T_CYCLES))
                        .ok_or(LinkSessionError::MalformedPacketPayload)?;
                    if !emulator
                        .schedule_game_boy_external_link_transfer(out_byte, clock_period_t_cycles)
                    {
                        return Err(LinkSessionError::MalformedPacketPayload);
                    }
                    self.passive_drain_gate = Some(PassiveDrainGate {
                        completion_tick,
                        rearm_deadline_tick,
                    });
                }
                Ok(())
            }
            ReplayGameBoyLinkEvent::RemoteReply {
                transfer_id,
                out_byte,
                passive,
                serial_generation,
            } => {
                if self.pending_master_transfer != Some(transfer_id) {
                    return Err(LinkSessionError::MalformedPacketPayload);
                }
                let applied = emulator.apply_game_boy_link_reply(GameBoyLinkReply {
                    out_byte,
                    passive,
                    serial_generation,
                });
                if !applied {
                    return Err(LinkSessionError::MalformedPacketPayload);
                }
                self.pending_master_transfer = None;
                Ok(())
            }
        }
    }

    fn next_undelivered_reply_transfer_id(&self) -> Option<u64> {
        let index = *self.reply_indices.get(self.reply_cursor)?;
        let ReplayGameBoyLinkEvent::RemoteReply { transfer_id, .. } = self.events[index].event
        else {
            unreachable!("reply index must point to reply event");
        };
        Some(transfer_id)
    }

    fn next_undelivered_remote_master_start_index(&self) -> Option<usize> {
        self.remote_master_indices
            .get(self.remote_master_cursor)
            .copied()
    }

    fn passive_completion_allows_next_remote_start(&mut self, emulator: &GameBoyEmulator) -> bool {
        let Some(gate) = self.passive_drain_gate else {
            return true;
        };
        let tick = emulator.cpu_cycles();
        if tick < gate.completion_tick {
            return false;
        }
        let next_reply_allows_early_nonpassive = self
            .next_undelivered_remote_master_start_index()
            .and_then(|index| match self.events[index].event {
                ReplayGameBoyLinkEvent::RemoteMasterStart { local_reply, .. } => local_reply,
                _ => None,
            })
            .is_some_and(|reply| !reply.passive);
        if !next_reply_allows_early_nonpassive
            && tick < gate.rearm_deadline_tick
            && !emulator.game_boy_link_reply_to_master_start().passive
        {
            return false;
        }
        self.passive_drain_gate = None;
        true
    }

    fn next_undelivered_local_master_start_index(&self) -> Option<usize> {
        self.local_master_indices
            .get(self.local_master_cursor)
            .copied()
    }

    fn find_pending_reply_index(&self, transfer_id: u64) -> Option<usize> {
        self.reply_indices
            .iter()
            .skip(self.reply_cursor)
            .copied()
            .find(|index| {
                !self.events[*index].delivered
                    && matches!(
                        self.events[*index].event,
                        ReplayGameBoyLinkEvent::RemoteReply {
                            transfer_id: event_transfer_id,
                            ..
                        } if event_transfer_id == transfer_id
                    )
            })
    }

    fn mark_event_delivered(&mut self, index: usize) {
        self.events[index].delivered = true;
        match self.events[index].event {
            ReplayGameBoyLinkEvent::LocalMasterStart { .. } => advance_index_cursor(
                &self.events,
                &self.local_master_indices,
                &mut self.local_master_cursor,
            ),
            ReplayGameBoyLinkEvent::RemoteMasterStart { .. } => advance_index_cursor(
                &self.events,
                &self.remote_master_indices,
                &mut self.remote_master_cursor,
            ),
            ReplayGameBoyLinkEvent::RemoteReply { .. } => {
                advance_index_cursor(&self.events, &self.reply_indices, &mut self.reply_cursor)
            }
        }
        while self
            .events
            .get(self.first_undelivered_event)
            .is_some_and(|record| record.delivered)
        {
            self.first_undelivered_event += 1;
        }
    }
}

fn advance_index_cursor(events: &[ReplayGameBoyLinkRecord], indices: &[usize], cursor: &mut usize) {
    while indices
        .get(*cursor)
        .is_some_and(|index| events[*index].delivered)
    {
        *cursor += 1;
    }
}

fn gb_replay_event_sort_key(event: &ReplayGameBoyLinkEvent) -> u8 {
    match event {
        ReplayGameBoyLinkEvent::LocalMasterStart { .. } => 0,
        ReplayGameBoyLinkEvent::RemoteMasterStart { .. } => 1,
        ReplayGameBoyLinkEvent::RemoteReply { .. } => 2,
    }
}

fn format_replay_record(index: usize, record: ReplayGameBoyLinkRecord) -> String {
    let kind = match record.event {
        ReplayGameBoyLinkEvent::LocalMasterStart { transfer_id, .. } => {
            format!("local_master id={transfer_id}")
        }
        ReplayGameBoyLinkEvent::RemoteMasterStart { transfer_id, .. } => {
            format!("remote_master id={transfer_id}")
        }
        ReplayGameBoyLinkEvent::RemoteReply { transfer_id, .. } => {
            format!("reply id={transfer_id}")
        }
    };
    format!(
        "#{index} frame={} tick={} {kind}",
        record.frame, record.tick
    )
}
