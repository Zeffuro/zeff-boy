use super::*;
use crate::state::{Reader, Writer};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MemptrProfile {
    #[default]
    Legacy,
    Nmos,
}

impl Cpu {
    pub fn memptr(&self) -> Option<u16> {
        self.memptr
    }

    pub fn memptr_profile(&self) -> MemptrProfile {
        self.memptr_profile
    }

    pub fn write_state_with_memptr(&self, writer: &mut Writer) {
        self.write_state(writer);
        self.write_memptr(writer);
    }

    pub fn read_state_with_memptr(&mut self, reader: &mut Reader<'_>) -> anyhow::Result<()> {
        let mut candidate = self.clone();
        candidate.read_legacy_state_fields(reader)?;
        candidate.read_memptr(reader)?;
        *self = candidate;
        Ok(())
    }

    pub(super) fn write_memptr(&self, writer: &mut Writer) {
        writer.write_u8(match self.memptr_profile {
            MemptrProfile::Legacy => 0,
            MemptrProfile::Nmos => 1,
        });
        writer.write_bool(self.memptr.is_some());
        writer.write_u16(self.memptr.unwrap_or(0));
    }

    pub(super) fn read_memptr(&mut self, reader: &mut Reader<'_>) -> anyhow::Result<()> {
        self.memptr_profile = match reader.read_u8()? {
            0 => MemptrProfile::Legacy,
            1 => MemptrProfile::Nmos,
            _ => anyhow::bail!("invalid Z80 MEMPTR profile"),
        };
        let known = reader.read_bool()?;
        let value = reader.read_u16()?;
        anyhow::ensure!(known || value == 0, "noncanonical unknown Z80 MEMPTR");
        anyhow::ensure!(
            self.memptr_profile != MemptrProfile::Legacy || !known,
            "history in legacy Z80 profile"
        );
        self.memptr = known.then_some(value);
        Ok(())
    }

    pub(super) fn set_memptr(&mut self, value: u16) {
        if self.memptr_profile == MemptrProfile::Nmos {
            self.memptr = Some(value);
        }
    }

    pub(super) fn memptr_accumulator_store(&mut self, address: u16, accumulator: u8) {
        self.set_memptr(u16::from_be_bytes([
            accumulator,
            (address as u8).wrapping_add(1),
        ]));
    }

    pub(super) fn memptr_relative(&mut self, delta: i16) {
        self.memptr = self.memptr.map(|value| value.wrapping_add_signed(delta));
    }

    pub(super) fn memptr_bit_xy(&mut self, opcode: u8) -> Option<u8> {
        if let Some(value) = self.memptr {
            return Some((value >> 8) as u8 & (Z80_FLAG_BIT_5 | Z80_FLAG_BIT_3));
        }
        self.state = CpuState::Suspended;
        self.trap = Some(CpuTrap::UnsupportedPrefixedOpcode {
            pc: self.last_opcode_pc,
            prefix: Z80_PREFIX_CB,
            opcode,
        });
        None
    }
}
