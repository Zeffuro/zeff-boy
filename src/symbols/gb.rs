use crate::emu_backend::GbBackend;

use super::{
    AddressSpaceId, CpuLocation, DebugAccess, DebugAddressResolver, ExecMode, ImageId, RegionId,
    ResolvedDebugLocation, StorageLocation,
};

const GB_CPU_SPACE: AddressSpaceId = AddressSpaceId(0);
const GB_ROM_IMAGE: ImageId = ImageId(0);
const GB_ROM_REGION: RegionId = RegionId(0);

impl DebugAddressResolver for GbBackend {
    fn resolve_exec(&self, cpu: CpuLocation) -> ResolvedDebugLocation {
        let storage = u16::try_from(cpu.address)
            .ok()
            .and_then(|address| self.emu.rom_offset_for_cpu_address(address))
            .map(|offset| StorageLocation {
                image: GB_ROM_IMAGE,
                region: GB_ROM_REGION,
                offset: offset as u64,
            });
        ResolvedDebugLocation {
            cpu,
            storage,
            bank: storage.map(|location| (location.offset / 0x4000) as u32),
            exec_mode: ExecMode::Sm83,
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

pub(crate) fn gb_cpu_location(address: u16) -> CpuLocation {
    CpuLocation {
        space: GB_CPU_SPACE,
        address: u64::from(address),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

    use super::*;

    #[test]
    fn resolver_tracks_the_current_gb_rom_bank() {
        let mut rom = vec![0; 4 * 0x4000];
        rom[0x147] = 0x19;
        rom[0x148] = 0x01;
        let emu =
            zeff_gb_core::emulator::Emulator::from_rom_data(&rom, HardwareModePreference::Auto)
                .unwrap();
        let mut backend = GbBackend::new(emu, PathBuf::from("test.gb"));
        let cpu = gb_cpu_location(0x4560);

        let before = backend.resolve_exec(cpu);
        backend.emu.cpu_write8(0x2000, 2);
        let after = backend.resolve_exec(cpu);

        assert_eq!(before.storage.unwrap().offset, 0x4560);
        assert_eq!(after.storage.unwrap().offset, 0x8560);
        assert_ne!(before.mapping_epoch, after.mapping_epoch);
    }
}
