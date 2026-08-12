use crate::emu_backend::EmuBackend;

use super::packet::{LinkPacket, LinkPacketDecodeError, LinkPacketKind};
use super::protocol::{LinkEndpointId, LinkSystemType, LinkTransferId};
use super::transport::{LinkConnectionState, LinkTransport, LinkTransportError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkSessionError {
    Transport(LinkTransportError),
    Decode(LinkPacketDecodeError),
    MalformedPacketPayload,
    UnexpectedSystem {
        expected: LinkSystemType,
        actual: LinkSystemType,
    },
    IncompatibleSystems,
}

impl From<LinkTransportError> for LinkSessionError {
    fn from(value: LinkTransportError) -> Self {
        Self::Transport(value)
    }
}

impl From<LinkPacketDecodeError> for LinkSessionError {
    fn from(value: LinkPacketDecodeError) -> Self {
        Self::Decode(value)
    }
}

pub(crate) struct LinkSession<T: LinkTransport> {
    transport: T,
    system: LinkSystemType,
    endpoint: LinkEndpointId,
    next_transfer_id: u32,
}

impl<T: LinkTransport> LinkSession<T> {
    pub(crate) fn new(transport: T, system: LinkSystemType, endpoint: LinkEndpointId) -> Self {
        Self {
            transport,
            system,
            endpoint,
            next_transfer_id: 0,
        }
    }

    pub(crate) fn state(&self) -> LinkConnectionState {
        self.transport.state()
    }

    pub(crate) fn endpoint(&self) -> LinkEndpointId {
        self.endpoint
    }

    pub(crate) fn send(
        &mut self,
        kind: LinkPacketKind,
        payload: &[u8],
    ) -> Result<LinkTransferId, LinkSessionError> {
        let transfer_id = LinkTransferId(self.next_transfer_id);
        self.next_transfer_id = self.next_transfer_id.wrapping_add(1);
        let packet = LinkPacket {
            system: self.system,
            endpoint: self.endpoint,
            transfer_id,
            kind,
            payload: payload.to_vec(),
        };

        self.transport.send(&packet.encode())?;
        Ok(transfer_id)
    }

    pub(crate) fn try_receive_packet(&mut self) -> Result<Option<LinkPacket>, LinkSessionError> {
        let Some(bytes) = self.transport.try_receive()? else {
            return Ok(None);
        };

        let packet = LinkPacket::decode(&bytes)?;
        if packet.system != self.system {
            return Err(LinkSessionError::UnexpectedSystem {
                expected: self.system,
                actual: packet.system,
            });
        }

        Ok(Some(packet))
    }

    pub(crate) fn disconnect(&mut self) {
        self.transport.disconnect();
    }
}

pub(crate) struct LinkedEmulationSession {
    first: EmuBackend,
    second: EmuBackend,
}

impl LinkedEmulationSession {
    pub(crate) fn new(
        mut first: EmuBackend,
        mut second: EmuBackend,
    ) -> Result<Self, LinkSessionError> {
        if !first.sync_link_peer(&mut second) {
            return Err(LinkSessionError::IncompatibleSystems);
        }

        Ok(Self { first, second })
    }

    pub(crate) fn backends(&self) -> (&EmuBackend, &EmuBackend) {
        (&self.first, &self.second)
    }

    pub(crate) fn backends_mut(&mut self) -> (&mut EmuBackend, &mut EmuBackend) {
        (&mut self.first, &mut self.second)
    }

    pub(crate) fn sync_link_peer(&mut self) -> Result<(), LinkSessionError> {
        if self.first.sync_link_peer(&mut self.second) {
            Ok(())
        } else {
            Err(LinkSessionError::IncompatibleSystems)
        }
    }

    pub(crate) fn step_frame_pair(&mut self) -> Result<(), LinkSessionError> {
        self.sync_link_peer()?;
        self.first.step_frame();
        self.second.step_frame();
        self.sync_link_peer()
    }

    pub(crate) fn into_backends(self) -> (EmuBackend, EmuBackend) {
        (self.first, self.second)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use zeff_gb_core::hardware::types::constants::{INTERRUPT_IF, SERIAL_SB, SERIAL_SC};
    use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

    use super::*;
    use crate::link::transport::LocalLinkTransport;

    #[test]
    fn link_session_sends_framed_packets_over_local_transport() {
        let (left_transport, right_transport) = LocalLinkTransport::pair();
        let mut left = LinkSession::new(left_transport, LinkSystemType::GameBoy, LinkEndpointId(1));
        let mut right =
            LinkSession::new(right_transport, LinkSystemType::GameBoy, LinkEndpointId(2));

        let transfer_id = left.send(LinkPacketKind::LinkEvent, &[0x12, 0x34]).unwrap();
        let received = right.try_receive_packet().unwrap().unwrap();

        assert_eq!(transfer_id, LinkTransferId(0));
        assert_eq!(received.system, LinkSystemType::GameBoy);
        assert_eq!(received.endpoint, LinkEndpointId(1));
        assert_eq!(received.transfer_id, LinkTransferId(0));
        assert_eq!(received.kind, LinkPacketKind::LinkEvent);
        assert_eq!(received.payload, vec![0x12, 0x34]);
        assert_eq!(right.try_receive_packet().unwrap(), None);
    }

    #[test]
    fn link_session_rejects_wrong_system_packets() {
        let (left_transport, right_transport) = LocalLinkTransport::pair();
        let mut left = LinkSession::new(left_transport, LinkSystemType::GameBoy, LinkEndpointId(1));
        let mut right =
            LinkSession::new(right_transport, LinkSystemType::GameGear, LinkEndpointId(2));

        left.send(LinkPacketKind::Hello, &[]).unwrap();

        assert_eq!(
            right.try_receive_packet(),
            Err(LinkSessionError::UnexpectedSystem {
                expected: LinkSystemType::GameGear,
                actual: LinkSystemType::GameBoy,
            })
        );
    }

    #[test]
    fn linked_emulation_session_steps_game_boy_link_pair() {
        let mut session = LinkedEmulationSession::new(gb_backend(), gb_backend()).unwrap();

        {
            let (left, right) = session.backends_mut();
            let (EmuBackend::Gb(left), EmuBackend::Gb(right)) = (left, right) else {
                panic!("expected GB backends");
            };
            left.emu.write_byte(SERIAL_SB, 0x5A);
            right.emu.write_byte(SERIAL_SB, 0xC3);
            left.emu.write_byte(SERIAL_SC, 0x81);
            right.emu.write_byte(SERIAL_SC, 0x80);
        }

        session.step_frame_pair().unwrap();

        let (left, right) = session.backends();
        let (EmuBackend::Gb(left), EmuBackend::Gb(right)) = (left, right) else {
            panic!("expected GB backends");
        };
        assert_eq!(left.emu.cpu_peek8(SERIAL_SB), 0xC3);
        assert_eq!(right.emu.cpu_peek8(SERIAL_SB), 0x5A);
        assert_eq!(left.emu.cpu_peek8(SERIAL_SC) & 0x80, 0);
        assert_eq!(right.emu.cpu_peek8(SERIAL_SC) & 0x80, 0);
        assert_eq!(left.emu.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
        assert_eq!(right.emu.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
    }

    #[test]
    fn linked_emulation_session_rejects_incompatible_backends() {
        assert_eq!(
            LinkedEmulationSession::new(gb_backend(), gba_backend()).map(|_| ()),
            Err(LinkSessionError::IncompatibleSystems)
        );
    }

    fn gb_backend() -> EmuBackend {
        let rom = vec![0u8; 0x8000];
        let gb =
            zeff_gb_core::emulator::Emulator::from_rom_data(&rom, HardwareModePreference::Auto)
                .expect("GB emulator should initialize");
        EmuBackend::from_gb(gb, PathBuf::from("test.gb"))
    }

    fn gba_backend() -> EmuBackend {
        let mut rom = vec![0u8; 0xC0];
        rom[0xA0..0xA4].copy_from_slice(b"TEST");
        rom[0xAC..0xB0].copy_from_slice(b"ABCD");
        rom[0xB0..0xB2].copy_from_slice(b"01");
        rom[0xB2] = 0x96;
        let gba = zeff_gba_core::emulator::Emulator::new(&rom, 44_100)
            .expect("GBA emulator should initialize");
        EmuBackend::from_gba(gba, PathBuf::from("test.gba"))
    }
}
