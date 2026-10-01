use std::collections::{BTreeMap, BTreeSet};

use super::super::{NesNativeChannel, prg_span};
use crate::{Budget, RomSpan, ScanStop};

#[cfg(test)]
mod tests;

pub(super) struct Data {
    pub four_channels: bool,
    pub header: RomSpan,
    pub entries: [RomSpan; 2],
    pub channels: [Vec<NesNativeChannel>; 2],
    pub spans: Vec<RomSpan>,
}

enum Error {
    Invalid,
    Stop(ScanStop),
}
type Parsed<T> = Result<T, Error>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Header,
    Instrument,
    EnvelopeCommand,
    EnvelopeOperand,
    ChannelCommand,
    ChannelOperand,
}

struct Parser<'a, 'b, 'c> {
    bytes: &'a [u8],
    header: u16,
    budget: &'b mut Budget<'c>,
    roles: BTreeMap<u16, Role>,
}

impl Parser<'_, '_, '_> {
    fn byte(&mut self, address: u16, role: Role) -> Parsed<u8> {
        self.budget.charge().map_err(Error::Stop)?;
        if address < self.header || address >= 0xf800 || address >> 8 != self.header >> 8 {
            return Err(Error::Invalid);
        }
        if self
            .roles
            .insert(address, role)
            .is_some_and(|old| old != role)
        {
            return Err(Error::Invalid);
        }
        self.bytes
            .get(usize::from(address - 0x8000) + 16)
            .copied()
            .ok_or(Error::Invalid)
    }

    fn word(&mut self, address: u16, role: Role) -> Parsed<u16> {
        Ok(u16::from_le_bytes([
            self.byte(address, role)?,
            self.byte(address.checked_add(1).ok_or(Error::Invalid)?, role)?,
        ]))
    }

    fn data_address(&self, address: u16) -> Parsed<()> {
        if address < self.header + 33 || address >= 0xf800 || address >> 8 != self.header >> 8 {
            return Err(Error::Invalid);
        }
        Ok(())
    }

    fn envelope(&mut self, address: u16, volume: bool) -> Parsed<()> {
        self.data_address(address)?;
        let mut seen = BTreeMap::new();
        let mut position = 0_u16;
        let mut yields = 0;
        let mut has_value = false;
        let mut has_volume = false;
        for _ in 0..256 {
            if let Some(previous) = seen.insert(position, yields) {
                return if yields > previous && has_value && (!volume || has_volume) {
                    Ok(())
                } else {
                    Err(Error::Invalid)
                };
            }
            if position >= 32 {
                return Err(Error::Invalid);
            }
            let at = address.checked_add(position).ok_or(Error::Invalid)?;
            let opcode = self.byte(at, Role::EnvelopeCommand)?;
            if opcode >= 128 {
                let value = i16::from(opcode) - 192;
                if (volume && !(0..=15).contains(&value)) || (!volume && value != 0) {
                    return Err(Error::Invalid);
                }
                has_value = true;
                has_volume |= value > 0;
                yields += 1;
                position += 1;
            } else if opcode != 0 {
                yields += 1;
                position += 1;
            } else {
                if position + 1 >= 32 {
                    return Err(Error::Invalid);
                }
                position = u16::from(self.byte(
                    at.checked_add(1).ok_or(Error::Invalid)?,
                    Role::EnvelopeOperand,
                )?);
            }
        }
        Err(Error::Invalid)
    }

    fn channel(&mut self, address: u16, max_note: u8) -> Parsed<(RomSpan, bool)> {
        self.data_address(address)?;
        let mut seen = BTreeMap::new();
        let mut pc = address;
        let mut low = address;
        let mut high = address;
        let mut yields = 0;
        let mut has_note = false;
        let mut uses_voice = false;
        let mut visited_bytes = BTreeSet::new();
        for _ in 0..256 {
            if let Some(previous) = seen.insert(pc, yields) {
                if yields <= previous
                    || uses_voice != has_note
                    || visited_bytes.len() != usize::from(high - low)
                {
                    return Err(Error::Invalid);
                }
                let span =
                    prg_span(self.bytes, low, usize::from(high - low)).ok_or(Error::Invalid)?;
                return Ok((span, uses_voice));
            }
            self.data_address(pc)?;
            low = low.min(pc);
            let opcode = self.byte(pc, Role::ChannelCommand)?;
            visited_bytes.insert(pc);
            pc = pc.checked_add(1).ok_or(Error::Invalid)?;
            high = high.max(pc);
            match opcode {
                0..=127 if max_note != 0 && opcode >> 1 <= max_note => {
                    uses_voice = true;
                    yields += 1;
                    has_note |= opcode >> 1 != 0;
                }
                0x80 if max_note != 0 => uses_voice = true,
                value if (0x81..0xfb).contains(&value) && value & 1 == 1 => {
                    yields += 1;
                }
                0xfb if max_note != 0 => {
                    uses_voice = true;
                    let speed = self.byte(pc, Role::ChannelOperand)?;
                    visited_bytes.insert(pc);
                    if !(1..=127).contains(&speed) {
                        return Err(Error::Invalid);
                    }
                    pc = pc.checked_add(1).ok_or(Error::Invalid)?;
                    high = high.max(pc);
                }
                0xfd => {
                    let target = self.word(pc, Role::ChannelOperand)?;
                    visited_bytes.insert(pc);
                    visited_bytes.insert(pc + 1);
                    high = high.max(pc.checked_add(2).ok_or(Error::Invalid)?);
                    pc = target;
                }
                _ => return Err(Error::Invalid),
            }
        }
        Err(Error::Invalid)
    }

    fn inspect(mut self) -> Parsed<Data> {
        if self.byte(self.header, Role::Header)? != 2 {
            return Err(Error::Invalid);
        }
        let instrument = self.word(self.header + 1, Role::Header)?;
        self.word(self.header + 3, Role::Header)?;
        self.data_address(instrument)?;
        if self.byte(instrument, Role::Instrument)? != 0xb0
            || self.byte(instrument + 7, Role::Instrument)? != 0
        {
            return Err(Error::Invalid);
        }
        for index in 0..3 {
            let address = self.word(instrument + 1 + index * 2, Role::Instrument)?;
            self.envelope(address, index == 0)?;
        }
        let mut channels = [Vec::new(), Vec::new()];
        let mut shapes = [0_u8; 2];
        for (song, channels) in channels.iter_mut().enumerate() {
            let row = self.header + 5 + song as u16 * 14;
            if self.word(row + 10, Role::Header)? != 307
                || self.word(row + 12, Role::Header)? != 256
            {
                return Err(Error::Invalid);
            }
            for index in 0..5 {
                let entry = row + index * 2;
                let address = self.word(entry, Role::Header)?;
                let max_note = match index {
                    3 => 16,
                    4 => 0,
                    _ => 63,
                };
                let (sequence, active) = self.channel(address, max_note)?;
                if active {
                    shapes[song] |= 1 << index;
                }
                channels.push(NesNativeChannel {
                    number: index as u8 + 1,
                    entry: prg_span(self.bytes, entry, 2).ok_or(Error::Invalid)?,
                    sequence,
                });
            }
        }
        if shapes[0] != shapes[1] || ![3, 15].contains(&shapes[0]) {
            return Err(Error::Invalid);
        }
        let mut spans: Vec<RomSpan> = Vec::new();
        for address in self.roles.keys().copied() {
            if let Some(last) = spans.last_mut()
                && last.canonical_cpu_address + last.byte_len == u32::from(address)
            {
                last.byte_len += 1;
                continue;
            }
            spans.push(prg_span(self.bytes, address, 1).ok_or(Error::Invalid)?);
        }
        Ok(Data {
            four_channels: shapes[0] == 15,
            header: prg_span(self.bytes, self.header, 33).ok_or(Error::Invalid)?,
            entries: [
                prg_span(self.bytes, self.header + 5, 14).ok_or(Error::Invalid)?,
                prg_span(self.bytes, self.header + 19, 14).ok_or(Error::Invalid)?,
            ],
            channels,
            spans,
        })
    }
}

pub(super) fn inspect(
    bytes: &[u8],
    header: u16,
    budget: &mut Budget<'_>,
) -> Result<Option<Data>, ScanStop> {
    budget.charge()?;
    if !(0x8000..=0xf800 - 33).contains(&header) {
        return Ok(None);
    }
    match (Parser {
        bytes,
        header,
        budget,
        roles: BTreeMap::new(),
    })
    .inspect()
    {
        Ok(data) => Ok(Some(data)),
        Err(Error::Invalid) => Ok(None),
        Err(Error::Stop(stop)) => Err(stop),
    }
}
