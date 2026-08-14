use zeff_emu_common::replay::{ReplayEvent, ReplayGameBoyLinkEvent};
use zeff_gb_core::emulator::Emulator as GameBoyEmulator;
use zeff_gb_core::hardware::bus::{GameBoyLinkAction, GameBoyLinkReply};

use crate::link::LinkSessionError;

use super::diagnostics::{format_replay_reply, format_reply};

pub(crate) struct GameBoyReplayLink {
    pub(super) events: Vec<ReplayGameBoyLinkRecord>,
    base_frame: u64,
    base_tick: u64,
    pub(super) pending_master_transfer: Option<u64>,
    link_active: bool,
    local_master_armed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ReplayGameBoyLinkRecord {
    pub(super) frame: u64,
    pub(super) tick: u64,
    pub(super) event: ReplayGameBoyLinkEvent,
    pub(super) delivered: bool,
}

impl GameBoyReplayLink {
    pub(crate) fn new(
        events: Vec<ReplayEvent>,
        base_frame: u64,
        replay_start_tick: Option<u64>,
        playback_start_tick: u64,
    ) -> Self {
        let mut events: Vec<_> = events
            .into_iter()
            .filter_map(|event| {
                if let ReplayEvent::GameBoyLink { frame, tick, event } = event {
                    Some(ReplayGameBoyLinkRecord {
                        frame,
                        tick,
                        event,
                        delivered: false,
                    })
                } else {
                    None
                }
            })
            .collect();
        events.sort_by_key(|record| {
            (
                record.frame,
                record.tick,
                gb_replay_event_sort_key(&record.event),
            )
        });
        Self {
            events,
            base_frame,
            base_tick: replay_start_tick.map(|_| playback_start_tick).unwrap_or(0),
            pending_master_transfer: None,
            link_active: false,
            local_master_armed: false,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub(crate) fn poll_emulator(
        &mut self,
        emulator: &mut GameBoyEmulator,
    ) -> Result<(), LinkSessionError> {
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
                self.set_replay_peer_present(emulator, self.event_frame_is_due(emulator, record));
            }
            Some(record) if matches!(record.event, ReplayGameBoyLinkEvent::RemoteReply { .. }) => {
                self.set_replay_peer_present(
                    emulator,
                    self.event_is_due(emulator, record)
                        || self.next_undelivered_local_master_start_index().is_none(),
                );
            }
            Some(record)
                if matches!(
                    record.event,
                    ReplayGameBoyLinkEvent::RemoteMasterStart { .. }
                ) =>
            {
                // A future or next remote-master event does not imply the cable
                // is absent. Frame-boundary GameBoyLinkState events and replay
                // start state carry the authoritative peer-present timeline.
                // Forcing the peer absent here changes games that poll serial
                // state while waiting for the remote endpoint to become master.
            }
            None => {
                // Preserve the last recorded/restored peer-present state after
                // the final semantic transfer. The recording may end while the
                // cable is still connected.
            }
            Some(_) => unreachable!("all Game Boy replay event variants are handled"),
        }
    }

    fn event_is_due(&self, emulator: &GameBoyEmulator, record: ReplayGameBoyLinkRecord) -> bool {
        let frame = self.absolute_event_frame(record.frame);
        frame < emulator.frame_count()
            || (frame == emulator.frame_count()
                && self.absolute_event_tick(record.tick) <= emulator.cpu_cycles())
    }

    fn event_frame_is_due(
        &self,
        emulator: &GameBoyEmulator,
        record: ReplayGameBoyLinkRecord,
    ) -> bool {
        self.absolute_event_frame(record.frame) <= emulator.frame_count()
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
        let delivered = self.events.iter().filter(|record| record.delivered).count();
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
            self.events.len(),
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
        if self.absolute_event_frame(record.frame) > emulator.frame_count()
            || self.absolute_event_tick(record.tick) > emulator.cpu_cycles()
        {
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
        self.events[index].delivered = true;
        self.link_active = true;
        Ok(Some(transfer_id))
    }

    fn apply_due_remote_master_starts(
        &mut self,
        emulator: &mut GameBoyEmulator,
    ) -> Result<(), LinkSessionError> {
        loop {
            let Some(index) = self.next_undelivered_remote_master_start_index() else {
                return Ok(());
            };
            let record = self.events[index];
            if self.absolute_event_frame(record.frame) > emulator.frame_count()
                || self.absolute_event_tick(record.tick) > emulator.cpu_cycles()
            {
                return Ok(());
            }
            self.events[index].delivered = true;
            self.link_active = true;
            self.apply_event(emulator, record.event)?;
        }
    }

    fn absolute_event_frame(&self, replay_frame: u64) -> u64 {
        self.base_frame.saturating_add(replay_frame)
    }

    pub(super) fn absolute_event_tick(&self, replay_tick: u64) -> u64 {
        self.base_tick.saturating_add(replay_tick)
    }

    fn first_due_undelivered_event(
        &self,
        emulator: &GameBoyEmulator,
    ) -> Option<ReplayGameBoyLinkEvent> {
        self.events.iter().find_map(|record| {
            if record.delivered
                || self.absolute_event_frame(record.frame) > emulator.frame_count()
                || self.absolute_event_tick(record.tick) > emulator.cpu_cycles()
            {
                return None;
            }
            Some(record.event)
        })
    }

    fn next_undelivered_event(&self) -> Option<ReplayGameBoyLinkRecord> {
        self.events.iter().copied().find(|record| !record.delivered)
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
            self.events[index].delivered = true;
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
        self.events[index].delivered = true;
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
                out_byte,
                local_reply,
                ..
            } => {
                let passive = if let Some(reply) = local_reply {
                    let actual = emulator.game_boy_link_reply_to_master_start();
                    if actual.passive != reply.passive
                        || actual.out_byte != reply.out_byte
                        || actual.serial_generation != reply.serial_generation
                    {
                        log::warn!(
                            "GB replay remote-master local reply mismatch: expected {}, actual {}",
                            format_replay_reply(reply),
                            format_reply(actual)
                        );
                        return Err(LinkSessionError::MalformedPacketPayload);
                    }
                    reply.passive
                } else {
                    emulator.game_boy_link_reply_to_master_start().passive
                };
                if passive && !emulator.complete_game_boy_external_link_transfer(out_byte) {
                    return Err(LinkSessionError::MalformedPacketPayload);
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
        self.events.iter().find_map(|record| {
            if record.delivered {
                return None;
            }
            if let ReplayGameBoyLinkEvent::RemoteReply { transfer_id, .. } = record.event {
                Some(transfer_id)
            } else {
                None
            }
        })
    }

    fn next_undelivered_remote_master_start_index(&self) -> Option<usize> {
        self.events.iter().position(|record| {
            !record.delivered
                && matches!(
                    record.event,
                    ReplayGameBoyLinkEvent::RemoteMasterStart { .. }
                )
        })
    }

    fn next_undelivered_local_master_start_index(&self) -> Option<usize> {
        self.events.iter().position(|record| {
            !record.delivered
                && matches!(
                    record.event,
                    ReplayGameBoyLinkEvent::LocalMasterStart { .. }
                )
        })
    }

    fn find_pending_reply_index(&self, transfer_id: u64) -> Option<usize> {
        self.events.iter().position(|record| {
            !record.delivered
                && matches!(
                    record.event,
                    ReplayGameBoyLinkEvent::RemoteReply {
                        transfer_id: event_transfer_id,
                        ..
                    } if event_transfer_id == transfer_id
                )
        })
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
