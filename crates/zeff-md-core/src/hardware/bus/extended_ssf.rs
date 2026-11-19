use super::*;
use crate::cartridge::extended_ssf::ExtendedSsf;
use zeff_m68k::bus::FunctionCode;

fn provenance(a: BusAccess) -> bool {
    a.bus_address & 1 == 0
        && a.function_code != FunctionCode::CpuSpace
        && a.kind != AccessKind::InterruptAcknowledge
        && a.logical_address & BUS_ADDRESS_MASK
            == a.bus_address + u32::from(a.lanes == ByteLanes::Lower)
}

impl Bus {
    pub(super) fn preflight_extended_ssf_write(&self, a: BusAccess) -> Result<(), BusError> {
        if self.cartridge.extended_ssf.is_none() || !ExtendedSsf::domain(a.bus_address) {
            return Ok(());
        }
        let rom = a.bus_address < 0x400000;
        let control = (0xa130f0..=0xa130fe).contains(&a.bus_address)
            && a.kind == AccessKind::Data
            && matches!(
                a.function_code,
                FunctionCode::UserData | FunctionCode::SupervisorData
            );
        if provenance(a) && (rom || control) {
            Ok(())
        } else {
            Err(BusError::UnsupportedAccess {
                address: a.bus_address,
                direction: Direction::Write,
            })
        }
    }
    pub(super) fn write_extended_ssf(
        &mut self,
        a: BusAccess,
        value: u16,
    ) -> Option<Result<(), BusError>> {
        if self.cartridge.extended_ssf.is_none() || !ExtendedSsf::domain(a.bus_address) {
            return None;
        }
        if let Err(error) = self.preflight_extended_ssf_write(a) {
            return Some(Err(error));
        }
        if a.bus_address >= 0xa130f0 {
            let selected = if a.lanes == ByteLanes::Upper {
                (value >> 8) as u8
            } else {
                value as u8
            };
            self.cartridge
                .extended_ssf
                .as_mut()
                .unwrap()
                .control(a.bus_address, selected);
        }
        Some(Ok(()))
    }
    pub(super) fn read_extended_ssf(
        &self,
        a: BusAccess,
    ) -> Option<Result<InstructionInput, BusError>> {
        let board = self.cartridge.extended_ssf.as_ref()?;
        if !ExtendedSsf::domain(a.bus_address) {
            return None;
        }
        let failure = || BusError::UnsupportedAccess {
            address: a.bus_address,
            direction: Direction::Read,
        };
        if !provenance(a) {
            return Some(Err(failure()));
        }
        let byte = |address| {
            board
                .source_offset(address)
                .and_then(|offset| self.cartridge.source_sample(offset))
                .ok_or_else(failure)
        };
        Some(instruction_input::lanes(a, byte))
    }
}
