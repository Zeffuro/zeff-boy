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
    pub passive_responder_completed: bool,
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
    RejectedPassiveCompletion {
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
            Self::RejectedPassiveCompletion { responder } => {
                write!(f, "{responder:?} rejected passive link completion")
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
    ) {
        self.io.serial.restore_replay_link_state(state);
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

    pub fn try_sync_game_boy_link_peer(
        &mut self,
        peer: &mut Self,
    ) -> Result<GameBoyLinkExchangeOutcome, GameBoyLinkExchangeError> {
        self.io.serial.set_link_peer_present(true);
        peer.io.serial.set_link_peer_present(true);

        let local_action = self.io.serial.link_action();
        let peer_action = peer.io.serial.link_action();
        if local_action.is_none() && peer_action.is_none() {
            return Ok(GameBoyLinkExchangeOutcome::Idle);
        }

        Self::validate_link_action(self, local_action, GameBoyLinkExchangeSide::Local)?;
        Self::validate_link_action(peer, peer_action, GameBoyLinkExchangeSide::Peer)?;

        let local_reply = self.io.serial.reply_to_master_start();
        let peer_reply = peer.io.serial.reply_to_master_start();
        let _ = self.io.serial.take_link_action();
        let _ = peer.io.serial.take_link_action();

        let local_exchange = local_action
            .map(|action| {
                Self::apply_link_exchange(
                    self,
                    peer,
                    action,
                    peer_reply,
                    GameBoyLinkExchangeSide::Local,
                    GameBoyLinkExchangeSide::Peer,
                )
            })
            .transpose()?;
        let peer_exchange = peer_action
            .map(|action| {
                Self::apply_link_exchange(
                    peer,
                    self,
                    action,
                    local_reply,
                    GameBoyLinkExchangeSide::Peer,
                    GameBoyLinkExchangeSide::Local,
                )
            })
            .transpose()?;

        Ok(GameBoyLinkExchangeOutcome::Exchanged {
            local_action: local_exchange,
            peer_action: peer_exchange,
        })
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

    fn apply_link_exchange(
        master: &mut Self,
        responder: &mut Self,
        action: GameBoyLinkAction,
        reply: GameBoyLinkReply,
        master_side: GameBoyLinkExchangeSide,
        responder_side: GameBoyLinkExchangeSide,
    ) -> Result<GameBoyLinkTransferExchange, GameBoyLinkExchangeError> {
        if !master.io.serial.apply_link_reply(reply) {
            return Err(GameBoyLinkExchangeError::RejectedReply {
                side: master_side,
                action_generation: action.serial_generation,
                serial_generation: master.io.serial.replay_link_state().serial_generation,
            });
        }

        let reply_disposition = if master.io.serial.pending_link_byte().is_none() {
            master.if_reg |= 0x08;
            GameBoyLinkReplyDisposition::Completed
        } else {
            GameBoyLinkReplyDisposition::AcceptedPending
        };
        let passive_responder_completed = if reply.passive {
            if !responder
                .io
                .serial
                .complete_external_from_master(action.out_byte)
            {
                return Err(GameBoyLinkExchangeError::RejectedPassiveCompletion {
                    responder: responder_side,
                });
            }
            responder.if_reg |= 0x08;
            true
        } else {
            false
        };

        Ok(GameBoyLinkTransferExchange {
            reply: reply_disposition,
            passive_responder_completed,
        })
    }
}
