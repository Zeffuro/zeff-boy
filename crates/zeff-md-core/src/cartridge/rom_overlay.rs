use super::Cartridge;
use anyhow::{Result, ensure};
use std::collections::BTreeMap;
use std::sync::Arc;
use zeff_emu_common::cheats::CheatPatch;
use zeff_m68k::Registers;
use zeff_m68k::bus::BUS_ADDRESS_MASK;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RomOverlay {
    bytes: Arc<[(u32, u8)]>,
    initial: Option<Registers>,
}

impl RomOverlay {
    pub(super) fn write_rollback_metadata(
        &self,
        writer: &mut zeff_emu_common::save_state::StateWriter,
    ) {
        writer.write_bool(self.initial.is_some());
        if let Some(initial) = &self.initial {
            super::rollback::write_registers(initial, writer);
        }
    }

    pub(super) fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut crate::emulator::rollback::MdRetainedAllocations<'a>,
    ) -> anyhow::Result<()> {
        let Self { bytes, initial: _ } = self;
        into.arc(
            bytes,
            crate::emulator::rollback::MdAllocationKind::RomOverlay,
        )?;
        Ok(())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub(crate) fn read(&self, offset: usize, source: u8) -> u8 {
        self.bytes
            .binary_search_by_key(&(offset as u32), |&(address, _)| address)
            .map_or(source, |index| self.bytes[index].1)
    }
}

impl Cartridge {
    pub(crate) fn supports_rom_overlay(&self) -> bool {
        self.rom_overlay_size_compatible()
            && self.svp_overlay_compatible()
            && self.special_save_overlay_compatible()
            && self.validate_save_latch().is_ok()
            && self.download_rom_overlay_compatible()
            && self.linear_rom.is_none_or(|_| self.linear_rom_compatible())
            && self
                .unprotected_rom
                .is_none_or(|_| self.unprotected_rom_compatible())
            && self.parallel_rom_overlay_compatible()
            && self.fixed_register_rom_compatible()
            && self.lock_on_overlay_compatible()
    }

    fn svp_overlay_compatible(&self) -> bool {
        self.svp.as_ref().is_none_or(|model| {
            model.shared_rom_history_owner()
                && Arc::ptr_eq(&self.bytes, &self.digest_memo.seal)
                && self.bytes.len() <= 0x200000
                && self.bytes.len().is_multiple_of(2)
        })
    }

    fn download_rom_overlay_compatible(&self) -> bool {
        self.validate_download().is_ok()
            && (self.download.is_none() || Arc::ptr_eq(&self.bytes, &self.digest_memo.seal))
    }

    pub(crate) fn tmss_rom_overlay_compatible(&self) -> bool {
        Arc::ptr_eq(&self.bytes, &self.digest_memo.seal)
            && self.bytes.len() <= 0x400000
            && self.digest_memo.factory_save.is_none()
            && self.digest_memo.factory_lock_on.is_none()
            && self.digest_memo.factory_registers.is_none()
            && self.digest_memo.factory_protection.is_none()
            && self.svp.is_none()
            && self.save.is_none()
            && self.save_latch.is_none()
            && self.download.is_none()
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

    fn rom_overlay_size_compatible(&self) -> bool {
        if self.bytes.len() <= 0x800000 {
            return self.bytes.len() <= 0x400000
                || self.ssf2.is_some()
                || self.extended_ssf.is_some();
        }
        Arc::ptr_eq(&self.bytes, &self.digest_memo.seal)
            && self.bytes.len() <= crate::constants::MAX_BANKED_CARTRIDGE_SIZE
            && self
                .extended_ssf
                .as_ref()
                .is_some_and(|board| board.matches_rom(&self.bytes))
            && self.svp.is_none()
            && self.save.is_none()
            && self.save_latch.is_none()
            && self.download.is_none()
            && self.linear_rom.is_none()
            && self.unprotected_rom.is_none()
            && self.parallel_adapter.is_none()
            && self.registers.is_none()
            && self.protection.is_none()
            && self.sf001.is_none()
            && self.sf002.is_none()
            && self.sf004.is_none()
            && self.realtec.is_none()
            && self.ssf2.is_none()
            && self.lock_on.is_none()
            && self.lock_on_save.is_none()
    }

    fn fixed_register_rom_compatible(&self) -> bool {
        if self.digest_memo.factory_registers.is_none()
            && self.digest_memo.factory_protection.is_none()
            && self.registers.is_none()
            && self.protection.is_none()
        {
            return true;
        }
        Arc::ptr_eq(&self.bytes, &self.digest_memo.seal)
            && self.protection == self.digest_memo.factory_protection
            && match (&self.registers, &self.digest_memo.factory_registers) {
                (Some(board), Some(expected)) => board.validate_profile(expected).is_ok(),
                (None, None) => true,
                _ => false,
            }
            && matches!(self.bytes.len(), 0x100000 | 0x200000)
            && (self.registers.is_some() ^ self.protection.is_some())
            && self.save.is_none()
            && self.save_latch.is_none()
            && self.download.is_none()
            && self.linear_rom.is_none()
            && self.unprotected_rom.is_none()
            && self.parallel_adapter.is_none()
            && self.sf001.is_none()
            && self.sf002.is_none()
            && self.sf004.is_none()
            && self.extended_ssf.is_none()
            && self.realtec.is_none()
            && self.ssf2.is_none()
            && self.lock_on.is_none()
            && self.lock_on_save.is_none()
    }

    fn lock_on_overlay_compatible(&self) -> bool {
        use super::registers::LockOn;
        if self.digest_memo.factory_lock_on != Some(LockOn::TwoMiB)
            && self.lock_on != Some(LockOn::TwoMiB)
            && self.lock_on_save.is_none()
        {
            return self.lock_on.is_none_or(LockOn::supports_rom_overlay);
        }
        self.digest_memo.factory_lock_on == Some(LockOn::TwoMiB)
            && Arc::ptr_eq(&self.bytes, &self.digest_memo.seal)
            && self.bytes.len() == 0x400000
            && self.lock_on == Some(LockOn::TwoMiB)
            && self.lock_on_save.is_some()
            && self.save.is_none()
            && self.save_latch.is_none()
            && self.download.is_none()
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
    }

    fn special_save_overlay_compatible(&self) -> bool {
        let current = self
            .save
            .as_ref()
            .and_then(super::SaveMemory::special_rom_overlay_layout);
        if self.digest_memo.factory_save.is_none() && current.is_none() {
            return true;
        }
        self.digest_memo.factory_save.is_some()
            && current == self.digest_memo.factory_save
            && Arc::ptr_eq(&self.bytes, &self.digest_memo.seal)
            && self
                .save
                .as_ref()
                .is_some_and(|save| save.validate_state().is_ok())
            && self.save_latch.is_none()
            && self.download.is_none()
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

    fn parallel_rom_overlay_compatible(&self) -> bool {
        if self.parallel_adapter.is_none()
            && !super::parallel_adapter::ParallelAdapter::identity_geometry(&self.digest_memo.seal)
            && !super::parallel_adapter::ParallelAdapter::identity_geometry(&self.bytes)
        {
            return true;
        }
        Arc::ptr_eq(&self.bytes, &self.digest_memo.seal)
            && self.validate_parallel_adapter().is_ok()
            && self
                .parallel_adapter
                .as_ref()
                .is_none_or(|_| self.realtec.is_none() && self.extended_ssf.is_none())
    }

    pub(crate) fn source_byte(&self, offset: usize) -> Option<u8> {
        self.bytes
            .get(offset)
            .map(|&source| self.rom_overlay.read(offset, source))
    }

    pub(crate) fn effective_initial(&self) -> Registers {
        self.rom_overlay.initial.unwrap_or(self.initial)
    }

    pub(crate) fn prepare_rom_overlay(&self, patches: &[CheatPatch]) -> Result<RomOverlay> {
        let mut bytes = BTreeMap::new();
        for patch in patches {
            let (address, value, compare) = match *patch {
                CheatPatch::RomWrite { address, value } => (u32::from(address), value, None),
                CheatPatch::RomWriteIfEquals {
                    address,
                    value,
                    compare,
                } => (u32::from(address), value, Some(compare)),
                CheatPatch::WideRomWrite { address, value } => (address, value, None),
                CheatPatch::WideRomWriteIfEquals {
                    address,
                    value,
                    compare,
                } => (address, value, Some(compare)),
                _ => continue,
            };
            ensure!(
                self.supports_rom_overlay(),
                "ROM cheats require a qualified cartridge read mask"
            );
            let source = self.bytes.get(address as usize).copied().ok_or_else(|| {
                anyhow::anyhow!("ROM cheat offset {address:06X} is outside the source image")
            })?;
            if compare.is_none_or(|compare| compare.matches(source)) {
                bytes.insert(address, value.resolve_with_current(source));
            }
        }
        if self.download.is_none() {
            bytes.retain(|&address, value| self.bytes[address as usize] != *value);
        }
        let mut overlay = RomOverlay {
            bytes: bytes.into_iter().collect::<Vec<_>>().into(),
            initial: None,
        };
        if !overlay.is_empty() {
            let vectors = self.realtec.as_ref().map_or(0, |_| {
                super::realtec::Realtec::new().source_offset(0).unwrap()
            });
            let vector = |offset| {
                u32::from_be_bytes(std::array::from_fn(|index| {
                    let source = vectors + offset + index;
                    overlay.read(source, self.bytes[source])
                }))
            };
            let pc = vector(4);
            let cold_rom_end = if self.sf002.is_some() {
                0x200000
            } else {
                0x400000
            };
            let valid_pc = if self.realtec.is_some() {
                super::realtec::Realtec::cold_prefetch_valid(pc)
            } else if let Some(mapping) = self.lock_on {
                mapping.cold_prefetch_valid(pc, self.bytes.len())
            } else if let Some(board) = &self.parallel_adapter {
                board.cold_prefetch_valid(pc, self.bytes.len())
            } else {
                pc & 1 == 0
                    && (pc & BUS_ADDRESS_MASK) as usize + 3 < self.bytes.len().min(cold_rom_end)
            };
            ensure!(valid_pc, "ROM cheats produce an invalid reset PC");
            let mut initial = self.initial;
            initial.ssp = vector(0);
            initial.pc = pc;
            overlay.initial = Some(initial);
        }
        Ok(overlay)
    }
}
