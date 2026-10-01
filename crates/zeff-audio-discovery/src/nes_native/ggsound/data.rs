use std::collections::BTreeMap;

use super::super::{NesNativeChannel, prg_span};
use crate::{Budget, RomSpan, ScanStop};

#[cfg(test)]
mod tests;

pub(super) struct Data {
    pub table: RomSpan,
    pub headers: [RomSpan; 2],
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
    SongTable,
    InstrumentTable,
    Header,
    Instrument,
    EnvelopeValue,
    EnvelopeStop,
    ChannelCommand,
    ChannelOperand,
}

struct Parser<'a, 'b, 'c> {
    bytes: &'a [u8],
    root: u16,
    budget: &'b mut Budget<'c>,
    roles: BTreeMap<u16, Role>,
}

impl Parser<'_, '_, '_> {
    fn byte(&mut self, address: u16, role: Role) -> Parsed<u8> {
        self.budget.charge().map_err(Error::Stop)?;
        if address < self.root || address >= 0xc000 || address >> 8 != self.root >> 8 {
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

    fn instrument(&mut self, table: u16) -> Parsed<()> {
        let instrument = self.word(table, Role::InstrumentTable)?;
        for (index, offset) in [3, 5, 7].into_iter().enumerate() {
            if self.byte(
                instrument.checked_add(index as u16).ok_or(Error::Invalid)?,
                Role::Instrument,
            )? != offset
            {
                return Err(Error::Invalid);
            }
            let address = instrument
                .checked_add(u16::from(offset))
                .ok_or(Error::Invalid)?;
            let value = self.byte(address, Role::EnvelopeValue)?;
            let valid = match index {
                0 => (1..=15).contains(&value),
                1 => value == 0,
                _ => value == 0x80,
            };
            let stop = if index == 2 { 0x3f } else { 0x80 };
            if !valid || self.byte(address + 1, Role::EnvelopeStop)? != stop {
                return Err(Error::Invalid);
            }
        }
        Ok(())
    }

    fn channel(&mut self, master: u16) -> Parsed<RomSpan> {
        if self.byte(master, Role::ChannelCommand)? != 0x74 {
            return Err(Error::Invalid);
        }
        let mut pc = self.word(master + 1, Role::ChannelOperand)?;
        if self.byte(master + 3, Role::ChannelCommand)? != 0x73
            || self.word(master + 4, Role::ChannelOperand)? != master
        {
            return Err(Error::Invalid);
        }
        let mut instrument_selected = false;
        let mut length_set = false;
        let mut has_note = false;
        let start = pc;
        while pc.checked_sub(start).is_some_and(|length| length < 128) {
            let opcode = self.byte(pc, Role::ChannelCommand)?;
            pc = pc.checked_add(1).ok_or(Error::Invalid)?;
            match opcode {
                0..=0x5f if instrument_selected && length_set => has_note = true,
                0x60..=0x6f => length_set = true,
                0x72 => {
                    if pc - start >= 128 || self.byte(pc, Role::ChannelOperand)? != 0 {
                        return Err(Error::Invalid);
                    }
                    pc += 1;
                    instrument_selected = true;
                }
                0x75 if has_note => {
                    return prg_span(self.bytes, master, 6).ok_or(Error::Invalid);
                }
                _ => return Err(Error::Invalid),
            }
        }
        Err(Error::Invalid)
    }

    fn inspect(mut self, instruments: u16) -> Parsed<Data> {
        let headers = [
            self.word(self.root, Role::SongTable)?,
            self.word(self.root + 2, Role::SongTable)?,
        ];
        self.instrument(instruments)?;
        let mut channels = [Vec::new(), Vec::new()];
        for (song, channels) in channels.iter_mut().enumerate() {
            let header = headers[song];
            if self.word(header, Role::Header)? != 0x600
                || self.word(header + 2, Role::Header)? != 0x500
                || self.word(header + 8, Role::Header)? != 0
                || self.word(header + 10, Role::Header)? != 0
            {
                return Err(Error::Invalid);
            }
            for index in 0..2 {
                let entry = header + 4 + index * 2;
                let address = self.word(entry, Role::Header)?;
                let sequence = self.channel(address)?;
                channels.push(NesNativeChannel {
                    number: index as u8 + 1,
                    entry: prg_span(self.bytes, entry, 2).ok_or(Error::Invalid)?,
                    sequence,
                });
            }
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
            table: prg_span(self.bytes, self.root, 4).ok_or(Error::Invalid)?,
            headers: [
                prg_span(self.bytes, headers[0], 12).ok_or(Error::Invalid)?,
                prg_span(self.bytes, headers[1], 12).ok_or(Error::Invalid)?,
            ],
            entries: [
                prg_span(self.bytes, self.root, 2).ok_or(Error::Invalid)?,
                prg_span(self.bytes, self.root + 2, 2).ok_or(Error::Invalid)?,
            ],
            channels,
            spans,
        })
    }
}

pub(super) fn inspect(
    bytes: &[u8],
    root: u16,
    instruments: u16,
    budget: &mut Budget<'_>,
) -> Result<Option<Data>, ScanStop> {
    budget.charge()?;
    if !(0x8000..=0xbffc).contains(&root) {
        return Ok(None);
    }
    match (Parser {
        bytes,
        root,
        budget,
        roles: BTreeMap::new(),
    })
    .inspect(instruments)
    {
        Ok(data) => Ok(Some(data)),
        Err(Error::Invalid) => Ok(None),
        Err(Error::Stop(stop)) => Err(stop),
    }
}
