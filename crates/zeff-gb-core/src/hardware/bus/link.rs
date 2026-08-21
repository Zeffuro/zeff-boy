use std::fmt;

use super::Bus;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GameBoyLinkState {
    pub pending_master_byte: Option<u8>,
    pub external_clock_byte: Option<u8>,
    pub output_byte: u8,
}

impl GameBoyLinkState {
    pub fn is_idle(self) -> bool {
        self.pending_master_byte.is_none() && self.external_clock_byte.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameBoyLinkAction {
    pub out_byte: u8,
    pub clock_period_t_cycles: u64,
    pub serial_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameBoyLinkReply {
    pub out_byte: u8,
    pub passive: bool,
    pub serial_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameBoyLinkReplyDisposition {
    AcceptedPending,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameBoyLinkTransferExchange {
    pub reply: GameBoyLinkReplyDisposition,
    pub passive_responder_scheduled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameBoyLinkExchangePreview {
    pub local_action: Option<GameBoyLinkAction>,
    pub peer_action: Option<GameBoyLinkAction>,
    pub local_reply: GameBoyLinkReply,
    pub peer_reply: GameBoyLinkReply,
}

#[derive(Debug, PartialEq, Eq)]
pub struct GameBoyLinkPreparedTransfer {
    action: GameBoyLinkAction,
    reply: GameBoyLinkReply,
    passive_responder_scheduled: bool,
}

impl GameBoyLinkPreparedTransfer {
    pub fn action(&self) -> GameBoyLinkAction {
        self.action
    }

    pub fn reply(&self) -> GameBoyLinkReply {
        self.reply
    }

    pub fn passive_responder_scheduled(&self) -> bool {
        self.passive_responder_scheduled
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct GameBoyLinkPreparedExchange {
    pub local_action: Option<GameBoyLinkPreparedTransfer>,
    pub peer_action: Option<GameBoyLinkPreparedTransfer>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameBoyLinkExchangeOutcome {
    Idle,
    Exchanged {
        local_action: Option<GameBoyLinkTransferExchange>,
        peer_action: Option<GameBoyLinkTransferExchange>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameBoyLinkExchangeSide {
    Local,
    Peer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameBoyLinkExchangeError {
    RejectedReply {
        side: GameBoyLinkExchangeSide,
        action_generation: u64,
        serial_generation: u64,
    },
    RejectedPassiveScheduling {
        responder: GameBoyLinkExchangeSide,
    },
}

impl fmt::Display for GameBoyLinkExchangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RejectedReply {
                side,
                action_generation,
                serial_generation,
            } => write!(
                f,
                "{side:?} rejected link reply for action generation {action_generation} at serial generation {serial_generation}"
            ),
            Self::RejectedPassiveScheduling { responder } => {
                write!(f, "{responder:?} rejected passive link scheduling")
            }
        }
    }
}

impl std::error::Error for GameBoyLinkExchangeError {}

impl Bus {
    pub fn set_game_boy_link_peer_present(&mut self, present: bool) {
        self.io.serial.set_link_peer_present(present);
    }

    pub fn restore_game_boy_link_peer_present_without_action(&mut self, present: bool) {
        self.io
            .serial
            .restore_link_peer_present_without_action(present);
    }

    pub fn game_boy_link_state(&self) -> GameBoyLinkState {
        self.io.serial.link_state()
    }

    pub fn game_boy_link_pending_master_response(&self) -> bool {
        self.io.serial.pending_link_byte().is_some()
            && self.io.serial.pending_link_response().is_none()
    }

    pub fn game_boy_link_waiting_at_completion_boundary(&self) -> bool {
        self.io.serial.waiting_at_link_completion_boundary()
    }

    pub fn take_game_boy_link_action(&mut self) -> Option<GameBoyLinkAction> {
        self.io.serial.take_link_action()
    }

    pub fn game_boy_link_replay_state(&self) -> zeff_emu_common::replay::ReplayGameBoyLinkState {
        self.io.serial.replay_link_state()
    }

    pub fn restore_game_boy_link_replay_state(
        &mut self,
        state: zeff_emu_common::replay::ReplayGameBoyLinkState,
    ) -> bool {
        self.io.serial.restore_replay_link_state(state)
    }

    pub fn game_boy_link_reply_to_master_start(&self) -> GameBoyLinkReply {
        self.io.serial.reply_to_master_start()
    }

    pub fn apply_game_boy_link_reply(&mut self, reply: GameBoyLinkReply) -> bool {
        let completed = self.io.serial.apply_link_reply(reply);
        if completed && self.io.serial.pending_link_byte().is_none() {
            self.if_reg |= 0x08;
        }
        completed
    }

    pub fn complete_game_boy_external_link_transfer(&mut self, peer_byte: u8) -> bool {
        let completed = self.io.serial.complete_external_from_master(peer_byte);
        if completed {
            self.if_reg |= 0x08;
        }
        completed
    }

    pub fn schedule_game_boy_external_link_transfer(&mut self, peer_byte: u8, period: u64) -> bool {
        self.io
            .serial
            .schedule_external_from_master(peer_byte, period)
    }

    pub fn sync_game_boy_remote_link_peer(&mut self, peer_state: GameBoyLinkState) -> bool {
        self.sync_game_boy_remote_link_peer_with_idle_response(peer_state, None)
    }

    pub fn sync_game_boy_remote_link_peer_with_idle_response(
        &mut self,
        peer_state: GameBoyLinkState,
        idle_master_response: Option<u8>,
    ) -> bool {
        self.io.serial.set_link_peer_present(true);
        let completed = self
            .io
            .serial
            .apply_remote_link_peer_state(peer_state, idle_master_response);
        if completed {
            self.if_reg |= 0x08;
        }
        completed
    }

    pub fn sync_game_boy_link_peer(&mut self, peer: &mut Self) {
        let _ = self.try_sync_game_boy_link_peer(peer);
    }

    pub fn preview_game_boy_link_peer(&self, peer: &Self) -> GameBoyLinkExchangePreview {
        GameBoyLinkExchangePreview {
            local_action: self.io.serial.link_action_after_peer_present(),
            peer_action: peer.io.serial.link_action_after_peer_present(),
            local_reply: self.io.serial.reply_to_master_start(),
            peer_reply: peer.io.serial.reply_to_master_start(),
        }
    }

    pub fn try_sync_game_boy_link_peer(
        &mut self,
        peer: &mut Self,
    ) -> Result<GameBoyLinkExchangeOutcome, GameBoyLinkExchangeError> {
        let prepared = self.try_prepare_game_boy_link_peer(peer)?;
        if prepared.local_action.is_none() && prepared.peer_action.is_none() {
            return Ok(GameBoyLinkExchangeOutcome::Idle);
        }
        debug_assert!(
            Self::validate_prepared_reply(
                self,
                prepared.local_action.as_ref(),
                GameBoyLinkExchangeSide::Local,
            )
            .is_ok()
        );
        debug_assert!(
            Self::validate_prepared_reply(
                peer,
                prepared.peer_action.as_ref(),
                GameBoyLinkExchangeSide::Peer,
            )
            .is_ok()
        );
        let local_exchange = prepared
            .local_action
            .map(|transfer| Self::commit_prepared_reply(self, transfer));
        let peer_exchange = prepared
            .peer_action
            .map(|transfer| Self::commit_prepared_reply(peer, transfer));

        Ok(GameBoyLinkExchangeOutcome::Exchanged {
            local_action: local_exchange,
            peer_action: peer_exchange,
        })
    }

    pub fn try_prepare_game_boy_link_peer(
        &mut self,
        peer: &mut Self,
    ) -> Result<GameBoyLinkPreparedExchange, GameBoyLinkExchangeError> {
        let preview = self.preview_game_boy_link_peer(peer);
        if preview.local_action.is_none() && preview.peer_action.is_none() {
            self.io.serial.set_link_peer_present(true);
            peer.io.serial.set_link_peer_present(true);
            return Ok(GameBoyLinkPreparedExchange::default());
        }

        Self::validate_link_action(self, preview.local_action, GameBoyLinkExchangeSide::Local)?;
        Self::validate_link_action(peer, preview.peer_action, GameBoyLinkExchangeSide::Peer)?;
        Self::validate_passive_schedule(
            peer,
            preview.local_action,
            preview.peer_reply,
            GameBoyLinkExchangeSide::Peer,
        )?;
        Self::validate_passive_schedule(
            self,
            preview.peer_action,
            preview.local_reply,
            GameBoyLinkExchangeSide::Local,
        )?;

        self.io.serial.set_link_peer_present(true);
        peer.io.serial.set_link_peer_present(true);
        debug_assert_eq!(self.io.serial.link_action(), preview.local_action);
        debug_assert_eq!(peer.io.serial.link_action(), preview.peer_action);
        let committed_local_action = self.io.serial.take_link_action();
        let committed_peer_action = peer.io.serial.take_link_action();
        debug_assert_eq!(committed_local_action, preview.local_action);
        debug_assert_eq!(committed_peer_action, preview.peer_action);

        let local_action = preview
            .local_action
            .map(|action| Self::prepare_link_transfer(peer, action, preview.peer_reply));
        let peer_action = preview
            .peer_action
            .map(|action| Self::prepare_link_transfer(self, action, preview.local_reply));

        Ok(GameBoyLinkPreparedExchange {
            local_action,
            peer_action,
        })
    }

    pub fn try_apply_prepared_game_boy_link_reply(
        &mut self,
        transfer: GameBoyLinkPreparedTransfer,
    ) -> Result<GameBoyLinkTransferExchange, GameBoyLinkExchangeError> {
        Self::validate_prepared_reply(self, Some(&transfer), GameBoyLinkExchangeSide::Local)?;
        Ok(Self::commit_prepared_reply(self, transfer))
    }

    fn validate_link_action(
        bus: &Self,
        action: Option<GameBoyLinkAction>,
        side: GameBoyLinkExchangeSide,
    ) -> Result<(), GameBoyLinkExchangeError> {
        if let Some(action) = action
            && !bus.io.serial.link_action_is_current(action)
        {
            return Err(GameBoyLinkExchangeError::RejectedReply {
                side,
                action_generation: action.serial_generation,
                serial_generation: bus.io.serial.replay_link_state().serial_generation,
            });
        }
        Ok(())
    }

    fn validate_passive_schedule(
        responder: &Self,
        action: Option<GameBoyLinkAction>,
        reply: GameBoyLinkReply,
        responder_side: GameBoyLinkExchangeSide,
    ) -> Result<(), GameBoyLinkExchangeError> {
        if action.is_some()
            && reply.passive
            && !responder.io.serial.can_schedule_external_from_master()
        {
            return Err(GameBoyLinkExchangeError::RejectedPassiveScheduling {
                responder: responder_side,
            });
        }
        Ok(())
    }

    fn prepare_link_transfer(
        responder: &mut Self,
        action: GameBoyLinkAction,
        reply: GameBoyLinkReply,
    ) -> GameBoyLinkPreparedTransfer {
        let passive_responder_scheduled = reply.passive;
        if passive_responder_scheduled {
            let scheduled = responder
                .io
                .serial
                .schedule_external_from_master(action.out_byte, action.clock_period_t_cycles);
            debug_assert!(scheduled);
        }

        GameBoyLinkPreparedTransfer {
            action,
            reply,
            passive_responder_scheduled,
        }
    }

    fn validate_prepared_reply(
        master: &Self,
        transfer: Option<&GameBoyLinkPreparedTransfer>,
        side: GameBoyLinkExchangeSide,
    ) -> Result<(), GameBoyLinkExchangeError> {
        if let Some(transfer) = transfer
            && !master
                .io
                .serial
                .pending_link_action_is_current(transfer.action)
        {
            return Err(GameBoyLinkExchangeError::RejectedReply {
                side,
                action_generation: transfer.action.serial_generation,
                serial_generation: master.io.serial.replay_link_state().serial_generation,
            });
        }
        Ok(())
    }

    fn commit_prepared_reply(
        master: &mut Self,
        transfer: GameBoyLinkPreparedTransfer,
    ) -> GameBoyLinkTransferExchange {
        let accepted = master.io.serial.apply_link_reply(transfer.reply);
        debug_assert!(accepted);

        let reply = if master.io.serial.pending_link_byte().is_none() {
            master.if_reg |= 0x08;
            GameBoyLinkReplyDisposition::Completed
        } else {
            GameBoyLinkReplyDisposition::AcceptedPending
        };

        GameBoyLinkTransferExchange {
            reply,
            passive_responder_scheduled: transfer.passive_responder_scheduled,
        }
    }
}
