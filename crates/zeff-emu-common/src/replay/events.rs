use crate::media::{MediaEvent, MediaObjectId, MediaSlotId};
use anyhow::{Result, bail};

use super::{
    MetadataCursor, read_bool, read_optional_u8, write_optional_u8, write_string, write_u32,
    write_u64,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayEvent {
    FdsDiskSide {
        frame: u64,
        side: u8,
    },
    Media {
        frame: u64,
        sequence: u32,
        event: MediaEvent,
    },
    GameBoyLink {
        frame: u64,
        tick: u64,
        event: ReplayGameBoyLinkEvent,
    },
    GameBoyLinkState {
        frame: u64,
        state: ReplayGameBoyLinkState,
    },
    GameBoyLinkStateAtTick {
        frame: u64,
        tick: u64,
        state: ReplayGameBoyLinkState,
    },
    WonderSwanLink {
        frame: u64,
        session_cycle: u64,
        event: ReplayWonderSwanLinkEvent,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayGameBoyLinkEvent {
    LocalMasterStart {
        transfer_id: u64,
        clock_period_t_cycles: u64,
        out_byte: u8,
        serial_generation: u64,
    },
    RemoteMasterStart {
        transfer_id: u64,
        clock_period_t_cycles: u64,
        out_byte: u8,
        serial_generation: u64,
        local_reply: Option<ReplayGameBoyLinkReply>,
    },
    RemoteReply {
        transfer_id: u64,
        out_byte: u8,
        passive: bool,
        serial_generation: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayGameBoyLinkReply {
    pub out_byte: u8,
    pub passive: bool,
    pub serial_generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayGameBoyLinkAction {
    pub out_byte: u8,
    pub clock_period_t_cycles: u64,
    pub serial_generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayGameBoyLinkState {
    pub peer_present: bool,
    pub pending_master_byte: Option<u8>,
    pub pending_master_response: Option<u8>,
    pub pending_master_completion_ready: bool,
    pub queued_master_action: Option<ReplayGameBoyLinkAction>,
    pub serial_generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayWonderSwanLinkEvent {
    RemoteByte {
        generation: u64,
        baud_bps: u32,
        byte: u8,
    },
}

impl ReplayEvent {
    pub fn frame(&self) -> u64 {
        match self {
            Self::FdsDiskSide { frame, .. } => *frame,
            Self::Media { frame, .. } => *frame,
            Self::GameBoyLink { frame, .. } => *frame,
            Self::GameBoyLinkState { frame, .. } => *frame,
            Self::GameBoyLinkStateAtTick { frame, .. } => *frame,
            Self::WonderSwanLink { frame, .. } => *frame,
        }
    }

    pub(super) fn sort_key(&self) -> (u64, u64, u8, u32) {
        match self {
            Self::FdsDiskSide { frame, .. } => (*frame, 0, 0, 0),
            Self::Media {
                frame, sequence, ..
            } => (*frame, 0, 0, *sequence),
            Self::GameBoyLinkState { frame, .. } => (*frame, 0, 1, 0),
            Self::GameBoyLinkStateAtTick { frame, tick, .. } => (*frame, *tick, 2, 0),
            Self::GameBoyLink { frame, tick, event } => {
                (*frame, *tick, 3 + event.sort_discriminant(), 0)
            }
            Self::WonderSwanLink {
                frame,
                session_cycle,
                ..
            } => (*frame, *session_cycle, 5, 0),
        }
    }

    pub(super) fn is_frame_boundary_event(&self) -> bool {
        matches!(
            self,
            Self::FdsDiskSide { .. } | Self::Media { .. } | Self::GameBoyLinkState { .. }
        )
    }

    pub(super) fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Self::FdsDiskSide { frame, side } => {
                out.push(0);
                write_u64(out, *frame);
                out.push(*side);
            }
            Self::Media {
                frame,
                sequence,
                event,
            } => {
                out.push(5);
                write_u64(out, *frame);
                write_u32(out, *sequence);
                encode_media_event(out, event);
            }
            Self::GameBoyLink { frame, tick, event } => {
                out.push(1);
                write_u64(out, *frame);
                write_u64(out, *tick);
                event.encode(out);
            }
            Self::GameBoyLinkState { frame, state } => {
                out.push(3);
                write_u64(out, *frame);
                state.encode(out);
            }
            Self::GameBoyLinkStateAtTick { frame, tick, state } => {
                out.push(4);
                write_u64(out, *frame);
                write_u64(out, *tick);
                state.encode(out);
            }
            Self::WonderSwanLink {
                frame,
                session_cycle,
                event,
            } => {
                out.push(2);
                write_u64(out, *frame);
                write_u64(out, *session_cycle);
                event.encode(out);
            }
        }
    }

    pub(super) fn decode(cursor: &mut MetadataCursor<'_>) -> Result<Self> {
        match cursor.read_u8()? {
            0 => Ok(Self::FdsDiskSide {
                frame: cursor.read_u64()?,
                side: cursor.read_u8()?,
            }),
            5 => Ok(Self::Media {
                frame: cursor.read_u64()?,
                sequence: cursor.read_u32()?,
                event: decode_media_event(cursor)?,
            }),
            1 => Ok(Self::GameBoyLink {
                frame: cursor.read_u64()?,
                tick: cursor.read_u64()?,
                event: ReplayGameBoyLinkEvent::decode(cursor)?,
            }),
            3 => Ok(Self::GameBoyLinkState {
                frame: cursor.read_u64()?,
                state: ReplayGameBoyLinkState::decode(cursor)?,
            }),
            4 => Ok(Self::GameBoyLinkStateAtTick {
                frame: cursor.read_u64()?,
                tick: cursor.read_u64()?,
                state: ReplayGameBoyLinkState::decode(cursor)?,
            }),
            2 => Ok(Self::WonderSwanLink {
                frame: cursor.read_u64()?,
                session_cycle: cursor.read_u64()?,
                event: ReplayWonderSwanLinkEvent::decode(cursor)?,
            }),
            tag => bail!("unknown replay event tag: {tag}"),
        }
    }
}

fn encode_media_event(out: &mut Vec<u8>, event: &MediaEvent) {
    match event {
        MediaEvent::Insert {
            slot,
            media_id,
            side,
            write_protected,
        } => {
            out.push(0);
            write_string(out, slot.as_ref());
            write_string(out, media_id.as_ref());
            write_optional_u8(out, *side);
            out.push(u8::from(*write_protected));
        }
        MediaEvent::Eject { slot } => {
            out.push(1);
            write_string(out, slot.as_ref());
        }
        MediaEvent::SelectSide { slot, side } => {
            out.push(2);
            write_string(out, slot.as_ref());
            out.push(*side);
        }
        MediaEvent::SetWriteProtected {
            slot,
            write_protected,
        } => {
            out.push(3);
            write_string(out, slot.as_ref());
            out.push(u8::from(*write_protected));
        }
    }
}

fn decode_media_event(cursor: &mut MetadataCursor<'_>) -> Result<MediaEvent> {
    match cursor.read_u8()? {
        0 => Ok(MediaEvent::Insert {
            slot: MediaSlotId::new(cursor.read_string()?),
            media_id: MediaObjectId::new(cursor.read_string()?),
            side: read_optional_u8(cursor, "media insert side")?,
            write_protected: read_bool(cursor, "media insert write-protected flag")?,
        }),
        1 => Ok(MediaEvent::Eject {
            slot: MediaSlotId::new(cursor.read_string()?),
        }),
        2 => Ok(MediaEvent::SelectSide {
            slot: MediaSlotId::new(cursor.read_string()?),
            side: cursor.read_u8()?,
        }),
        3 => Ok(MediaEvent::SetWriteProtected {
            slot: MediaSlotId::new(cursor.read_string()?),
            write_protected: read_bool(cursor, "media write-protected flag")?,
        }),
        tag => bail!("unknown replay media event tag: {tag}"),
    }
}

impl ReplayGameBoyLinkEvent {
    fn sort_discriminant(&self) -> u8 {
        match self {
            Self::LocalMasterStart { .. } => 0,
            Self::RemoteMasterStart { .. } => 1,
            Self::RemoteReply { .. } => 2,
        }
    }

    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Self::LocalMasterStart {
                transfer_id,
                clock_period_t_cycles,
                out_byte,
                serial_generation,
            } => {
                out.push(3);
                write_u64(out, *transfer_id);
                write_u64(out, *clock_period_t_cycles);
                out.push(*out_byte);
                write_u64(out, *serial_generation);
            }
            Self::RemoteMasterStart {
                transfer_id,
                clock_period_t_cycles,
                out_byte,
                serial_generation,
                local_reply,
            } => match local_reply {
                Some(reply) => {
                    out.push(2);
                    write_u64(out, *transfer_id);
                    write_u64(out, *clock_period_t_cycles);
                    out.push(*out_byte);
                    write_u64(out, *serial_generation);
                    reply.encode(out);
                }
                None => {
                    out.push(0);
                    write_u64(out, *transfer_id);
                    write_u64(out, *clock_period_t_cycles);
                    out.push(*out_byte);
                    write_u64(out, *serial_generation);
                }
            },
            Self::RemoteReply {
                transfer_id,
                out_byte,
                passive,
                serial_generation,
            } => {
                out.push(1);
                write_u64(out, *transfer_id);
                out.push(*out_byte);
                out.push(u8::from(*passive));
                write_u64(out, *serial_generation);
            }
        }
    }

    fn decode(cursor: &mut MetadataCursor<'_>) -> Result<Self> {
        match cursor.read_u8()? {
            0 => Ok(Self::RemoteMasterStart {
                transfer_id: cursor.read_u64()?,
                clock_period_t_cycles: cursor.read_u64()?,
                out_byte: cursor.read_u8()?,
                serial_generation: cursor.read_u64()?,
                local_reply: None,
            }),
            1 => Ok(Self::RemoteReply {
                transfer_id: cursor.read_u64()?,
                out_byte: cursor.read_u8()?,
                passive: read_bool(cursor, "GB link reply passive flag")?,
                serial_generation: cursor.read_u64()?,
            }),
            2 => Ok(Self::RemoteMasterStart {
                transfer_id: cursor.read_u64()?,
                clock_period_t_cycles: cursor.read_u64()?,
                out_byte: cursor.read_u8()?,
                serial_generation: cursor.read_u64()?,
                local_reply: Some(ReplayGameBoyLinkReply::decode(cursor)?),
            }),
            3 => Ok(Self::LocalMasterStart {
                transfer_id: cursor.read_u64()?,
                clock_period_t_cycles: cursor.read_u64()?,
                out_byte: cursor.read_u8()?,
                serial_generation: cursor.read_u64()?,
            }),
            tag => bail!("unknown GB replay link event tag: {tag}"),
        }
    }
}

impl ReplayGameBoyLinkReply {
    fn encode(&self, out: &mut Vec<u8>) {
        out.push(self.out_byte);
        out.push(u8::from(self.passive));
        write_u64(out, self.serial_generation);
    }

    fn decode(cursor: &mut MetadataCursor<'_>) -> Result<Self> {
        Ok(Self {
            out_byte: cursor.read_u8()?,
            passive: read_bool(cursor, "GB remote-master local reply passive flag")?,
            serial_generation: cursor.read_u64()?,
        })
    }
}

impl ReplayGameBoyLinkAction {
    fn encode(self, out: &mut Vec<u8>) {
        out.push(self.out_byte);
        write_u64(out, self.clock_period_t_cycles);
        write_u64(out, self.serial_generation);
    }

    fn decode(cursor: &mut MetadataCursor<'_>) -> Result<Self> {
        Ok(Self {
            out_byte: cursor.read_u8()?,
            clock_period_t_cycles: cursor.read_u64()?,
            serial_generation: cursor.read_u64()?,
        })
    }
}

impl ReplayGameBoyLinkState {
    pub fn is_idle(self) -> bool {
        !self.peer_present
            && self.pending_master_byte.is_none()
            && self.pending_master_response.is_none()
            && !self.pending_master_completion_ready
            && self.queued_master_action.is_none()
            && self.serial_generation == 0
    }

    pub(super) fn encode(self, out: &mut Vec<u8>) {
        out.push(u8::from(self.peer_present));
        write_optional_u8(out, self.pending_master_byte);
        write_optional_u8(out, self.pending_master_response);
        out.push(u8::from(self.pending_master_completion_ready));
        match self.queued_master_action {
            Some(action) => {
                out.push(1);
                action.encode(out);
            }
            None => out.push(0),
        }
        write_u64(out, self.serial_generation);
    }

    pub(super) fn decode(cursor: &mut MetadataCursor<'_>) -> Result<Self> {
        Ok(Self {
            peer_present: read_bool(cursor, "GB link start peer-present flag")?,
            pending_master_byte: read_optional_u8(cursor, "GB link start pending master byte")?,
            pending_master_response: read_optional_u8(
                cursor,
                "GB link start pending master response",
            )?,
            pending_master_completion_ready: read_bool(
                cursor,
                "GB link start pending master completion flag",
            )?,
            queued_master_action: if read_bool(cursor, "GB link start queued action flag")? {
                Some(ReplayGameBoyLinkAction::decode(cursor)?)
            } else {
                None
            },
            serial_generation: cursor.read_u64()?,
        })
    }
}

impl ReplayWonderSwanLinkEvent {
    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Self::RemoteByte {
                generation,
                baud_bps,
                byte,
            } => {
                out.push(0);
                write_u64(out, *generation);
                write_u32(out, *baud_bps);
                out.push(*byte);
            }
        }
    }

    fn decode(cursor: &mut MetadataCursor<'_>) -> Result<Self> {
        match cursor.read_u8()? {
            0 => Ok(Self::RemoteByte {
                generation: cursor.read_u64()?,
                baud_bps: cursor.read_u32()?,
                byte: cursor.read_u8()?,
            }),
            tag => bail!("unknown WonderSwan replay link event tag: {tag}"),
        }
    }
}
