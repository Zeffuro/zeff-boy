use crate::save_state::{StateReader, StateWriter};
use anyhow::Result;
use std::fmt;

#[derive(Clone, Debug)]
pub(super) enum SgbEvent {
    Pal01([u16; 4], [u16; 4]),
    Pal23([u16; 4], [u16; 4]),
    PalSet([u16; 4], u8, bool),
    PalTrn,
    MaskEn(u8),
    MltReq(u8),
    ChrTrn(u8),
    PctTrn,
    AttrTrn,
    AttrSet(u8, bool),
    AttrBlk(Vec<u8>),
    AttrLin(Vec<u8>),
    AttrDiv([u8; 16]),
    AttrChr(Vec<u8>),
}

pub(super) struct SgbState {
    collecting: bool,
    bit_count: u8,
    current_byte: u8,
    packet: [u8; 16],
    packet_pos: usize,
    packets_remaining: u8,
    multi_packet_data: Vec<u8>,
    multi_packet_command: u8,
}

impl fmt::Debug for SgbState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SgbState")
            .field("collecting", &self.collecting)
            .field("bit_count", &self.bit_count)
            .field("packet_pos", &self.packet_pos)
            .finish()
    }
}

impl SgbState {
    pub(super) fn new() -> Self {
        Self {
            collecting: false,
            bit_count: 0,
            current_byte: 0,
            packet: [0; 16],
            packet_pos: 0,
            packets_remaining: 0,
            multi_packet_data: Vec::new(),
            multi_packet_command: 0,
        }
    }

    pub(super) fn on_joyp_write(&mut self, value: u8) -> Option<SgbEvent> {
        let p14_low = (value & 0x10) == 0;
        let p15_low = (value & 0x20) == 0;

        if p14_low && p15_low {
            if self.collecting && self.packet_pos > 0 {
                log::warn!(
                    "SGB packet reset mid-transfer at byte {} (discarding partial packet)",
                    self.packet_pos
                );
            }
            self.collecting = true;
            self.bit_count = 0;
            self.current_byte = 0;
            self.packet_pos = 0;
            self.packet = [0; 16];
            log::info!("SGB packet start");
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

        let bit = bit?;

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

            if self.packets_remaining > 0 && !self.multi_packet_data.is_empty() {
                self.multi_packet_data.extend_from_slice(&self.packet);
                self.packets_remaining -= 1;
                if self.packets_remaining == 0 {
                    let result = self.parse_multi_packet();
                    self.multi_packet_data.clear();
                    self.multi_packet_command = 0;
                    return result;
                }
                return None;
            }

            if self.packets_remaining > 0 {
                self.packets_remaining -= 1;
                return None;
            }

            let command = self.packet[0] >> 3;
            let packet_count = self.packet[0] & 0x07;
            let packet_count = if packet_count == 0 { 1 } else { packet_count };
            log::info!(
                "SGB packet complete: command=0x{:02X}, packet_count={}",
                command,
                packet_count
            );

            if packet_count > 1 && needs_multi_packet(command) {
                self.multi_packet_command = command;
                self.multi_packet_data.clear();
                self.multi_packet_data.extend_from_slice(&self.packet);
                self.packets_remaining = packet_count - 1;
                return None;
            }

            if packet_count > 1 {
                self.packets_remaining = packet_count - 1;
            }

            return self.parse_packet();
        }

        None
    }

    fn parse_multi_packet(&self) -> Option<SgbEvent> {
        log::info!(
            "SGB multi-packet command 0x{:02X} complete: {} bytes total",
            self.multi_packet_command,
            self.multi_packet_data.len()
        );
        match self.multi_packet_command {
            0x04 => Some(SgbEvent::AttrBlk(self.multi_packet_data.clone())),
            0x05 => Some(SgbEvent::AttrLin(self.multi_packet_data.clone())),
            0x07 => Some(SgbEvent::AttrChr(self.multi_packet_data.clone())),
            _ => None,
        }
    }

    fn parse_packet(&self) -> Option<SgbEvent> {
        let command = self.packet[0] >> 3;

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
            0x04 => Some(SgbEvent::AttrBlk(self.packet.to_vec())),
            0x05 => Some(SgbEvent::AttrLin(self.packet.to_vec())),
            0x06 => Some(SgbEvent::AttrDiv(self.packet)),
            0x07 => Some(SgbEvent::AttrChr(self.packet.to_vec())),

            0x0A => {
                let pal0 = read_u16(&self.packet, 1) & 0x01FF;
                let pal1 = read_u16(&self.packet, 3) & 0x01FF;
                let pal2 = read_u16(&self.packet, 5) & 0x01FF;
                let pal3 = read_u16(&self.packet, 7) & 0x01FF;
                let attr_file = self.packet[9] & 0x3F;
                let cancel_mask = self.packet[9] & 0x40 != 0;
                Some(SgbEvent::PalSet(
                    [pal0, pal1, pal2, pal3],
                    attr_file,
                    cancel_mask,
                ))
            }
            0x0B => Some(SgbEvent::PalTrn),
            0x11 => Some(SgbEvent::MltReq(self.packet[1] & 0x03)),
            0x13 => Some(SgbEvent::ChrTrn(self.packet[1] & 0x01)),
            0x14 => Some(SgbEvent::PctTrn),
            0x15 => Some(SgbEvent::AttrTrn),
            0x16 => {
                let atf = self.packet[1] & 0x3F;
                let cancel_mask = self.packet[1] & 0x40 != 0;
                Some(SgbEvent::AttrSet(atf, cancel_mask))
            }
            0x17 => Some(SgbEvent::MaskEn(self.packet[1] & 0x03)),
            0x0F => None,
            0x10 => None,
            0x08 | 0x09 => None,
            0x0C..=0x0E => None,
            0x18 => None,
            _ => {
                log::warn!("Unrecognized SGB command {:02X}; skipping", command);
                None
            }
        }
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_bool(self.collecting);
        writer.write_u8(self.bit_count);
        writer.write_u8(self.current_byte);
        writer.write_bytes(&self.packet);
        writer.write_u64(self.packet_pos as u64);
        writer.write_u8(self.packets_remaining);
    }

    pub(super) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let collecting = reader.read_bool()?;
        let bit_count = reader.read_u8()?;
        let current_byte = reader.read_u8()?;
        let mut packet = [0u8; 16];
        reader.read_exact(&mut packet)?;
        let packet_pos = reader.read_u64()? as usize;
        let packets_remaining = reader.read_u8().unwrap_or(0);
        Ok(Self {
            collecting,
            bit_count,
            current_byte,
            packet,
            packet_pos: packet_pos.min(16),
            packets_remaining,
            multi_packet_data: Vec::new(),
            multi_packet_command: 0,
        })
    }
}

fn needs_multi_packet(command: u8) -> bool {
    matches!(command, 0x04 | 0x05 | 0x07)
}

fn read_u16(packet: &[u8; 16], index: usize) -> u16 {
    let lo = packet.get(index).copied().unwrap_or(0) as u16;
    let hi = packet.get(index + 1).copied().unwrap_or(0) as u16;
    lo | (hi << 8)
}

#[cfg(test)]
mod tests;
