use super::*;
use crate::state::{
    Reader, Writer, bounded_byte, read_option_u8, read_option_u16, write_option_u8,
    write_option_u16,
};

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
            Self::ProgramCounter => w.write_u8(2),
            Self::InterruptReturn => w.write_u8(3),
        }
    }

    fn read_continuation(r: &mut Reader<'_>) -> anyhow::Result<Self> {
        match r.read_u8()? {
            0 => Ok(Self::Register(bounded_byte(r, 3)?)),
            1 => Ok(Self::Index(r.read_bool()?)),
            2 => Ok(Self::ProgramCounter),
            3 => Ok(Self::InterruptReturn),
            _ => anyhow::bail!("invalid Z80 stack pair tag"),
        }
    }
}

fn read_condition(r: &mut Reader<'_>) -> anyhow::Result<Option<u8>> {
    let value = read_option_u8(r)?;
    anyhow::ensure!(
        value.is_none_or(|value| value < 8),
        "invalid Z80 stack condition"
    );
    Ok(value)
}

impl Stack {
    pub(crate) fn write_continuation(self, w: &mut Writer) {
        self.pair.write_continuation(w);
        w.write_u16(self.address);
        write_option_u16(self.target, w);
        self.fetch.write_continuation(w);
        match self.phase {
            Phase::JumpLow(condition) => {
                w.write_u8(0);
                write_option_u8(condition, w);
            }
            Phase::JumpHigh(condition, low) => {
                w.write_u8(1);
                write_option_u8(condition, w);
                w.write_u8(low);
            }
            Phase::CallLow(taken) => {
                w.write_u8(2);
                w.write_bool(taken);
            }
            Phase::CallHigh(taken, low) => {
                w.write_u8(3);
                w.write_bool(taken);
                w.write_u8(low);
            }
            Phase::ReturnPrepare(taken) => {
                w.write_u8(4);
                w.write_bool(taken);
            }
            Phase::PushPrepare(value) => {
                w.write_u8(5);
                w.write_u16(value);
            }
            Phase::PushHigh(value) => {
                w.write_u8(6);
                w.write_u16(value);
            }
            Phase::PushLow(value) => {
                w.write_u8(7);
                w.write_u8(value);
            }
            Phase::PopLow => w.write_u8(8),
            Phase::PopHigh(value) => {
                w.write_u8(9);
                w.write_u8(value);
            }
            Phase::ExchangeLow(value) => {
                w.write_u8(10);
                w.write_u16(value);
            }
            Phase::ExchangeHigh(value, low) => {
                w.write_u8(11);
                w.write_u16(value);
                w.write_u8(low);
            }
            Phase::ExchangeWriteHigh(value, result) => {
                w.write_u8(12);
                w.write_u16(value);
                w.write_u16(result);
            }
            Phase::ExchangeWriteLow(value, result) => {
                w.write_u8(13);
                w.write_u8(value);
                w.write_u16(result);
            }
        }
    }

    pub(crate) fn read_continuation(r: &mut Reader<'_>) -> anyhow::Result<Self> {
        let pair = Pair::read_continuation(r)?;
        let address = r.read_u16()?;
        let target = read_option_u16(r)?;
        let fetch = access::Fetch::read_continuation(r)?;
        let phase = match r.read_u8()? {
            0 => Phase::JumpLow(read_condition(r)?),
            1 => Phase::JumpHigh(read_condition(r)?, r.read_u8()?),
            2 => Phase::CallLow(r.read_bool()?),
            3 => Phase::CallHigh(r.read_bool()?, r.read_u8()?),
            4 => Phase::ReturnPrepare(r.read_bool()?),
            5 => Phase::PushPrepare(r.read_u16()?),
            6 => Phase::PushHigh(r.read_u16()?),
            7 => Phase::PushLow(r.read_u8()?),
            8 => Phase::PopLow,
            9 => Phase::PopHigh(r.read_u8()?),
            10 => Phase::ExchangeLow(r.read_u16()?),
            11 => Phase::ExchangeHigh(r.read_u16()?, r.read_u8()?),
            12 => Phase::ExchangeWriteHigh(r.read_u16()?, r.read_u16()?),
            13 => Phase::ExchangeWriteLow(r.read_u8()?, r.read_u16()?),
            _ => anyhow::bail!("invalid Z80 stack phase tag"),
        };
        Ok(Self {
            pair,
            address,
            target,
            phase,
            fetch,
        })
    }
}
