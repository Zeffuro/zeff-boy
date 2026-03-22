use crate::save_state::{StateReader, StateWriter};
use anyhow::Result;

#[derive(Clone, Copy)]
pub(crate) enum SgbEvent {
    Pal01([u16; 4], [u16; 4]),
    Pal23([u16; 4], [u16; 4]),
    PalSet(u8),
    MaskEn(u8),
    MltReq,
}

pub(crate) struct SgbState {
    collecting: bool,
    bit_count: u8,
    current_byte: u8,
    packet: [u8; 16],
    packet_pos: usize,
}

impl SgbState {
    pub(crate) fn new() -> Self {
        Self {
            collecting: false,
            bit_count: 0,
            current_byte: 0,
            packet: [0; 16],
            packet_pos: 0,
        }
    }

    pub(crate) fn on_joyp_write(&mut self, value: u8) -> Option<SgbEvent> {
        let p14_low = (value & 0x10) == 0;
        let p15_low = (value & 0x20) == 0;

        if p14_low && p15_low {
            self.collecting = true;
            self.bit_count = 0;
            self.current_byte = 0;
            self.packet_pos = 0;
            self.packet = [0; 16];
            return None;
        }

        if !self.collecting {
            return None;
        }

        let bit = match (p14_low, p15_low) {
            (true, false) => Some(0u8),
            (false, true) => Some(1u8),
            _ => None,
        };

        let Some(bit) = bit else {
            return None;
        };

        self.current_byte |= bit << self.bit_count;
        self.bit_count += 1;

        if self.bit_count == 8 {
            if self.packet_pos < self.packet.len() {
                self.packet[self.packet_pos] = self.current_byte;
                self.packet_pos += 1;
            }
            self.bit_count = 0;
            self.current_byte = 0;
        }

        if self.packet_pos == self.packet.len() {
            self.collecting = false;
            return self.parse_packet();
        }

        None
    }

    fn parse_packet(&self) -> Option<SgbEvent> {
        let command = self.packet[0] >> 3;
        let packet_count = self.packet[0] & 0x07;
        if packet_count > 1 {
            log::warn!(
                "SGB command {:02X} requested {} packets; only single-packet commands are supported",
                command,
                packet_count
            );
            return None;
        }

        match command {
            0x00 => {
                let common = read_u16(&self.packet, 1);
                let pal0 = [
                    common,
                    read_u16(&self.packet, 3),
                    read_u16(&self.packet, 5),
                    read_u16(&self.packet, 7),
                ];
                let pal1 = [
                    common,
                    read_u16(&self.packet, 9),
                    read_u16(&self.packet, 11),
                    read_u16(&self.packet, 13),
                ];
                Some(SgbEvent::Pal01(pal0, pal1))
            }
            0x01 => {
                let common = read_u16(&self.packet, 1);
                let pal2 = [
                    common,
                    read_u16(&self.packet, 3),
                    read_u16(&self.packet, 5),
                    read_u16(&self.packet, 7),
                ];
                let pal3 = [
                    common,
                    read_u16(&self.packet, 9),
                    read_u16(&self.packet, 11),
                    read_u16(&self.packet, 13),
                ];
                Some(SgbEvent::Pal23(pal2, pal3))
            }
            0x0A => Some(SgbEvent::PalSet(self.packet[1] & 0x03)),
            0x11 => Some(SgbEvent::MltReq),
            0x17 => Some(SgbEvent::MaskEn(self.packet[1] & 0x03)),
            _ => {
                log::warn!("Unrecognized SGB command {:02X}; skipping", command);
                None
            }
        }
    }

    pub(crate) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_bool(self.collecting);
        writer.write_u8(self.bit_count);
        writer.write_u8(self.current_byte);
        writer.write_bytes(&self.packet);
        writer.write_u64(self.packet_pos as u64);
    }

    pub(crate) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let collecting = reader.read_bool()?;
        let bit_count = reader.read_u8()?;
        let current_byte = reader.read_u8()?;
        let mut packet = [0u8; 16];
        reader.read_exact(&mut packet)?;
        let packet_pos = reader.read_u64()? as usize;
        Ok(Self {
            collecting,
            bit_count,
            current_byte,
            packet,
            packet_pos: packet_pos.min(16),
        })
    }
}

fn read_u16(packet: &[u8; 16], index: usize) -> u16 {
    let lo = packet.get(index).copied().unwrap_or(0) as u16;
    let hi = packet.get(index + 1).copied().unwrap_or(0) as u16;
    lo | (hi << 8)
}

#[cfg(test)]
mod tests {
    use super::{SgbEvent, SgbState};

    fn feed_packet(state: &mut SgbState, packet: [u8; 16]) -> Option<SgbEvent> {
        let mut out = state.on_joyp_write(0x00);
        for byte in packet {
            for bit in 0..8 {
                let value = if (byte >> bit) & 1 == 0 { 0x20 } else { 0x10 };
                out = state.on_joyp_write(value);
            }
        }
        out
    }

    #[test]
    fn decodes_pal01_packet() {
        let mut state = SgbState::new();
        let mut packet = [0u8; 16];
        packet[0] = (0x00 << 3) | 0x01;
        packet[1] = 0xFF;
        packet[2] = 0x7F;
        packet[3] = 0x00;
        packet[4] = 0x00;

        let event = feed_packet(&mut state, packet).expect("expected PAL01 event");
        match event {
            SgbEvent::Pal01(p0, p1) => {
                assert_eq!(p0[0], 0x7FFF);
                assert_eq!(p1[0], 0x7FFF);
            }
            _ => panic!("unexpected event"),
        }
    }

    #[test]
    fn unknown_command_is_ignored() {
        let mut state = SgbState::new();
        let mut packet = [0u8; 16];
        packet[0] = (0x1E << 3) | 0x01;
        assert!(feed_packet(&mut state, packet).is_none());
    }
}
