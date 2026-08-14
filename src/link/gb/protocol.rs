use zeff_gb_core::hardware::bus::{GameBoyLinkAction, GameBoyLinkReply};

const EVENT_MASTER_START: u8 = 1;
const EVENT_TRANSFER_REPLY: u8 = 2;

const MASTER_START_PAYLOAD_LEN: usize = 1 + 8 + 8 + 8 + 1 + 8;
const TRANSFER_REPLY_PAYLOAD_LEN: usize = 1 + 8 + 8 + 1 + 1 + 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct GbTransferId(pub(super) u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GameBoyLinkPayloadError {
    WrongLength { expected: usize, actual: usize },
    UnknownEvent(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GameBoyLinkEvent {
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

pub(super) fn encode_game_boy_link_event(event: GameBoyLinkEvent) -> Vec<u8> {
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

pub(super) fn decode_game_boy_link_event(
    payload: &[u8],
) -> Result<GameBoyLinkEvent, GameBoyLinkPayloadError> {
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
