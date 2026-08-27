use crate::emu_backend::ColecoBackend;

use super::{
    AddressSpaceId, CpuLocation, DebugAccess, DebugAddressResolver, ExecMode, ImageId, RegionId,
    ResolvedDebugLocation, StorageLocation,
};

const COLECO_CPU_SPACE: AddressSpaceId = AddressSpaceId(0);
const COLECO_ROM_IMAGE: ImageId = ImageId(0);
const COLECO_ROM_REGION: RegionId = RegionId(0);

impl DebugAddressResolver for ColecoBackend {
    fn resolve_exec(&self, cpu: CpuLocation) -> ResolvedDebugLocation {
        let storage = u16::try_from(cpu.address)
            .ok()
            .and_then(|address| self.emu.rom_offset_for_cpu_address(address))
            .map(|offset| StorageLocation {
                image: COLECO_ROM_IMAGE,
                region: COLECO_ROM_REGION,
                offset: offset as u64,
            });
        ResolvedDebugLocation {
            cpu,
            storage,
            bank: None,
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

pub(crate) fn coleco_cpu_location(address: u16) -> CpuLocation {
    CpuLocation {
        space: COLECO_CPU_SPACE,
        address: u64::from(address),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn resolver_maps_the_fixed_cartridge_window() {
        let mut cartridge = vec![0; 8 * 1024];
        cartridge[..2].copy_from_slice(&[0xAA, 0x55]);
        let emu = zeff_coleco_core::Emulator::new(&cartridge, &[0; 8 * 1024], 48_000).unwrap();
        let backend = ColecoBackend::new(emu, PathBuf::from("test.col"), [0; 32]);
        let location = backend.resolve_exec(coleco_cpu_location(0x8123));

        assert_eq!(location.storage.unwrap().offset, 0x123);
        assert_eq!(location.exec_mode, ExecMode::Z80);
        assert_eq!(location.mapping_epoch, 0);
    }
}
