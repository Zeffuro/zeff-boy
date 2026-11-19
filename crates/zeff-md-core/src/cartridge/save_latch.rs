use super::*;
use anyhow::{Result, ensure};
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};

const ROM_BYTES: usize = 0x200000;
const SAVE_END: u32 = 0x210000;
const DECLARATION: &[u8; 12] = b"RA\xf8\x20\x00\x20\x00\x01\x00\x20\xff\xff";
const IDENTITY: [u8; 32] = [
    0xae, 0x28, 0x5c, 0x4c, 0xe2, 0xda, 0xb0, 0x0b, 0x5b, 0x9e, 0x42, 0x73, 0x1c, 0x98, 0x40, 0xda,
    0x23, 0x90, 0x14, 0x30, 0xf7, 0x18, 0x26, 0x85, 0xfa, 0xd1, 0xbd, 0xc6, 0x33, 0x78, 0x98, 0x5b,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SaveLatch {
    digest: [u8; 32],
}

impl SaveLatch {
    pub(crate) fn detect(
        rom: &[u8],
        header: Option<&CartridgeHeader>,
        digest: impl FnOnce() -> [u8; 32],
    ) -> Option<Self> {
        if !Self::geometry(rom) {
            return None;
        }
        #[cfg(test)]
        if rom.get(0x80..0x90) == Some(b"LATCHED_SAVE_TST") {
            return Self::from_media(rom, digest());
        }
        let header = header?;
        if header.checksum != 0xba1f || header.calculated_checksum != 0xba1f {
            return None;
        }
        Self::from_media(rom, digest())
    }

    fn geometry(rom: &[u8]) -> bool {
        rom.len() == ROM_BYTES
            && rom.get(0x1b0..0x1bc) == Some(DECLARATION)
            && rom.get(0x18e..0x190) == Some(&[0xba, 0x1f])
    }

    fn from_media(rom: &[u8], digest: [u8; 32]) -> Option<Self> {
        if !Self::geometry(rom) {
            return None;
        }
        if digest == IDENTITY {
            return Some(Self { digest });
        }
        #[cfg(test)]
        if rom.get(0x80..0x90) == Some(b"LATCHED_SAVE_TST") {
            return Some(Self { digest });
        }
        None
    }

    pub(crate) fn sensitive(address: u32) -> bool {
        (0x200000..0x400000).contains(&address) || (0xa13000..=0xa130ff).contains(&address)
    }

    pub(crate) fn save_address(address: u32) -> bool {
        (0x200000..SAVE_END).contains(&address)
    }

    fn accepts(access: BusAccess, direction: Direction, enabled: bool) -> bool {
        let logical = access.bus_address | u32::from(access.lanes == ByteLanes::Lower);
        if access.bus_address & 1 != 0
            || access.logical_address != logical
            || access.logical_address > BUS_ADDRESS_MASK
        {
            return false;
        }
        let data = access.kind == AccessKind::Data
            && matches!(
                access.function_code,
                FunctionCode::UserData | FunctionCode::SupervisorData
            );
        if (0xa13000..=0xa130ff).contains(&access.bus_address) {
            return direction == Direction::Write
                && access.bus_address == ram::SAVE_CONTROL
                && data;
        }
        if direction == Direction::Write || enabled && Self::save_address(access.bus_address) {
            return data;
        }
        data || matches!(
            access.kind,
            AccessKind::Opcode | AccessKind::Extension | AccessKind::Prefetch
        ) && matches!(
            access.function_code,
            FunctionCode::UserProgram | FunctionCode::SupervisorProgram
        )
    }
}

impl Cartridge {
    pub(crate) fn save_latch_compatible(&self) -> bool {
        self.save
            .as_ref()
            .is_some_and(SaveMemory::fixed_latched_odd32)
            && self.linear_rom.is_none()
            && self.unprotected_rom.is_none()
            && self.parallel_adapter.is_none()
            && self.registers.is_none()
            && self.protection.is_none()
            && self.sf001.is_none()
            && self.sf002.is_none()
            && self.sf004.is_none()
            && self.extended_ssf.is_none()
            && self.realtec.is_none()
            && self.ssf2.is_none()
            && self.lock_on.is_none()
            && self.lock_on_save.is_none()
    }

    pub(crate) fn validate_save_latch(&self) -> Result<()> {
        let expected = if SaveLatch::geometry(&self.bytes) {
            SaveLatch::from_media(&self.bytes, self.rom_digest())
        } else {
            None
        };
        ensure!(
            self.save_latch == expected,
            "MD save latch identity mismatch"
        );
        if self.save_latch.is_some() {
            ensure!(self.save_latch_compatible(), "MD save latch owner conflict");
            self.save
                .as_ref()
                .unwrap()
                .validate_state_with_fixed_latch()?;
        }
        Ok(())
    }

    pub(crate) fn save_latch_access(
        &self,
        access: BusAccess,
        direction: Direction,
    ) -> Option<bool> {
        self.save_latch?;
        if !SaveLatch::sensitive(access.bus_address & BUS_ADDRESS_MASK)
            && !SaveLatch::sensitive(access.logical_address & BUS_ADDRESS_MASK)
        {
            return None;
        }
        Some(self.save.as_ref().is_some_and(|save| {
            save.fixed_latched_odd32() && SaveLatch::accepts(access, direction, save.enabled)
        }))
    }

    pub(crate) fn write_save_latch(
        &mut self,
        access: BusAccess,
        value: u16,
        now: u64,
        master_hz: u64,
    ) -> Option<bool> {
        if !self.save_latch_access(access, Direction::Write)? {
            return Some(false);
        }
        let save = self.save.as_mut()?;
        if access.bus_address == ram::SAVE_CONTROL {
            let selected = if access.lanes == ByteLanes::Upper {
                (value >> 8) as u8
            } else {
                value as u8
            };
            save.enabled = selected & 1 != 0;
            save.write_protected = save.enabled && selected & 2 != 0;
        } else if SaveLatch::save_address(access.bus_address) {
            if access.lanes != ByteLanes::Lower {
                save.write_with_fixed_latch(access.bus_address, (value >> 8) as u8, now, master_hz);
            }
            if access.lanes != ByteLanes::Upper {
                save.write_with_fixed_latch(access.bus_address + 1, value as u8, now, master_hz);
            }
        }
        Some(true)
    }

    pub(crate) fn read_save_latch(&self, address: u32) -> Option<u8> {
        self.save_latch?;
        if address >= 0x400000 {
            return None;
        }
        let save = self.save.as_ref()?;
        if save.enabled && SaveLatch::save_address(address) {
            return save.read(address);
        }
        self.bytes.get(address as usize & (ROM_BYTES - 1)).copied()
    }

    pub(crate) fn save_latch_rom_offset(&self, address: u32) -> Option<usize> {
        self.save_latch?;
        if address >= 0x400000 {
            return None;
        }
        let save = self.save.as_ref()?;
        (!(save.enabled && SaveLatch::save_address(address)))
            .then_some(address as usize & (ROM_BYTES - 1))
    }
}

#[cfg(test)]
#[path = "save_latch/tests.rs"]
mod tests;
#[cfg(test)]
pub(crate) use tests::{contract, image as fixture_image};
