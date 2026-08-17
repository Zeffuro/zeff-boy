use crate::emu_backend::Sega8Backend;

use super::{
    AddressSpaceId, CpuLocation, DebugAccess, DebugAddressResolver, ExecMode, ImageId, RegionId,
    ResolvedDebugLocation, StorageLocation,
};

const SEGA8_CPU_SPACE: AddressSpaceId = AddressSpaceId(0);
const SEGA8_ROM_IMAGE: ImageId = ImageId(0);
const SEGA8_ROM_REGION: RegionId = RegionId(0);

impl DebugAddressResolver for Sega8Backend {
    fn resolve_exec(&self, cpu: CpuLocation) -> ResolvedDebugLocation {
        let storage = u16::try_from(cpu.address)
            .ok()
            .and_then(|address| self.emu.rom_offset_for_cpu_address(address))
            .map(|offset| StorageLocation {
                image: SEGA8_ROM_IMAGE,
                region: SEGA8_ROM_REGION,
                offset: offset as u64,
            });
        ResolvedDebugLocation {
            cpu,
            storage,
            bank: storage.map(|location| (location.offset / 0x4000) as u32),
            exec_mode: ExecMode::Z80,
            mapping_epoch: self.mapping_epoch(),
        }
    }

    fn resolve_data(&self, cpu: CpuLocation, _access: DebugAccess) -> ResolvedDebugLocation {
        self.resolve_exec(cpu)
    }

    fn mapping_epoch(&self) -> u64 {
        self.emu.rom_mapping_token()
    }
}

pub(crate) fn sega8_cpu_location(address: u16) -> CpuLocation {
    CpuLocation {
        space: SEGA8_CPU_SPACE,
        address: u64::from(address),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use zeff_sega8_core::hardware::cartridge::SystemHint;

    use super::*;

    #[test]
    fn resolver_tracks_the_current_sega8_rom_bank() {
        let rom = vec![0; 4 * 0x4000];
        let emu = zeff_sega8_core::emulator::Emulator::new_with_hint(
            &rom,
            48_000,
            SystemHint::MasterSystem,
        )
        .unwrap();
        let mut backend = Sega8Backend::new(emu, PathBuf::from("test.sms"));
        let cpu = sega8_cpu_location(0x4000);

        let before = backend.resolve_exec(cpu);
        backend.emu.cpu_write8(0xFFFE, 3);
        let after = backend.resolve_exec(cpu);

        assert_eq!(before.storage.unwrap().offset, 0x4000);
        assert_eq!(after.storage.unwrap().offset, 0xC000);
        assert_ne!(before.mapping_epoch, after.mapping_epoch);
    }
}
