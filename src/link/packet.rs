use super::protocol::{LINK_PROTOCOL_VERSION, LinkEndpointId, LinkSystemType, LinkTransferId};

const PACKET_MAGIC: &[u8; 4] = b"ZBLK";
const HEADER_LEN: usize = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum LinkPacketKind {
    Hello = 1,
    LinkEvent = 2,
    Disconnect = 3,
    LinkState = 4,
}

impl TryFrom<u8> for LinkPacketKind {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Hello),
            2 => Ok(Self::LinkEvent),
            3 => Ok(Self::Disconnect),
            4 => Ok(Self::LinkState),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinkPacket {
    pub(crate) system: LinkSystemType,
    pub(crate) endpoint: LinkEndpointId,
    pub(crate) transfer_id: LinkTransferId,
    pub(crate) kind: LinkPacketKind,
    pub(crate) payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkPacketDecodeError {
    TooShort,
    BadMagic,
    UnsupportedVersion(u8),
    UnknownSystem(u8),
    UnknownKind(u8),
    LengthMismatch { expected: usize, actual: usize },
}

impl LinkPacket {
    pub(crate) fn encode(&self) -> Vec<u8> {
        assert!(
            self.payload.len() <= usize::from(u16::MAX),
            "link packet payload exceeds u16 length field"
        );
        let payload_len = self.payload.len() as u16;
        let payload = &self.payload;
        let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
        out.extend_from_slice(PACKET_MAGIC);
        out.push(LINK_PROTOCOL_VERSION);
        out.push(self.system as u8);
        out.push(self.endpoint.0);
        out.push(self.kind as u8);
        out.extend_from_slice(&self.transfer_id.0.to_le_bytes());
        out.extend_from_slice(&payload_len.to_le_bytes());
        out.extend_from_slice(payload);
        out
    }

    pub(crate) fn decode(input: &[u8]) -> Result<Self, LinkPacketDecodeError> {
        if input.len() < HEADER_LEN {
            return Err(LinkPacketDecodeError::TooShort);
        }
        if &input[..4] != PACKET_MAGIC {
            return Err(LinkPacketDecodeError::BadMagic);
        }

        let version = input[4];
        if version != LINK_PROTOCOL_VERSION {
            return Err(LinkPacketDecodeError::UnsupportedVersion(version));
        }

        let system = LinkSystemType::try_from(input[5])
            .map_err(|_| LinkPacketDecodeError::UnknownSystem(input[5]))?;
        let endpoint = LinkEndpointId(input[6]);
        let kind = LinkPacketKind::try_from(input[7])
            .map_err(|_| LinkPacketDecodeError::UnknownKind(input[7]))?;
        let transfer_id = LinkTransferId(u32::from_le_bytes([
            input[8], input[9], input[10], input[11],
        ]));
        let payload_len = usize::from(u16::from_le_bytes([input[12], input[13]]));
        let expected_len = HEADER_LEN + payload_len;
        if input.len() != expected_len {
            return Err(LinkPacketDecodeError::LengthMismatch {
                expected: expected_len,
                actual: input.len(),
            });
        }

        Ok(Self {
            system,
            endpoint,
            transfer_id,
            kind,
            payload: input[HEADER_LEN..].to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_packet_roundtrips_protocol_fields_and_payload() {
        let packet = LinkPacket {
            system: LinkSystemType::GameBoy,
            endpoint: LinkEndpointId(2),
            transfer_id: LinkTransferId(0x1234_5678),
            kind: LinkPacketKind::LinkEvent,
            payload: vec![0xAB, 0xCD],
        };

        let encoded = packet.encode();

        assert_eq!(LinkPacket::decode(&encoded), Ok(packet));
    }

    #[test]
    fn link_packet_rejects_wrong_length() {
        let packet = LinkPacket {
            system: LinkSystemType::GameGear,
            endpoint: LinkEndpointId(1),
            transfer_id: LinkTransferId(7),
            kind: LinkPacketKind::Hello,
            payload: vec![0x01],
        };
        let mut encoded = packet.encode();
        encoded.pop();

        assert_eq!(
            LinkPacket::decode(&encoded),
            Err(LinkPacketDecodeError::LengthMismatch {
                expected: HEADER_LEN + 1,
                actual: HEADER_LEN,
            })
        );
    }
}
