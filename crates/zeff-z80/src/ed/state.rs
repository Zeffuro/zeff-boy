use super::*;
use crate::state::{Reader, Writer, bounded_byte, read_option_u8, write_option_u8};

impl Io {
    pub(crate) fn write_continuation(self, w: &mut Writer) {
        w.write_u8(self.port);
        w.write_u8(self.register);
        write_option_u8(self.output, w);
    }

    pub(crate) fn read_continuation(r: &mut Reader<'_>) -> anyhow::Result<Self> {
        Ok(Self {
            port: r.read_u8()?,
            register: bounded_byte(r, 7)?,
            output: read_option_u8(r)?,
        })
    }
}

impl Internal {
    pub(crate) fn write_continuation(self, w: &mut Writer) {
        w.write_u8(self.opcode);
        w.write_bool(self.final_phase);
    }

    pub(crate) fn read_continuation(r: &mut Reader<'_>) -> anyhow::Result<Self> {
        let opcode = r.read_u8()?;
        let final_phase = r.read_bool()?;
        let initial =
            Self::decode(opcode).ok_or_else(|| anyhow::anyhow!("invalid Z80 internal opcode"))?;
        anyhow::ensure!(
            !initial.final_phase || final_phase,
            "invalid Z80 internal phase"
        );
        Ok(Self {
            opcode,
            final_phase,
        })
    }
}

impl Block {
    pub(crate) fn write_continuation(self, w: &mut Writer) {
        w.write_u8(self.opcode);
        w.write_u16(self.pc);
        w.write_u16(self.address);
        w.write_u16(self.destination);
        w.write_u8(self.port);
        match self.phase {
            BlockPhase::Lead => w.write_u8(0),
            BlockPhase::Read => w.write_u8(1),
            BlockPhase::Write(value) => {
                w.write_u8(2);
                w.write_u8(value);
            }
            BlockPhase::Finish(value) => {
                w.write_u8(3);
                w.write_u8(value);
            }
            BlockPhase::Repeat(value) => {
                w.write_u8(4);
                w.write_u8(value);
            }
        }
    }

    pub(crate) fn read_continuation(r: &mut Reader<'_>) -> anyhow::Result<Self> {
        let opcode = r.read_u8()?;
        anyhow::ensure!(opcode & 0xe4 == 0xa0, "invalid Z80 block opcode");
        let pc = r.read_u16()?;
        let address = r.read_u16()?;
        let destination = r.read_u16()?;
        let port = r.read_u8()?;
        let phase = match r.read_u8()? {
            0 => {
                anyhow::ensure!(opcode & 2 != 0, "invalid Z80 block lead phase");
                BlockPhase::Lead
            }
            1 => BlockPhase::Read,
            2 => {
                anyhow::ensure!(opcode & 3 != 1, "invalid Z80 block write phase");
                BlockPhase::Write(r.read_u8()?)
            }
            3 => {
                anyhow::ensure!(opcode & 2 == 0, "invalid Z80 block finish phase");
                BlockPhase::Finish(r.read_u8()?)
            }
            4 => {
                anyhow::ensure!(opcode & 0x10 != 0, "invalid Z80 block repeat phase");
                BlockPhase::Repeat(r.read_u8()?)
            }
            _ => anyhow::bail!("invalid Z80 block phase tag"),
        };
        Ok(Self {
            opcode,
            pc,
            address,
            destination,
            port,
            phase,
        })
    }
}
