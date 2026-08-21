use std::collections::VecDeque;
use zeff_emu_common::replay::{
    ReplayEvent, ReplayGameBoyLinkAction, ReplayGameBoyLinkCoordinatorOwner,
    ReplayGameBoyLinkCoordinatorState, ReplayGameBoyLinkEvent, ReplayGameBoyLinkReply,
    ReplayGameBoyLinkState,
};
use zeff_gb_core::emulator::Emulator as GameBoyEmulator;

use crate::emu_backend::EmuBackend;
use crate::link::{
    LinkConnectionState, LinkPacketKind, LinkSession, LinkSessionError, LinkTransport,
    LinkTransportError,
};

use super::diagnostics::format_reply;
use super::protocol::{
    GameBoyLinkEvent, GbTransferId, decode_game_boy_link_event, encode_game_boy_link_event,
};
#[cfg(not(target_arch = "wasm32"))]
use super::trace::LinkTrace;

const MASTER_REPLY_SPIN_LIMIT: usize = 64;
const PASSIVE_REARM_GRACE_T_CYCLES: u64 = 4096;

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
struct PassiveDrainGate {
    completion_tick: u64,
    rearm_deadline_tick: u64,
}

pub(crate) struct GameBoyRemoteLink<T: LinkTransport> {
    session: LinkSession<T>,
    next_transfer_id: u64,
    pending_master_transfer: Option<PendingGameBoyMasterTransfer>,
    applied_master_continuation: Option<ReplayGameBoyLinkCoordinatorState>,
    passive_drain_gate: Option<PassiveDrainGate>,
    replay_inbound_schedule: Option<VecDeque<(u64, u64)>>,
    replay_state_schedule: Option<VecDeque<(u64, u64, ReplayGameBoyLinkState)>>,
    recorded_replay_events: Vec<ReplayEvent>,
    #[cfg(not(target_arch = "wasm32"))]
    trace: Option<LinkTrace>,
}

impl<T: LinkTransport> GameBoyRemoteLink<T> {
    pub(crate) fn new(session: LinkSession<T>) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let trace = LinkTrace::from_env(session.endpoint());
        Self {
            session,
            next_transfer_id: 0,
            pending_master_transfer: None,
            applied_master_continuation: None,
            passive_drain_gate: None,
            replay_inbound_schedule: None,
            replay_state_schedule: None,
            recorded_replay_events: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            trace,
        }
    }

    pub(crate) fn with_replay_inbound_schedule(
        session: LinkSession<T>,
        schedule: Vec<(u64, u64)>,
    ) -> Self {
        let mut link = Self::new(session);
        link.replay_inbound_schedule = Some(schedule.into());
        link
    }

    pub(crate) fn with_replay_schedules(
        session: LinkSession<T>,
        inbound_schedule: Vec<(u64, u64)>,
        state_schedule: Vec<(u64, u64, ReplayGameBoyLinkState)>,
    ) -> Self {
        let mut link = Self::with_replay_inbound_schedule(session, inbound_schedule);
        link.replay_state_schedule = Some(state_schedule.into());
        link
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
                LinkTransportError::Disconnected,
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
                LinkTransportError::Disconnected,
            ));
        }

        if !self.apply_due_replay_states(emulator) {
            emulator.restore_game_boy_link_peer_present_without_action(false);
            return Ok(());
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
        self.trace("send disconnect".to_string());
        let _ = self.session.send(LinkPacketKind::Disconnect, &[]);
        self.session.disconnect();
    }

    pub(crate) fn pending_master_transfer_id(&self) -> Option<u64> {
        self.pending_master_transfer.map(|pending| pending.id.0)
    }

    pub(crate) fn replay_coordinator_state(
        &mut self,
        link_state: ReplayGameBoyLinkState,
    ) -> Result<Option<ReplayGameBoyLinkCoordinatorState>, String> {
        if self.applied_master_continuation.is_some() && !link_state.has_master_owned_transfer() {
            self.applied_master_continuation = None;
        }

        let state = if let Some(pending) = self.pending_master_transfer {
            Some(ReplayGameBoyLinkCoordinatorState {
                transfer_id: pending.id.0,
                action: ReplayGameBoyLinkAction {
                    out_byte: pending.out_byte,
                    clock_period_t_cycles: pending.clock_period_t_cycles,
                    serial_generation: pending.serial_generation,
                },
                owner: ReplayGameBoyLinkCoordinatorOwner::ReplayAwaitingReply,
                reply: None,
            })
        } else {
            self.applied_master_continuation
        };

        if let Some(state) = state {
            state.validate_against(link_state).map_err(|error| {
                format!(
                    "GB live link transfer {} cannot be captured: {error}",
                    state.transfer_id
                )
            })?;
            return Ok(Some(state));
        }
        if link_state.pending_master_byte.is_some() && link_state.queued_master_action.is_none() {
            return Err(
                "GB live link local master transfer has no retained coordinator ownership"
                    .to_string(),
            );
        }
        Ok(None)
    }

    pub(crate) fn take_replay_events(&mut self) -> Vec<ReplayEvent> {
        std::mem::take(&mut self.recorded_replay_events)
    }

    pub(crate) fn discard_replay_events_before_capture(&mut self) {
        self.recorded_replay_events.clear();
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
            if !self.passive_completion_allows_next_packet(emulator) {
                return Ok(());
            }
            if !self.replay_inbound_event_is_due(emulator) {
                return Ok(());
            }
            let Some(packet) = self.session.try_receive_packet()? else {
                return Ok(());
            };

            match packet.kind {
                LinkPacketKind::LinkEvent => {
                    let event = decode_game_boy_link_event(&packet.payload)
                        .map_err(|_| LinkSessionError::MalformedPacketPayload)?;
                    self.handle_event(emulator, event)?;
                    self.consume_replay_inbound_event();
                }
                LinkPacketKind::Disconnect => {
                    self.trace("recv disconnect".to_string());
                    self.session.disconnect();
                    return Err(LinkSessionError::Transport(
                        LinkTransportError::Disconnected,
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
                if reply.passive {
                    if action.clock_period_t_cycles == 0 {
                        return Err(LinkSessionError::MalformedPacketPayload);
                    }
                    let completion_tick = emulator
                        .cpu_cycles()
                        .checked_add(action.clock_period_t_cycles)
                        .ok_or(LinkSessionError::MalformedPacketPayload)?;
                    let rearm_deadline_tick = completion_tick
                        .checked_add(
                            action
                                .clock_period_t_cycles
                                .max(PASSIVE_REARM_GRACE_T_CYCLES),
                        )
                        .ok_or(LinkSessionError::MalformedPacketPayload)?;
                    if !emulator.schedule_game_boy_external_link_transfer(
                        action.out_byte,
                        action.clock_period_t_cycles,
                    ) {
                        return Err(LinkSessionError::MalformedPacketPayload);
                    }
                    self.passive_drain_gate = Some(PassiveDrainGate {
                        completion_tick,
                        rearm_deadline_tick,
                    });
                    self.trace(format!(
                        "schedule passive id={} tick={} period={} in={:02X}",
                        transfer_id.0,
                        emulator.cpu_cycles(),
                        action.clock_period_t_cycles,
                        action.out_byte
                    ));
                }
                self.send_event(GameBoyLinkEvent::TransferReply {
                    transfer_id,
                    sample_tick: emulator.cpu_cycles(),
                    reply,
                })?;
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
                    let pending = self
                        .pending_master_transfer
                        .expect("validated reply must have a pending transfer");
                    self.record_replay_event(
                        emulator,
                        ReplayGameBoyLinkEvent::RemoteReply {
                            transfer_id: transfer_id.0,
                            out_byte: reply.out_byte,
                            passive: reply.passive,
                            serial_generation: reply.serial_generation,
                        },
                    );
                    self.applied_master_continuation = Some(ReplayGameBoyLinkCoordinatorState {
                        transfer_id: pending.id.0,
                        action: ReplayGameBoyLinkAction {
                            out_byte: pending.out_byte,
                            clock_period_t_cycles: pending.clock_period_t_cycles,
                            serial_generation: pending.serial_generation,
                        },
                        owner: ReplayGameBoyLinkCoordinatorOwner::CoreHasReply,
                        reply: Some(ReplayGameBoyLinkReply {
                            out_byte: reply.out_byte,
                            passive: reply.passive,
                            serial_generation: reply.serial_generation,
                        }),
                    });
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
        self.applied_master_continuation = None;
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

    fn poll_for_pending_master_reply(
        &mut self,
        emulator: &mut GameBoyEmulator,
    ) -> Result<(), LinkSessionError> {
        for _ in 0..MASTER_REPLY_SPIN_LIMIT {
            if self.pending_master_transfer.is_none() {
                return Ok(());
            }
            if !self.passive_completion_allows_next_packet(emulator) {
                return Ok(());
            }
            if !self.replay_inbound_event_is_due(emulator) {
                return Ok(());
            }
            if let Some(packet) = self.session.try_receive_packet()? {
                match packet.kind {
                    LinkPacketKind::LinkEvent => {
                        let event = decode_game_boy_link_event(&packet.payload)
                            .map_err(|_| LinkSessionError::MalformedPacketPayload)?;
                        self.handle_event(emulator, event)?;
                        self.consume_replay_inbound_event();
                    }
                    LinkPacketKind::Disconnect => {
                        self.trace("recv disconnect".to_string());
                        self.session.disconnect();
                        return Err(LinkSessionError::Transport(
                            LinkTransportError::Disconnected,
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

    fn passive_completion_allows_next_packet(&mut self, emulator: &GameBoyEmulator) -> bool {
        let Some(gate) = self.passive_drain_gate else {
            return true;
        };
        let tick = emulator.cpu_cycles();
        if tick < gate.completion_tick
            || (tick < gate.rearm_deadline_tick
                && !emulator.game_boy_link_reply_to_master_start().passive)
        {
            return false;
        }
        self.passive_drain_gate = None;
        true
    }

    fn send_event(&mut self, event: GameBoyLinkEvent) -> Result<(), LinkSessionError> {
        self.session.send(
            LinkPacketKind::LinkEvent,
            &encode_game_boy_link_event(event),
        )?;
        Ok(())
    }

    fn replay_inbound_event_is_due(&self, emulator: &GameBoyEmulator) -> bool {
        let Some(schedule) = &self.replay_inbound_schedule else {
            return true;
        };
        let Some(&(frame, tick)) = schedule.front() else {
            return false;
        };
        emulator.frame_count() > frame
            || (emulator.frame_count() == frame && emulator.cpu_cycles() >= tick)
    }

    fn apply_due_replay_states(&mut self, emulator: &mut GameBoyEmulator) -> bool {
        let Some(schedule) = &mut self.replay_state_schedule else {
            return true;
        };
        let Some(&(frame, tick, _)) = schedule.front() else {
            return true;
        };
        if emulator.frame_count() < frame
            || (emulator.frame_count() == frame && emulator.cpu_cycles() < tick)
        {
            return false;
        }
        while let Some(&(frame, tick, state)) = schedule.front() {
            if emulator.frame_count() < frame
                || (emulator.frame_count() == frame && emulator.cpu_cycles() < tick)
            {
                break;
            }
            emulator.restore_game_boy_link_replay_state(state);
            schedule.pop_front();
        }
        true
    }

    fn consume_replay_inbound_event(&mut self) {
        if let Some(schedule) = &mut self.replay_inbound_schedule {
            schedule.pop_front();
        }
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
