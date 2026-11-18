use super::*;
use crate::cartridge::sf001::Location;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Sf001Writes([Option<(u32, u8)>; 2]);

impl Sf001Writes {
    pub(super) fn accepts(cartridge: &Cartridge, address: u32) -> bool {
        cartridge.sf001.as_ref().is_some_and(|board| {
            !matches!(board.decode(address), Location::Unclaimed)
                || (0xa130f0..=0xa130ff).contains(&address)
        })
    }

    pub(super) fn push(&mut self, address: u32, value: u8) -> bool {
        let Some(slot) = self.0.iter_mut().find(|slot| slot.is_none()) else {
            return false;
        };
        *slot = Some((address, value));
        true
    }

    pub(super) fn read(self, cartridge: &Cartridge, address: u32) -> u8 {
        let board = cartridge.sf001.as_ref().expect("SF read board");
        if self == Self::default() {
            return board
                .read(address, |offset| cartridge.source_byte(offset))
                .unwrap_or(SOUND_DISCONNECTED_BYTE);
        }
        let mut writes = [(0, 0); 2];
        let mut count = 0;
        for entry in self.0.into_iter().flatten() {
            writes[count] = entry;
            count += 1;
        }
        board
            .read_after_writes(
                address,
                |offset| cartridge.source_byte(offset),
                &writes[..count],
            )
            .unwrap_or(SOUND_DISCONNECTED_BYTE)
    }

    pub(super) fn publish(self, cartridge: &mut Cartridge) {
        for (address, value) in self.0.into_iter().flatten() {
            cartridge
                .sf001
                .as_mut()
                .expect("validated SF journal board")
                .write(address, value);
        }
    }

    pub(super) fn validate(
        self,
        cartridge: &Cartridge,
        any_bank: bool,
        access: Option<BankAccess>,
    ) -> Result<()> {
        let mut ended = false;
        for slot in self.0 {
            if let Some((address, _)) = slot {
                ensure!(
                    !ended && any_bank && Self::accepts(cartridge, address),
                    "SF journal board/address mismatch"
                );
            } else {
                ended = true;
            }
        }
        if let Some((address, value)) = self.0[0]
            && let Some(access) = access
        {
            ensure!(
                access
                    == BankAccess {
                        address,
                        value,
                        direction: Direction::Write
                    },
                "SF journal access mismatch"
            );
        }
        if let Some(access) = access
            && access.direction == Direction::Write
            && Self::accepts(cartridge, access.address)
        {
            ensure!(
                self.0[0] == Some((access.address, access.value)),
                "missing SF write journal"
            );
        }
        Ok(())
    }

    pub(super) fn is_empty(self) -> bool {
        self == Self::default()
    }

    pub(super) fn write_state(self, writer: &mut StateWriter) {
        for slot in self.0 {
            writer.write_bool(slot.is_some());
            if let Some((address, value)) = slot {
                writer.write_u32(address);
                writer.write_u8(value);
            }
        }
    }

    pub(super) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let mut journal = Self::default();
        let mut ended = false;
        for slot in &mut journal.0 {
            if reader.read_bool()? {
                let address = reader.read_u32()?;
                ensure!(
                    !ended && address <= BUS_ADDRESS_MASK,
                    "invalid SF journal address"
                );
                *slot = Some((address, reader.read_u8()?));
            } else {
                ended = true;
            }
        }
        Ok(journal)
    }
}
