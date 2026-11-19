use zeff_emu_common::save_state::{StateReader, StateWriter};

use super::{ADMITTED_ST, Core, FLAGS, Fault};

const A_KNOWN: &[u8; 4] = b"AKN1";

impl Core {
    pub(crate) fn validate(&self) -> Result<(), Fault> {
        if self.st & !ADMITTED_ST != 0 {
            return Err(Fault::StatusMode(self.st));
        }
        if self.flags_known & !FLAGS != 0 {
            return Err(Fault::InvalidState("flag knowledge mask"));
        }
        if self.st & FLAGS & !self.flags_known != 0 {
            return Err(Fault::InvalidState("unknown flag value"));
        }
        if self.a & !self.a_known != 0 {
            return Err(Fault::InvalidState("unknown accumulator value"));
        }
        if self.p != Self::product(self.x, self.y) {
            return Err(Fault::InvalidState("product disagrees with operands"));
        }
        if self.stack_len > 6 {
            return Err(Fault::InvalidState("stack depth"));
        }
        if self.pointers[3] != 0 || self.pointers[7] != 0 {
            return Err(Fault::InvalidState("special pointer storage"));
        }
        Ok(())
    }

    pub(crate) fn write_state(&self, writer: &mut StateWriter) -> Result<(), Fault> {
        self.validate()?;
        writer.write_u16(self.x);
        writer.write_u16(self.y);
        writer.write_u32(self.a);
        writer.write_u32(self.p);
        writer.write_u16(self.pc);
        writer.write_u16(self.st);
        writer.write_u16(self.flags_known);
        writer.write_u8(self.stack_len);
        for value in self.stack {
            writer.write_u16(value);
        }
        for value in self.pointers {
            writer.write_u8(value);
        }
        for bank in self.ram {
            for value in bank {
                writer.write_u16(value);
            }
        }
        if self.a_known != u32::MAX {
            writer.write_bytes(A_KNOWN);
            writer.write_u32(self.a_known);
        }
        Ok(())
    }

    pub(crate) fn read_state(reader: &mut StateReader<'_>) -> anyhow::Result<Self> {
        let mut state = Self::synthetic(0);
        state.x = reader.read_u16()?;
        state.y = reader.read_u16()?;
        state.a = reader.read_u32()?;
        state.p = reader.read_u32()?;
        state.pc = reader.read_u16()?;
        state.st = reader.read_u16()?;
        state.flags_known = reader.read_u16()?;
        state.stack_len = reader.read_u8()?;
        for value in &mut state.stack {
            *value = reader.read_u16()?;
        }
        for value in &mut state.pointers {
            *value = reader.read_u8()?;
        }
        for bank in &mut state.ram {
            for value in bank {
                *value = reader.read_u16()?;
            }
        }
        if !reader.is_exhausted() {
            let position = reader.position();
            match reader.read_u8()? {
                0..=2 => reader.set_position(position),
                b'A' => {
                    let mut rest = [0; 3];
                    reader.read_exact(&mut rest)?;
                    anyhow::ensure!(rest == A_KNOWN[1..], "unknown accumulator extension");
                    state.a_known = reader.read_u32()?;
                    anyhow::ensure!(state.a_known != u32::MAX, "redundant accumulator extension");
                }
                _ => anyhow::bail!("unknown CPU extension"),
            }
        }
        state.validate()?;
        Ok(state)
    }
}
