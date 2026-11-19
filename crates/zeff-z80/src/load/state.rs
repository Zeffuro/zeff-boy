use super::*;
use crate::state::{Reader, Writer, bounded_byte};

impl Pair {
    fn write_continuation(self, w: &mut Writer) {
        match self {
            Self::Register(value) => {
                w.write_u8(0);
                w.write_u8(value);
            }
            Self::Index(value) => {
                w.write_u8(1);
                w.write_bool(value);
            }
        }
    }

    fn read_continuation(r: &mut Reader<'_>) -> anyhow::Result<Self> {
        match r.read_u8()? {
            0 => Ok(Self::Register(bounded_byte(r, 3)?)),
            1 => Ok(Self::Index(r.read_bool()?)),
            _ => anyhow::bail!("invalid Z80 load pair tag"),
        }
    }
}

impl Memory {
    fn write_continuation(self, w: &mut Writer) {
        match self {
            Self::ReadPair(pair) => {
                w.write_u8(0);
                pair.write_continuation(w);
            }
            Self::WritePair(value) => {
                w.write_u8(1);
                w.write_u16(value);
            }
            Self::ReadAccumulator => w.write_u8(2),
            Self::WriteAccumulator(value) => {
                w.write_u8(3);
                w.write_u8(value);
            }
        }
    }

    fn read_continuation(r: &mut Reader<'_>) -> anyhow::Result<Self> {
        match r.read_u8()? {
            0 => Ok(Self::ReadPair(Pair::read_continuation(r)?)),
            1 => Ok(Self::WritePair(r.read_u16()?)),
            2 => Ok(Self::ReadAccumulator),
            3 => Ok(Self::WriteAccumulator(r.read_u8()?)),
            _ => anyhow::bail!("invalid Z80 load memory tag"),
        }
    }
}

impl Load {
    pub(crate) fn write_continuation(self, w: &mut Writer) {
        self.fetch.write_continuation(w);
        match self.phase {
            Phase::ImmediateRegister(value) => {
                w.write_u8(0);
                w.write_u8(value);
            }
            Phase::ImmediateLow(pair) => {
                w.write_u8(1);
                pair.write_continuation(w);
            }
            Phase::ImmediateHigh(pair, low) => {
                w.write_u8(2);
                pair.write_continuation(w);
                w.write_u8(low);
            }
            Phase::AddressLow(memory) => {
                w.write_u8(3);
                memory.write_continuation(w);
            }
            Phase::AddressHigh(memory, low) => {
                w.write_u8(4);
                memory.write_continuation(w);
                w.write_u8(low);
            }
            Phase::Memory(memory, address) => {
                w.write_u8(5);
                memory.write_continuation(w);
                w.write_u16(address);
            }
            Phase::ReadHigh(pair, address, low) => {
                w.write_u8(6);
                pair.write_continuation(w);
                w.write_u16(address);
                w.write_u8(low);
            }
            Phase::WriteHigh(address, high) => {
                w.write_u8(7);
                w.write_u16(address);
                w.write_u8(high);
            }
            Phase::ImmediateByte(address) => {
                w.write_u8(8);
                w.write_u16(address);
            }
            Phase::WriteByte(address, value) => {
                w.write_u8(9);
                w.write_u16(address);
                w.write_u8(value);
            }
        }
    }

    pub(crate) fn read_continuation(r: &mut Reader<'_>) -> anyhow::Result<Self> {
        let fetch = access::Fetch::read_continuation(r)?;
        let phase = match r.read_u8()? {
            0 => {
                let register = bounded_byte(r, 7)?;
                anyhow::ensure!(
                    register != REGISTER_MEMORY_INDEX,
                    "invalid Z80 immediate register"
                );
                Phase::ImmediateRegister(register)
            }
            1 => Phase::ImmediateLow(Pair::read_continuation(r)?),
            2 => Phase::ImmediateHigh(Pair::read_continuation(r)?, r.read_u8()?),
            3 => Phase::AddressLow(Memory::read_continuation(r)?),
            4 => Phase::AddressHigh(Memory::read_continuation(r)?, r.read_u8()?),
            5 => Phase::Memory(Memory::read_continuation(r)?, r.read_u16()?),
            6 => Phase::ReadHigh(Pair::read_continuation(r)?, r.read_u16()?, r.read_u8()?),
            7 => Phase::WriteHigh(r.read_u16()?, r.read_u8()?),
            8 => Phase::ImmediateByte(r.read_u16()?),
            9 => Phase::WriteByte(r.read_u16()?, r.read_u8()?),
            _ => anyhow::bail!("invalid Z80 load phase tag"),
        };
        Ok(Self { phase, fetch })
    }
}
