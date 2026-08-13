use zeff_ws_core::emulator::Emulator as WonderSwanEmulator;
use zeff_ws_core::hardware::bus::WonderSwanTxEvent;

use super::{LinkConnectionState, LinkPacketKind, LinkSession, LinkSessionError, LinkTransport};

const EVENT_TX_BYTE: u8 = 1;
const TX_BYTE_PAYLOAD_LEN: usize = 1 + 8 + 8 + 4 + 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WonderSwanLinkPayloadError {
    WrongLength { expected: usize, actual: usize },
    UnknownEvent(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WonderSwanLinkEvent {
    completed_cycle: u64,
    generation: u64,
    baud_bps: u32,
    byte: u8,
}

pub(crate) struct WonderSwanRemoteLink<T: LinkTransport> {
    session: LinkSession<T>,
}

impl<T: LinkTransport> WonderSwanRemoteLink<T> {
    pub(crate) fn new(session: LinkSession<T>) -> Self {
        Self { session }
    }

    pub(crate) fn state(&self) -> LinkConnectionState {
        self.session.state()
    }

    pub(crate) fn poll_emulator(
        &mut self,
        emulator: &mut WonderSwanEmulator,
    ) -> Result<(), LinkSessionError> {
        if self.state() == LinkConnectionState::Disconnected {
            return Err(LinkSessionError::Transport(
                super::LinkTransportError::Disconnected,
            ));
        }

        self.drain_incoming(emulator)?;
        self.send_completed_tx_events(emulator)?;
        self.drain_incoming(emulator)
    }

    pub(crate) fn disconnect(&mut self) {
        let _ = self.session.send(LinkPacketKind::Disconnect, &[]);
        self.session.disconnect();
    }

    fn drain_incoming(
        &mut self,
        emulator: &mut WonderSwanEmulator,
    ) -> Result<(), LinkSessionError> {
        loop {
            let Some(packet) = self.session.try_receive_packet()? else {
                return Ok(());
            };

            match packet.kind {
                LinkPacketKind::LinkEvent => {
                    let event = decode_wonder_swan_link_event(&packet.payload)
                        .map_err(|_| LinkSessionError::MalformedPacketPayload)?;
                    emulator.receive_wonder_swan_link_byte(event.byte);
                }
                LinkPacketKind::Disconnect => {
                    self.session.disconnect();
                    return Err(LinkSessionError::Transport(
                        super::LinkTransportError::Disconnected,
                    ));
                }
                LinkPacketKind::Hello | LinkPacketKind::LinkState => {}
            }
        }
    }

    fn send_completed_tx_events(
        &mut self,
        emulator: &mut WonderSwanEmulator,
    ) -> Result<(), LinkSessionError> {
        while let Some(event) = emulator.take_wonder_swan_link_tx_event() {
            self.send_event(event)?;
        }
        Ok(())
    }

    fn send_event(&mut self, event: WonderSwanTxEvent) -> Result<(), LinkSessionError> {
        self.session.send(
            LinkPacketKind::LinkEvent,
            &encode_wonder_swan_link_event(WonderSwanLinkEvent {
                completed_cycle: event.completed_cycle,
                generation: event.generation,
                baud_bps: event.baud_bps,
                byte: event.byte,
            }),
        )?;
        Ok(())
    }
}

fn encode_wonder_swan_link_event(event: WonderSwanLinkEvent) -> Vec<u8> {
    let mut out = Vec::with_capacity(TX_BYTE_PAYLOAD_LEN);
    out.push(EVENT_TX_BYTE);
    out.extend_from_slice(&event.completed_cycle.to_le_bytes());
    out.extend_from_slice(&event.generation.to_le_bytes());
    out.extend_from_slice(&event.baud_bps.to_le_bytes());
    out.push(event.byte);
    out
}

fn decode_wonder_swan_link_event(
    payload: &[u8],
) -> Result<WonderSwanLinkEvent, WonderSwanLinkPayloadError> {
    let Some(kind) = payload.first().copied() else {
        return Err(WonderSwanLinkPayloadError::WrongLength {
            expected: 1,
            actual: 0,
        });
    };
    if kind != EVENT_TX_BYTE {
        return Err(WonderSwanLinkPayloadError::UnknownEvent(kind));
    }
    if payload.len() != TX_BYTE_PAYLOAD_LEN {
        return Err(WonderSwanLinkPayloadError::WrongLength {
            expected: TX_BYTE_PAYLOAD_LEN,
            actual: payload.len(),
        });
    }

    Ok(WonderSwanLinkEvent {
        completed_cycle: read_u64(payload, 1),
        generation: read_u64(payload, 9),
        baud_bps: read_u32(payload, 17),
        byte: payload[21],
    })
}

fn read_u64(payload: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        payload[offset..offset + 8]
            .try_into()
            .expect("payload length checked before u64 read"),
    )
}

fn read_u32(payload: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        payload[offset..offset + 4]
            .try_into()
            .expect("payload length checked before u32 read"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wonder_swan_link_event_payload_roundtrips_tx_byte() {
        let event = WonderSwanLinkEvent {
            completed_cycle: 12_345,
            generation: 7,
            baud_bps: 38_400,
            byte: 0x5A,
        };

        assert_eq!(
            decode_wonder_swan_link_event(&encode_wonder_swan_link_event(event)),
            Ok(event)
        );
    }
}
