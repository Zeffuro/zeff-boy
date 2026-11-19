use super::*;
use crate::cartridge::registers::CartridgeRegisters;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RegisterWrites([Option<(u8, u8)>; 2]);

impl RegisterWrites {
    pub(super) fn push(&mut self, index: u8, value: u8) -> bool {
        if index >= 2 {
            return false;
        }
        let Some(slot) = self.0.iter_mut().find(|slot| slot.is_none()) else {
            return false;
        };
        *slot = Some((index, value));
        true
    }

    pub(super) fn publish(self, cartridge: &mut Cartridge) {
        for (index, value) in self.0.into_iter().flatten() {
            cartridge
                .registers
                .as_mut()
                .expect("validated register board")
                .write(index, value);
        }
    }

    pub(super) fn validate(
        self,
        cartridge: &Cartridge,
        any_bank_access: bool,
        access: Option<BankAccess>,
    ) -> Result<()> {
        let mut ended = false;
        for slot in self.0 {
            if let Some((index, _)) = slot {
                ensure!(
                    !ended && index < 2,
                    "invalid cartridge register journal index"
                );
                ensure!(
                    cartridge
                        .registers
                        .as_ref()
                        .is_some_and(|board| board.accepts_journal(index))
                        && any_bank_access,
                    "cartridge register journal board mismatch"
                );
            } else {
                ended = true;
            }
        }
        if let Some((index, value)) = self.0[0]
            && let Some(access) = access
        {
            ensure!(
                access.direction == Direction::Write
                    && CartridgeRegisters::index(access.address) == Some(index)
                    && access.value == value,
                "cartridge register journal access mismatch"
            );
        }
        Ok(())
    }

    pub(super) fn validate_access(self, access: BankAccess) -> Result<()> {
        if let Some(index) = CartridgeRegisters::index(access.address) {
            ensure!(
                self.0[0] == Some((index, access.value)),
                "missing cartridge register write journal"
            );
        }
        Ok(())
    }

    pub(super) fn write_state(self, writer: &mut StateWriter) {
        for slot in self.0 {
            writer.write_bool(slot.is_some());
            if let Some((index, value)) = slot {
                writer.write_u8(index);
                writer.write_u8(value);
            }
        }
    }

    pub(super) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let mut writes = Self::default();
        let mut ended = false;
        for slot in &mut writes.0 {
            if reader.read_bool()? {
                let index = reader.read_u8()?;
                ensure!(
                    !ended && index < 2,
                    "invalid cartridge register journal index"
                );
                *slot = Some((index, reader.read_u8()?));
            } else {
                ended = true;
            }
        }
        Ok(writes)
    }
}
