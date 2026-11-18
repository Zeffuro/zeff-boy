use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Sf002Writes([Option<(u32, u8)>; 2]);

fn access(address: u32) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & !1,
        lanes: if address & 1 == 0 {
            ByteLanes::Upper
        } else {
            ByteLanes::Lower
        },
        kind: AccessKind::Data,
        function_code: FunctionCode::SupervisorData,
    }
}

impl Sf002Writes {
    pub(super) fn push(&mut self, cartridge: &Cartridge, address: u32, value: u8) -> bool {
        let Some(index) = self.0.iter().position(Option::is_none) else {
            return false;
        };
        let mut next = *self;
        next.0[index] = Some((address, value));
        let mut projected = cartridge.clone();
        if next.project(&mut projected).is_err() {
            return false;
        }
        *self = next;
        true
    }

    pub(super) fn read(self, cartridge: &Cartridge, address: u32) -> Option<u8> {
        if self.is_empty() {
            return cartridge.read(address);
        }
        if address < 0x3c0000 {
            let mut controls = [(access(0), 0); 2];
            let mut count = 0;
            for (target, value) in self.0.into_iter().flatten() {
                if target < 0x3c0000 {
                    controls[count] = (access(target), u16::from(value));
                    count += 1;
                }
            }
            let offset = cartridge
                .sf002
                .as_ref()?
                .source_offset_after_writes(address, &controls[..count])
                .ok()??;
            return cartridge.source_byte(offset);
        }
        let mut projected = cartridge.clone();
        self.project(&mut projected).ok()?;
        projected.read(address)
    }

    pub(crate) fn project(
        self,
        cartridge: &mut Cartridge,
    ) -> std::result::Result<(), crate::hardware::bus::BusError> {
        for (address, value) in self.0.into_iter().flatten() {
            let value = if address & 1 == 0 {
                u16::from(value) << 8
            } else {
                u16::from(value)
            };
            if cartridge.write_sf002(access(address), value, 0, 1) != Some(true) {
                return Err(crate::hardware::bus::BusError::UnsupportedAccess {
                    address,
                    direction: Direction::Write,
                });
            }
        }
        Ok(())
    }

    pub(super) fn publish(self, cartridge: &mut Cartridge) {
        assert!(
            self.0[1].is_none(),
            "multiple SF-002 writes in one pending transfer"
        );
        self.project(cartridge).expect("preflighted SF-002 journal");
    }

    pub(super) fn validate(
        self,
        cartridge: &Cartridge,
        any_bank: bool,
        bank: Option<BankAccess>,
    ) -> Result<()> {
        if cartridge.sf002.is_none() && self.is_empty() {
            return Ok(());
        }
        ensure!(
            self.0[1].is_none(),
            "multiple SF-002 writes in one pending transfer"
        );
        let mut ended = false;
        for slot in self.0 {
            if let Some((address, value)) = slot {
                ensure!(
                    !ended && any_bank && cartridge.sf002.is_some(),
                    "SF-002 journal owner mismatch"
                );
                ensure!(
                    bank == Some(BankAccess {
                        address,
                        value,
                        direction: Direction::Write
                    }),
                    "SF-002 journal access mismatch"
                );
            } else {
                ended = true;
            }
        }
        if let Some(bank) = bank
            && cartridge.sf002.is_some()
            && bank.direction == Direction::Write
            && (bank.address <= CARTRIDGE_END
                || crate::cartridge::sf002::Sf002::rejects_time_write(bank.address))
        {
            ensure!(
                self.0[0] == Some((bank.address, bank.value)),
                "missing SF-002 journal"
            );
        }
        if self.is_empty() {
            return Ok(());
        }
        let mut projected = cartridge.clone();
        self.project(&mut projected)
            .map_err(|error| anyhow::anyhow!("invalid SF-002 journal: {error:?}"))
    }

    pub(crate) fn is_empty(self) -> bool {
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
        let mut result = Self::default();
        let mut ended = false;
        for slot in &mut result.0 {
            if reader.read_bool()? {
                let address = reader.read_u32()?;
                ensure!(
                    !ended && address <= BUS_ADDRESS_MASK,
                    "invalid SF-002 journal address"
                );
                *slot = Some((address, reader.read_u8()?));
            } else {
                ended = true;
            }
        }
        Ok(result)
    }
}
