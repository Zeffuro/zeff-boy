#![allow(dead_code, unused_imports)]

pub(crate) use self::packet::{LinkPacket, LinkPacketDecodeError, LinkPacketKind};
pub(crate) use self::protocol::{
    LINK_PROTOCOL_VERSION, LinkEndpointId, LinkSystemType, LinkTransferId,
};
pub(crate) use self::session::{LinkSession, LinkSessionError, LinkedEmulationSession};
pub(crate) use self::transport::{LinkConnectionState, LinkTransport, LinkTransportError};

pub(crate) mod gb;
pub(crate) mod packet;
pub(crate) mod protocol;
pub(crate) mod session;
pub(crate) mod transport;
