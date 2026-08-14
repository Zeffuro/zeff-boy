#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use zeff_emu_common::replay::{ReplayEvent, ReplayGameBoyLinkEvent, ReplayGameBoyLinkReply};
use zeff_gb_core::emulator::Emulator as GameBoyEmulator;
use zeff_gb_core::hardware::bus::{GameBoyLinkAction, GameBoyLinkReply};

use crate::emu_backend::EmuBackend;

use super::{LinkConnectionState, LinkPacketKind, LinkSession, LinkSessionError, LinkTransport};

const MASTER_REPLY_SPIN_LIMIT: usize = 64;
const PASSIVE_REARM_CATCHUP_T_CYCLES: u64 = 4096;
const PASSIVE_REARM_CATCHUP_INSTRUCTIONS: usize = 256;
const EVENT_MASTER_START: u8 = 1;
const EVENT_TRANSFER_REPLY: u8 = 2;

const MASTER_START_PAYLOAD_LEN: usize = 1 + 8 + 8 + 8 + 1 + 8;
const TRANSFER_REPLY_PAYLOAD_LEN: usize = 1 + 8 + 8 + 1 + 1 + 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GbTransferId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingGameBoyMasterTransfer {
    id: GbTransferId,
    start_tick: u64,
    clock_period_t_cycles: u64,
    out_byte: u8,
    serial_generation: u64,
    boundary_waits: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GameBoyLinkPayloadError {
    WrongLength { expected: usize, actual: usize },
    UnknownEvent(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GameBoyLinkEvent {
    MasterStart {
        transfer_id: GbTransferId,
        start_tick: u64,
        action: GameBoyLinkAction,
    },
    TransferReply {
        transfer_id: GbTransferId,
        sample_tick: u64,
        reply: GameBoyLinkReply,
    },
}

pub(crate) struct GameBoyRemoteLink<T: LinkTransport> {
    session: LinkSession<T>,
    next_transfer_id: u64,
    pending_master_transfer: Option<PendingGameBoyMasterTransfer>,
    passive_rearm_catchup_after_tick: Option<u64>,
    recorded_replay_events: Vec<ReplayEvent>,
    #[cfg(not(target_arch = "wasm32"))]
    trace: Option<LinkTrace>,
}

pub(crate) struct GameBoyReplayLink {
    events: Vec<ReplayGameBoyLinkRecord>,
    base_frame: u64,
    base_tick: u64,
    pending_master_transfer: Option<u64>,
    link_active: bool,
    local_master_armed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReplayGameBoyLinkRecord {
    frame: u64,
    tick: u64,
    event: ReplayGameBoyLinkEvent,
    delivered: bool,
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
            serial_generation: _,
        } = record.event
        else {
            unreachable!("next_undelivered_local_master_start_index must point to local start");
        };
        if out_byte != action.out_byte || clock_period_t_cycles != action.clock_period_t_cycles {
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

    fn absolute_event_tick(&self, replay_tick: u64) -> u64 {
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
                    if actual.passive != reply.passive || actual.out_byte != reply.out_byte {
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

impl<T: LinkTransport> GameBoyRemoteLink<T> {
    pub(crate) fn new(session: LinkSession<T>) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let trace = LinkTrace::from_env(session.endpoint());
        Self {
            session,
            next_transfer_id: 0,
            pending_master_transfer: None,
            passive_rearm_catchup_after_tick: None,
            recorded_replay_events: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            trace,
        }
    }

    pub(crate) fn state(&self) -> LinkConnectionState {
        self.session.state()
    }

    pub(crate) fn poll_backend(
        &mut self,
        backend: &mut EmuBackend,
    ) -> Result<(), LinkSessionError> {
        if self.state() == LinkConnectionState::Disconnected {
            backend.set_link_peer_present(false);
            return Err(LinkSessionError::Transport(
                super::LinkTransportError::Disconnected,
            ));
        }
        let EmuBackend::Gb(gb) = backend else {
            return Err(LinkSessionError::IncompatibleSystems);
        };
        self.poll_emulator(&mut gb.emu)
    }

    pub(crate) fn poll_emulator(
        &mut self,
        emulator: &mut GameBoyEmulator,
    ) -> Result<(), LinkSessionError> {
        if self.state() == LinkConnectionState::Disconnected {
            emulator.set_game_boy_link_peer_present(false);
            return Err(LinkSessionError::Transport(
                super::LinkTransportError::Disconnected,
            ));
        }

        emulator.set_game_boy_link_peer_present(true);
        self.drain_incoming(emulator)?;
        self.send_pending_master_start(emulator)?;
        if emulator.game_boy_link_waiting_at_completion_boundary() {
            self.poll_for_pending_master_reply(emulator)?;
        }
        self.drain_incoming(emulator)?;

        Ok(())
    }

    pub(crate) fn disconnect(&mut self) {
        let _ = self.session.send(LinkPacketKind::Disconnect, &[]);
        self.session.disconnect();
    }

    pub(crate) fn pending_master_transfer_id(&self) -> Option<u64> {
        self.pending_master_transfer.map(|pending| pending.id.0)
    }

    pub(crate) fn take_replay_events(&mut self) -> Vec<ReplayEvent> {
        std::mem::take(&mut self.recorded_replay_events)
    }

    pub(crate) fn trace_wait_boundary(&mut self, cycle: u64, context: &str) {
        let message = if let Some(pending) = &mut self.pending_master_transfer {
            pending.boundary_waits = pending.boundary_waits.saturating_add(1);
            format!(
                "wait completion_boundary context={} tick={} pending={} start={} elapsed={} period={} out={:02X} gen={} waits={}",
                context,
                cycle,
                pending.id.0,
                pending.start_tick,
                cycle.saturating_sub(pending.start_tick),
                pending.clock_period_t_cycles,
                pending.out_byte,
                pending.serial_generation,
                pending.boundary_waits
            )
        } else {
            format!(
                "wait completion_boundary context={} tick={} pending=None",
                context, cycle
            )
        };
        self.trace(message);
    }

    pub(crate) fn trace_wait_pending_master(&mut self, cycle: u64, context: &str) {
        let message = if let Some(pending) = &self.pending_master_transfer {
            format!(
                "wait pending_master context={} tick={} pending={} start={} elapsed={} period={} out={:02X} gen={}",
                context,
                cycle,
                pending.id.0,
                pending.start_tick,
                cycle.saturating_sub(pending.start_tick),
                pending.clock_period_t_cycles,
                pending.out_byte,
                pending.serial_generation
            )
        } else {
            format!(
                "wait pending_master context={} tick={} pending=None",
                context, cycle
            )
        };
        self.trace(message);
    }

    fn drain_incoming(&mut self, emulator: &mut GameBoyEmulator) -> Result<(), LinkSessionError> {
        loop {
            let Some(packet) = self.session.try_receive_packet()? else {
                return Ok(());
            };

            match packet.kind {
                LinkPacketKind::LinkEvent => {
                    let event = decode_game_boy_link_event(&packet.payload)
                        .map_err(|_| LinkSessionError::MalformedPacketPayload)?;
                    self.handle_event(emulator, event)?;
                }
                LinkPacketKind::Disconnect => {
                    self.trace("recv disconnect".to_string());
                    self.session.disconnect();
                    return Err(LinkSessionError::Transport(
                        super::LinkTransportError::Disconnected,
                    ));
                }
                LinkPacketKind::Hello | LinkPacketKind::LinkState => {}
            }
        }
    }

    fn handle_event(
        &mut self,
        emulator: &mut GameBoyEmulator,
        event: GameBoyLinkEvent,
    ) -> Result<(), LinkSessionError> {
        match event {
            GameBoyLinkEvent::MasterStart {
                transfer_id,
                start_tick,
                action,
            } => {
                self.catch_up_after_passive_completion(emulator, action.clock_period_t_cycles);
                let reply = emulator.game_boy_link_reply_to_master_start();
                self.trace(format!(
                    "recv master_start id={} tick={} out={:02X} period={} gen={} reply={}",
                    transfer_id.0,
                    start_tick,
                    action.out_byte,
                    action.clock_period_t_cycles,
                    action.serial_generation,
                    format_reply(reply)
                ));
                self.record_replay_event(
                    emulator,
                    ReplayGameBoyLinkEvent::RemoteMasterStart {
                        transfer_id: transfer_id.0,
                        clock_period_t_cycles: action.clock_period_t_cycles,
                        out_byte: action.out_byte,
                        serial_generation: action.serial_generation,
                        local_reply: Some(ReplayGameBoyLinkReply {
                            out_byte: reply.out_byte,
                            passive: reply.passive,
                            serial_generation: reply.serial_generation,
                        }),
                    },
                );
                self.send_event(GameBoyLinkEvent::TransferReply {
                    transfer_id,
                    sample_tick: emulator.cpu_cycles(),
                    reply,
                })?;
                if reply.passive {
                    if emulator.complete_game_boy_external_link_transfer(action.out_byte) {
                        self.trace(format!(
                            "complete passive id={} in={:02X}",
                            transfer_id.0, action.out_byte
                        ));
                        self.passive_rearm_catchup_after_tick = Some(emulator.cpu_cycles());
                    } else {
                        self.trace(format!(
                            "passive completion missed id={} in={:02X}",
                            transfer_id.0, action.out_byte
                        ));
                    }
                }
            }
            GameBoyLinkEvent::TransferReply {
                transfer_id,
                sample_tick,
                reply,
            } => {
                self.trace(format!(
                    "recv reply id={} tick={} {}",
                    transfer_id.0,
                    sample_tick,
                    format_reply(reply)
                ));
                if self.pending_master_transfer.map(|pending| pending.id) != Some(transfer_id) {
                    self.trace(format!(
                        "protocol fault unexpected_reply id={} pending={:?}",
                        transfer_id.0,
                        self.pending_master_transfer.map(|pending| pending.id.0)
                    ));
                    return Err(LinkSessionError::MalformedPacketPayload);
                }
                if emulator.apply_game_boy_link_reply(reply) {
                    self.record_replay_event(
                        emulator,
                        ReplayGameBoyLinkEvent::RemoteReply {
                            transfer_id: transfer_id.0,
                            out_byte: reply.out_byte,
                            passive: reply.passive,
                            serial_generation: reply.serial_generation,
                        },
                    );
                    self.pending_master_transfer = None;
                    self.trace(format!(
                        "bound reply id={} in={:02X} passive={}",
                        transfer_id.0, reply.out_byte, reply.passive
                    ));
                } else {
                    self.trace(format!("reply rejected id={}", transfer_id.0));
                    return Err(LinkSessionError::MalformedPacketPayload);
                }
            }
        }

        Ok(())
    }

    fn send_pending_master_start(
        &mut self,
        emulator: &mut GameBoyEmulator,
    ) -> Result<(), LinkSessionError> {
        let Some(action) = emulator.take_game_boy_link_action() else {
            return Ok(());
        };

        if self.pending_master_transfer.is_some() {
            self.trace(format!(
                "protocol fault local_master_while_pending pending={:?} out={:02X}",
                self.pending_master_transfer.map(|pending| pending.id.0),
                action.out_byte
            ));
            return Err(LinkSessionError::MalformedPacketPayload);
        }

        let transfer_id = self.allocate_transfer_id();
        self.pending_master_transfer = Some(PendingGameBoyMasterTransfer {
            id: transfer_id,
            start_tick: emulator.cpu_cycles(),
            clock_period_t_cycles: action.clock_period_t_cycles,
            out_byte: action.out_byte,
            serial_generation: action.serial_generation,
            boundary_waits: 0,
        });
        self.trace(format!(
            "send master_start id={} tick={} out={:02X} period={} gen={}",
            transfer_id.0,
            emulator.cpu_cycles(),
            action.out_byte,
            action.clock_period_t_cycles,
            action.serial_generation
        ));
        self.record_replay_event(
            emulator,
            ReplayGameBoyLinkEvent::LocalMasterStart {
                transfer_id: transfer_id.0,
                clock_period_t_cycles: action.clock_period_t_cycles,
                out_byte: action.out_byte,
                serial_generation: action.serial_generation,
            },
        );
        self.send_event(GameBoyLinkEvent::MasterStart {
            transfer_id,
            start_tick: emulator.cpu_cycles(),
            action,
        })
    }

    fn catch_up_after_passive_completion(
        &mut self,
        emulator: &mut GameBoyEmulator,
        clock_period_t_cycles: u64,
    ) {
        let Some(completed_tick) = self.passive_rearm_catchup_after_tick else {
            return;
        };

        if emulator.game_boy_link_reply_to_master_start().passive {
            self.passive_rearm_catchup_after_tick = None;
            return;
        }

        let start_tick = emulator.cpu_cycles();
        let cycle_budget = clock_period_t_cycles.max(PASSIVE_REARM_CATCHUP_T_CYCLES);
        let target_tick = start_tick.saturating_add(cycle_budget);
        let mut instructions = 0usize;
        while emulator.cpu_cycles() < target_tick
            && instructions < PASSIVE_REARM_CATCHUP_INSTRUCTIONS
            && !emulator.is_cpu_suspended()
            && !emulator.game_boy_link_reply_to_master_start().passive
        {
            let before = emulator.cpu_cycles();
            let (_, _, _, cycles) = emulator.step_instruction();
            instructions += 1;
            if cycles == 0 || emulator.cpu_cycles() == before {
                break;
            }
        }

        let passive = emulator.game_boy_link_reply_to_master_start().passive;
        self.trace(format!(
            "catchup passive_rearm completed_tick={} start_tick={} end_tick={} elapsed={} instructions={} passive={}",
            completed_tick,
            start_tick,
            emulator.cpu_cycles(),
            emulator.cpu_cycles().saturating_sub(start_tick),
            instructions,
            passive
        ));
        self.passive_rearm_catchup_after_tick = None;
    }

    fn poll_for_pending_master_reply(
        &mut self,
        emulator: &mut GameBoyEmulator,
    ) -> Result<(), LinkSessionError> {
        for _ in 0..MASTER_REPLY_SPIN_LIMIT {
            if self.pending_master_transfer.is_none() {
                return Ok(());
            }
            if let Some(packet) = self.session.try_receive_packet()? {
                match packet.kind {
                    LinkPacketKind::LinkEvent => {
                        let event = decode_game_boy_link_event(&packet.payload)
                            .map_err(|_| LinkSessionError::MalformedPacketPayload)?;
                        self.handle_event(emulator, event)?;
                    }
                    LinkPacketKind::Disconnect => {
                        self.trace("recv disconnect".to_string());
                        self.session.disconnect();
                        return Err(LinkSessionError::Transport(
                            super::LinkTransportError::Disconnected,
                        ));
                    }
                    LinkPacketKind::Hello | LinkPacketKind::LinkState => {}
                }
            } else {
                #[cfg(not(target_arch = "wasm32"))]
                std::thread::yield_now();
            }
        }

        Ok(())
    }

    fn send_event(&mut self, event: GameBoyLinkEvent) -> Result<(), LinkSessionError> {
        self.session.send(
            LinkPacketKind::LinkEvent,
            &encode_game_boy_link_event(event),
        )?;
        Ok(())
    }

    fn allocate_transfer_id(&mut self) -> GbTransferId {
        let endpoint = u64::from(self.session.endpoint().0) << 56;
        let counter = self.next_transfer_id & 0x00FF_FFFF_FFFF_FFFF;
        self.next_transfer_id = self.next_transfer_id.wrapping_add(1);
        GbTransferId(endpoint | counter)
    }

    fn record_replay_event(&mut self, emulator: &GameBoyEmulator, event: ReplayGameBoyLinkEvent) {
        self.recorded_replay_events.push(ReplayEvent::GameBoyLink {
            frame: emulator.frame_count(),
            tick: emulator.cpu_cycles(),
            event,
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn trace(&mut self, message: String) {
        if let Some(trace) = &mut self.trace {
            trace.write(&message);
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn trace(&mut self, _message: String) {}
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

fn encode_game_boy_link_event(event: GameBoyLinkEvent) -> Vec<u8> {
    match event {
        GameBoyLinkEvent::MasterStart {
            transfer_id,
            start_tick,
            action,
        } => {
            let mut out = Vec::with_capacity(MASTER_START_PAYLOAD_LEN);
            out.push(EVENT_MASTER_START);
            out.extend_from_slice(&transfer_id.0.to_le_bytes());
            out.extend_from_slice(&start_tick.to_le_bytes());
            out.extend_from_slice(&action.clock_period_t_cycles.to_le_bytes());
            out.push(action.out_byte);
            out.extend_from_slice(&action.serial_generation.to_le_bytes());
            out
        }
        GameBoyLinkEvent::TransferReply {
            transfer_id,
            sample_tick,
            reply,
        } => {
            let mut out = Vec::with_capacity(TRANSFER_REPLY_PAYLOAD_LEN);
            out.push(EVENT_TRANSFER_REPLY);
            out.extend_from_slice(&transfer_id.0.to_le_bytes());
            out.extend_from_slice(&sample_tick.to_le_bytes());
            out.push(reply.out_byte);
            out.push(u8::from(reply.passive));
            out.extend_from_slice(&reply.serial_generation.to_le_bytes());
            out
        }
    }
}

fn decode_game_boy_link_event(payload: &[u8]) -> Result<GameBoyLinkEvent, GameBoyLinkPayloadError> {
    let Some(kind) = payload.first().copied() else {
        return Err(GameBoyLinkPayloadError::WrongLength {
            expected: 1,
            actual: 0,
        });
    };

    match kind {
        EVENT_MASTER_START => {
            if payload.len() != MASTER_START_PAYLOAD_LEN {
                return Err(GameBoyLinkPayloadError::WrongLength {
                    expected: MASTER_START_PAYLOAD_LEN,
                    actual: payload.len(),
                });
            }
            let transfer_id = GbTransferId(read_u64(payload, 1));
            Ok(GameBoyLinkEvent::MasterStart {
                transfer_id,
                start_tick: read_u64(payload, 9),
                action: GameBoyLinkAction {
                    clock_period_t_cycles: read_u64(payload, 17),
                    out_byte: payload[25],
                    serial_generation: read_u64(payload, 26),
                },
            })
        }
        EVENT_TRANSFER_REPLY => {
            if payload.len() != TRANSFER_REPLY_PAYLOAD_LEN {
                return Err(GameBoyLinkPayloadError::WrongLength {
                    expected: TRANSFER_REPLY_PAYLOAD_LEN,
                    actual: payload.len(),
                });
            }
            Ok(GameBoyLinkEvent::TransferReply {
                transfer_id: GbTransferId(read_u64(payload, 1)),
                sample_tick: read_u64(payload, 9),
                reply: GameBoyLinkReply {
                    out_byte: payload[17],
                    passive: payload[18] != 0,
                    serial_generation: read_u64(payload, 19),
                },
            })
        }
        other => Err(GameBoyLinkPayloadError::UnknownEvent(other)),
    }
}

fn read_u64(payload: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        payload[offset..offset + 8]
            .try_into()
            .expect("payload length checked before u64 read"),
    )
}

fn format_reply(reply: GameBoyLinkReply) -> String {
    format!(
        "out={:02X} passive={} gen={}",
        reply.out_byte, reply.passive, reply.serial_generation
    )
}

fn format_replay_reply(reply: ReplayGameBoyLinkReply) -> String {
    format!(
        "out={:02X} passive={} gen={}",
        reply.out_byte, reply.passive, reply.serial_generation
    )
}

#[cfg(not(target_arch = "wasm32"))]
struct LinkTrace {
    file: std::fs::File,
}

#[cfg(not(target_arch = "wasm32"))]
impl LinkTrace {
    fn from_env(endpoint: super::LinkEndpointId) -> Option<Self> {
        let raw = std::env::var("ZEFF_BOY_LINK_TRACE").ok()?;
        if raw.trim().is_empty() || raw.eq_ignore_ascii_case("0") {
            return None;
        }

        let path = trace_path(&raw, endpoint);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()?;
        Some(Self { file })
    }

    fn write(&mut self, message: &str) {
        let _ = writeln!(self.file, "{message}");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn trace_path(raw: &str, endpoint: super::LinkEndpointId) -> PathBuf {
    let pid = std::process::id();
    if raw == "1" {
        return PathBuf::from(".tmp").join(format!("zeff-boy-link-{pid}-e{}.log", endpoint.0));
    }

    let substituted = raw
        .replace("{pid}", &pid.to_string())
        .replace("{endpoint}", &endpoint.0.to_string());
    let path = PathBuf::from(substituted);
    if path.extension().is_some() {
        path
    } else {
        path.join(format!("zeff-boy-link-{pid}-e{}.log", endpoint.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use zeff_gb_core::hardware::types::constants::{IE_ADDR, INTERRUPT_IF, SERIAL_SB, SERIAL_SC};
    use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

    use crate::link::transport::LocalLinkTransport;
    use crate::link::{LinkEndpointId, LinkSystemType};

    #[test]
    fn game_boy_link_event_payload_roundtrips_master_start() {
        let event = GameBoyLinkEvent::MasterStart {
            transfer_id: GbTransferId(0x0100_0000_0000_0007),
            start_tick: 123,
            action: GameBoyLinkAction {
                out_byte: 0xAB,
                clock_period_t_cycles: 4096,
                serial_generation: 42,
            },
        };

        assert_eq!(
            decode_game_boy_link_event(&encode_game_boy_link_event(event)),
            Ok(event)
        );
    }

    #[test]
    fn game_boy_link_event_payload_roundtrips_transfer_reply() {
        let event = GameBoyLinkEvent::TransferReply {
            transfer_id: GbTransferId(0x0200_0000_0000_0009),
            sample_tick: 456,
            reply: GameBoyLinkReply {
                out_byte: 0x34,
                passive: true,
                serial_generation: 77,
            },
        };

        assert_eq!(
            decode_game_boy_link_event(&encode_game_boy_link_event(event)),
            Ok(event)
        );
    }

    #[test]
    fn game_boy_remote_link_binds_reply_to_exact_transfer_id() {
        let (left_transport, right_transport) = LocalLinkTransport::pair();
        let mut left_link = GameBoyRemoteLink::new(LinkSession::new(
            left_transport,
            LinkSystemType::GameBoy,
            LinkEndpointId(1),
        ));
        let mut right_link = GameBoyRemoteLink::new(LinkSession::new(
            right_transport,
            LinkSystemType::GameBoy,
            LinkEndpointId(2),
        ));
        let mut left = gb_emulator();
        let mut right = gb_emulator();

        left.set_game_boy_link_peer_present(true);
        right.set_game_boy_link_peer_present(true);
        left.write_byte(SERIAL_SB, 0xAB);
        right.write_byte(SERIAL_SB, 0x34);
        left.write_byte(SERIAL_SC, 0x81);
        right.write_byte(SERIAL_SC, 0x80);

        left_link.poll_emulator(&mut left).unwrap();
        right_link.poll_emulator(&mut right).unwrap();
        left_link.poll_emulator(&mut left).unwrap();

        assert_eq!(right.cpu_peek8(SERIAL_SB), 0xAB);
        assert_eq!(right.cpu_peek8(SERIAL_SC) & 0x80, 0);
        assert_eq!(right.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
        assert_eq!(left.cpu_peek8(SERIAL_SB), 0xAB);
        assert_eq!(left.cpu_peek8(SERIAL_SC) & 0x80, 0x80);

        left.step_frame();
        assert_eq!(left.cpu_peek8(SERIAL_SB), 0x34);
        assert_eq!(left.cpu_peek8(SERIAL_SC) & 0x80, 0);
        assert_eq!(left.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
    }

    #[test]
    fn game_boy_remote_link_records_replay_events_for_endpoint() {
        let (left_transport, right_transport) = LocalLinkTransport::pair();
        let mut left_link = GameBoyRemoteLink::new(LinkSession::new(
            left_transport,
            LinkSystemType::GameBoy,
            LinkEndpointId(1),
        ));
        let mut right_link = GameBoyRemoteLink::new(LinkSession::new(
            right_transport,
            LinkSystemType::GameBoy,
            LinkEndpointId(2),
        ));
        let mut left = gb_emulator();
        let mut right = gb_emulator();

        left.set_game_boy_link_peer_present(true);
        right.set_game_boy_link_peer_present(true);
        left.write_byte(SERIAL_SB, 0xAB);
        right.write_byte(SERIAL_SB, 0x34);
        left.write_byte(SERIAL_SC, 0x81);
        right.write_byte(SERIAL_SC, 0x80);

        left_link.poll_emulator(&mut left).unwrap();
        right_link.poll_emulator(&mut right).unwrap();
        left_link.poll_emulator(&mut left).unwrap();

        let left_events = left_link.take_replay_events();
        let right_events = right_link.take_replay_events();

        assert_eq!(
            left_events,
            vec![
                ReplayEvent::GameBoyLink {
                    frame: 0,
                    tick: 0,
                    event: ReplayGameBoyLinkEvent::LocalMasterStart {
                        transfer_id: 0x0100_0000_0000_0000,
                        clock_period_t_cycles: 4096,
                        out_byte: 0xAB,
                        serial_generation: 4,
                    },
                },
                ReplayEvent::GameBoyLink {
                    frame: 0,
                    tick: 0,
                    event: ReplayGameBoyLinkEvent::RemoteReply {
                        transfer_id: 0x0100_0000_0000_0000,
                        out_byte: 0x34,
                        passive: true,
                        serial_generation: 4,
                    },
                },
            ]
        );
        assert_eq!(
            right_events,
            vec![ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 0,
                event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                    transfer_id: 0x0100_0000_0000_0000,
                    clock_period_t_cycles: 4096,
                    out_byte: 0xAB,
                    serial_generation: 4,
                    local_reply: Some(ReplayGameBoyLinkReply {
                        out_byte: 0x34,
                        passive: true,
                        serial_generation: 4,
                    }),
                },
            }]
        );
    }

    #[test]
    fn game_boy_replay_link_applies_recorded_reply_without_tcp_peer() {
        let mut replay_link = GameBoyReplayLink::new(
            vec![ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 0,
                event: ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: 0x0100_0000_0000_0000,
                    out_byte: 0x34,
                    passive: true,
                    serial_generation: 4,
                },
            }],
            0,
            None,
            0,
        );
        let mut gb = gb_emulator();

        replay_link.poll_emulator(&mut gb).unwrap();
        gb.write_byte(SERIAL_SB, 0xAB);
        gb.write_byte(SERIAL_SC, 0x81);
        replay_link.poll_emulator(&mut gb).unwrap();
        gb.step_frame();

        assert_eq!(gb.cpu_peek8(SERIAL_SB), 0x34);
        assert_eq!(gb.cpu_peek8(SERIAL_SC) & 0x80, 0);
        assert_eq!(gb.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
    }

    #[test]
    fn game_boy_replay_link_validates_recorded_local_master_start() {
        let mut replay_link = GameBoyReplayLink::new(
            vec![
                ReplayEvent::GameBoyLink {
                    frame: 0,
                    tick: 0,
                    event: ReplayGameBoyLinkEvent::LocalMasterStart {
                        transfer_id: 0x0100_0000_0000_0000,
                        clock_period_t_cycles: 4096,
                        out_byte: 0xAB,
                        serial_generation: 4,
                    },
                },
                ReplayEvent::GameBoyLink {
                    frame: 0,
                    tick: 0,
                    event: ReplayGameBoyLinkEvent::RemoteReply {
                        transfer_id: 0x0100_0000_0000_0000,
                        out_byte: 0x34,
                        passive: true,
                        serial_generation: 4,
                    },
                },
            ],
            0,
            None,
            0,
        );
        let mut gb = gb_emulator();

        replay_link.poll_emulator(&mut gb).unwrap();
        gb.write_byte(SERIAL_SB, 0xAB);
        gb.write_byte(SERIAL_SC, 0x81);
        replay_link.poll_emulator(&mut gb).unwrap();

        assert!(replay_link.events.iter().all(|record| record.delivered));
        assert_eq!(replay_link.pending_master_transfer, None);
        gb.step_frame();
        assert_eq!(gb.cpu_peek8(SERIAL_SB), 0x34);
    }

    #[test]
    fn game_boy_replay_link_does_not_arm_future_local_master_start() {
        let mut replay_link = GameBoyReplayLink::new(
            vec![
                ReplayEvent::GameBoyLink {
                    frame: 5,
                    tick: 0,
                    event: ReplayGameBoyLinkEvent::LocalMasterStart {
                        transfer_id: 0x0100_0000_0000_0000,
                        clock_period_t_cycles: 4096,
                        out_byte: 0xAB,
                        serial_generation: 4,
                    },
                },
                ReplayEvent::GameBoyLink {
                    frame: 5,
                    tick: 0,
                    event: ReplayGameBoyLinkEvent::RemoteReply {
                        transfer_id: 0x0100_0000_0000_0000,
                        out_byte: 0x34,
                        passive: true,
                        serial_generation: 4,
                    },
                },
            ],
            0,
            None,
            0,
        );
        let mut gb = gb_emulator();

        replay_link.poll_emulator(&mut gb).unwrap();
        gb.write_byte(SERIAL_SB, 0xAB);
        gb.write_byte(SERIAL_SC, 0x81);
        replay_link.poll_emulator(&mut gb).unwrap();

        assert_eq!(replay_link.pending_master_transfer, None);
        assert!(replay_link.events.iter().all(|record| !record.delivered));
    }

    #[test]
    fn game_boy_replay_link_preserves_recorded_peer_presence_before_remote_master() {
        let mut replay_link = GameBoyReplayLink::new(
            vec![ReplayEvent::GameBoyLink {
                frame: 5,
                tick: 0,
                event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                    transfer_id: 0x0100_0000_0000_0000,
                    clock_period_t_cycles: 4096,
                    out_byte: 0xAB,
                    serial_generation: 4,
                    local_reply: None,
                },
            }],
            0,
            None,
            0,
        );
        let mut gb = gb_emulator();
        gb.restore_game_boy_link_replay_state(zeff_emu_common::replay::ReplayGameBoyLinkState {
            peer_present: true,
            pending_master_byte: None,
            pending_master_response: None,
            pending_master_completion_ready: false,
            queued_master_action: None,
            serial_generation: 0,
        });

        replay_link.poll_emulator(&mut gb).unwrap();

        assert!(gb.game_boy_link_replay_state().peer_present);
        assert_eq!(replay_link.pending_master_transfer, None);
        assert!(replay_link.events.iter().all(|record| !record.delivered));
    }

    #[test]
    fn game_boy_replay_link_expands_relative_ticks_from_playback_start() {
        let replay_link = GameBoyReplayLink::new(
            vec![ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 123,
                event: ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: 0x0100_0000_0000_0000,
                    out_byte: 0x34,
                    passive: true,
                    serial_generation: 4,
                },
            }],
            0,
            Some(9_000),
            5_000,
        );

        assert_eq!(replay_link.absolute_event_tick(123), 5_123);
    }

    #[test]
    fn game_boy_replay_link_ignores_local_master_serial_generation_mismatch() {
        let mut replay_link = GameBoyReplayLink::new(
            vec![
                ReplayEvent::GameBoyLink {
                    frame: 0,
                    tick: 0,
                    event: ReplayGameBoyLinkEvent::LocalMasterStart {
                        transfer_id: 0x0100_0000_0000_0000,
                        clock_period_t_cycles: 4096,
                        out_byte: 0xAB,
                        serial_generation: 999,
                    },
                },
                ReplayEvent::GameBoyLink {
                    frame: 0,
                    tick: 0,
                    event: ReplayGameBoyLinkEvent::RemoteReply {
                        transfer_id: 0x0100_0000_0000_0000,
                        out_byte: 0x34,
                        passive: true,
                        serial_generation: 999,
                    },
                },
            ],
            0,
            None,
            0,
        );
        let mut gb = gb_emulator();

        replay_link.poll_emulator(&mut gb).unwrap();
        gb.write_byte(SERIAL_SB, 0xAB);
        gb.write_byte(SERIAL_SC, 0x81);
        replay_link.poll_emulator(&mut gb).unwrap();

        assert!(replay_link.events.iter().all(|record| record.delivered));
        assert_eq!(replay_link.pending_master_transfer, None);
    }

    #[test]
    fn game_boy_replay_link_does_not_consume_due_reply_before_local_master() {
        let mut replay_link = GameBoyReplayLink::new(
            vec![ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 0,
                event: ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: 0x0100_0000_0000_0000,
                    out_byte: 0x34,
                    passive: true,
                    serial_generation: 4,
                },
            }],
            0,
            None,
            0,
        );
        let mut gb = gb_emulator();

        replay_link.poll_emulator(&mut gb).unwrap();

        assert!(!replay_link.events[0].delivered);
        assert_eq!(replay_link.pending_master_transfer, None);
    }

    #[test]
    fn game_boy_replay_link_binds_matching_reply_before_recorded_frame_boundary() {
        let mut replay_link = GameBoyReplayLink::new(
            vec![ReplayEvent::GameBoyLink {
                frame: 5,
                tick: u64::MAX,
                event: ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: 0x0100_0000_0000_0000,
                    out_byte: 0x34,
                    passive: true,
                    serial_generation: 4,
                },
            }],
            0,
            None,
            0,
        );
        let mut gb = gb_emulator();

        replay_link.poll_emulator(&mut gb).unwrap();
        gb.write_byte(SERIAL_SB, 0xAB);
        gb.write_byte(SERIAL_SC, 0x81);
        replay_link.poll_emulator(&mut gb).unwrap();

        assert_eq!(replay_link.pending_master_transfer, None);
        assert!(replay_link.events[0].delivered);
        gb.step_frame();
        assert_eq!(gb.cpu_peek8(SERIAL_SB), 0x34);
    }

    #[test]
    fn game_boy_replay_link_does_not_synthesize_local_transfer_from_restored_sc() {
        let mut replay_link = GameBoyReplayLink::new(Vec::new(), 0, None, 0);
        let mut gb = gb_emulator();

        gb.write_byte(SERIAL_SB, 0xAB);
        gb.write_byte(SERIAL_SC, 0x81);
        let state = gb.encode_state_bytes().unwrap();
        gb.load_state_from_bytes(state).unwrap();

        replay_link.poll_emulator(&mut gb).unwrap();

        assert_eq!(gb.cpu_peek8(SERIAL_SC) & 0x80, 0x80);
        assert_eq!(replay_link.pending_master_transfer, None);
    }

    #[test]
    fn game_boy_replay_link_handles_simultaneous_remote_master_before_reply() {
        let mut replay_link = GameBoyReplayLink::new(
            vec![
                ReplayEvent::GameBoyLink {
                    frame: 0,
                    tick: 0,
                    event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                        transfer_id: 0x0200_0000_0000_0000,
                        clock_period_t_cycles: 4096,
                        out_byte: 0x56,
                        serial_generation: 4,
                        local_reply: None,
                    },
                },
                ReplayEvent::GameBoyLink {
                    frame: 0,
                    tick: 0,
                    event: ReplayGameBoyLinkEvent::RemoteReply {
                        transfer_id: 0x0100_0000_0000_0000,
                        out_byte: 0x34,
                        passive: false,
                        serial_generation: 4,
                    },
                },
            ],
            0,
            None,
            0,
        );
        let mut gb = gb_emulator();

        replay_link.poll_emulator(&mut gb).unwrap();
        gb.write_byte(SERIAL_SB, 0xAB);
        gb.write_byte(SERIAL_SC, 0x81);
        replay_link.poll_emulator(&mut gb).unwrap();
        gb.step_frame();

        assert_eq!(gb.cpu_peek8(SERIAL_SB), 0x34);
        assert_eq!(gb.cpu_peek8(SERIAL_SC) & 0x80, 0);
        assert_eq!(gb.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
    }

    #[test]
    fn game_boy_replay_link_rejects_remote_master_local_reply_mismatch() {
        let mut replay_link = GameBoyReplayLink::new(
            vec![ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 0,
                event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                    transfer_id: 0x0100_0000_0000_0000,
                    clock_period_t_cycles: 4096,
                    out_byte: 0x56,
                    serial_generation: 4,
                    local_reply: Some(ReplayGameBoyLinkReply {
                        out_byte: 0x34,
                        passive: true,
                        serial_generation: 4,
                    }),
                },
            }],
            0,
            None,
            0,
        );
        let mut gb = gb_emulator();

        assert_eq!(
            replay_link.poll_emulator(&mut gb),
            Err(LinkSessionError::MalformedPacketPayload)
        );
    }

    #[test]
    fn game_boy_remote_link_does_not_spin_wait_for_early_pending_reply() {
        let (left_transport, _right_transport) = LocalLinkTransport::pair();
        let mut left_link = GameBoyRemoteLink::new(LinkSession::new(
            left_transport,
            LinkSystemType::GameBoy,
            LinkEndpointId(1),
        ));
        let mut left = gb_emulator();

        left.set_game_boy_link_peer_present(true);
        left.write_byte(SERIAL_SB, 0xAB);
        left.write_byte(SERIAL_SC, 0x81);

        left_link.poll_emulator(&mut left).unwrap();

        assert_eq!(
            left_link.pending_master_transfer_id(),
            Some(0x0100_0000_0000_0000)
        );
        assert!(!left.game_boy_link_waiting_at_completion_boundary());
        assert_eq!(left.cpu_peek8(SERIAL_SC) & 0x80, 0x80);
        assert_eq!(left.cpu_peek8(INTERRUPT_IF) & 0x08, 0);
    }

    #[test]
    fn game_boy_remote_link_catches_up_passive_rearm_before_queued_master_start() {
        let (left_transport, right_transport) = LocalLinkTransport::pair();
        let mut left_session =
            LinkSession::new(left_transport, LinkSystemType::GameBoy, LinkEndpointId(1));
        let mut right_link = GameBoyRemoteLink::new(LinkSession::new(
            right_transport,
            LinkSystemType::GameBoy,
            LinkEndpointId(2),
        ));
        let mut right = gb_emulator_with_serial_rearm_isr(0x56);

        right.set_game_boy_link_peer_present(true);
        right.write_byte(SERIAL_SB, 0x34);
        right.write_byte(SERIAL_SC, 0x80);
        queue_master_start(&mut left_session, 0x0100_0000_0000_0000, 0xAB);
        queue_master_start(&mut left_session, 0x0100_0000_0000_0001, 0xCD);

        right_link.poll_emulator(&mut right).unwrap();

        let first = receive_reply(&mut left_session);
        let GameBoyLinkEvent::TransferReply {
            transfer_id, reply, ..
        } = first
        else {
            panic!("expected first transfer reply, got {first:?}");
        };
        assert_eq!(transfer_id, GbTransferId(0x0100_0000_0000_0000));
        assert_eq!(reply.out_byte, 0x34);
        assert!(reply.passive);

        let second = receive_reply(&mut left_session);
        let GameBoyLinkEvent::TransferReply {
            transfer_id, reply, ..
        } = second
        else {
            panic!("expected second transfer reply, got {second:?}");
        };
        assert_eq!(transfer_id, GbTransferId(0x0100_0000_0000_0001));
        assert_eq!(reply.out_byte, 0x56);
        assert!(reply.passive);
        assert_eq!(right.cpu_peek8(SERIAL_SB), 0xCD);
        assert_eq!(right.cpu_peek8(SERIAL_SC) & 0x80, 0);
        assert_eq!(right.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
    }

    #[test]
    fn game_boy_remote_link_rejects_unmatched_reply() {
        let (left_transport, right_transport) = LocalLinkTransport::pair();
        let mut left_link = GameBoyRemoteLink::new(LinkSession::new(
            left_transport,
            LinkSystemType::GameBoy,
            LinkEndpointId(1),
        ));
        let mut right_session =
            LinkSession::new(right_transport, LinkSystemType::GameBoy, LinkEndpointId(2));
        let mut left = gb_emulator();

        right_session
            .send(
                LinkPacketKind::LinkEvent,
                &encode_game_boy_link_event(GameBoyLinkEvent::TransferReply {
                    transfer_id: GbTransferId(0x0200_0000_0000_0001),
                    sample_tick: 0,
                    reply: GameBoyLinkReply {
                        out_byte: 0x34,
                        passive: true,
                        serial_generation: 0,
                    },
                }),
            )
            .unwrap();

        assert_eq!(
            left_link.poll_emulator(&mut left),
            Err(LinkSessionError::MalformedPacketPayload)
        );
    }

    fn gb_emulator() -> GameBoyEmulator {
        let rom = vec![0u8; 0x8000];
        GameBoyEmulator::from_rom_data(&rom, HardwareModePreference::Auto)
            .expect("GB emulator should initialize")
    }

    fn gb_emulator_with_serial_rearm_isr(next_reply: u8) -> GameBoyEmulator {
        let mut rom = vec![0u8; 0x8000];
        rom[0x0058..0x0061]
            .copy_from_slice(&[0x3E, next_reply, 0xE0, 0x01, 0x3E, 0x80, 0xE0, 0x02, 0xD9]);
        rom[0x0100..0x0105].copy_from_slice(&[0xFB, 0x00, 0x76, 0x18, 0xFD]);
        let mut emulator = GameBoyEmulator::from_rom_data(&rom, HardwareModePreference::Auto)
            .expect("GB emulator should initialize");
        emulator.write_byte(IE_ADDR, 0x08);
        emulator.step_instruction();
        emulator.step_instruction();
        emulator.step_instruction();
        emulator
    }

    fn queue_master_start(
        session: &mut LinkSession<LocalLinkTransport>,
        transfer_id: u64,
        out_byte: u8,
    ) {
        session
            .send(
                LinkPacketKind::LinkEvent,
                &encode_game_boy_link_event(GameBoyLinkEvent::MasterStart {
                    transfer_id: GbTransferId(transfer_id),
                    start_tick: 0,
                    action: GameBoyLinkAction {
                        out_byte,
                        clock_period_t_cycles: 4096,
                        serial_generation: 4,
                    },
                }),
            )
            .unwrap();
    }

    fn receive_reply(session: &mut LinkSession<LocalLinkTransport>) -> GameBoyLinkEvent {
        let packet = session
            .try_receive_packet()
            .unwrap()
            .expect("reply packet should be queued");
        assert_eq!(packet.kind, LinkPacketKind::LinkEvent);
        decode_game_boy_link_event(&packet.payload).expect("reply payload should decode")
    }

    #[allow(dead_code)]
    fn gb_backend() -> EmuBackend {
        EmuBackend::from_gb(gb_emulator(), PathBuf::from("test.gb"))
    }
}
