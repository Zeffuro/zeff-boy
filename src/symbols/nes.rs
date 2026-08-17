use crate::emu_backend::NesBackend;

use super::{
    AddressSpaceId, CpuLocation, DebugAccess, DebugAddressResolver, ExecMode, ImageId, RegionId,
    ResolvedDebugLocation, StorageLocation,
};

const NES_CPU_SPACE: AddressSpaceId = AddressSpaceId(0);
const NES_ROM_IMAGE: ImageId = ImageId(0);
const NES_ROM_REGION: RegionId = RegionId(0);

impl DebugAddressResolver for NesBackend {
    fn resolve_exec(&self, cpu: CpuLocation) -> ResolvedDebugLocation {
        let storage = u16::try_from(cpu.address)
            .ok()
            .and_then(|address| self.emu.rom_offset_for_cpu_address(address))
            .map(|offset| StorageLocation {
                image: NES_ROM_IMAGE,
                region: NES_ROM_REGION,
                offset: offset as u64,
            });
        ResolvedDebugLocation {
            cpu,
            storage,
            bank: storage.map(|location| (location.offset / 0x2000) as u32),
            exec_mode: ExecMode::Mos6502,
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

pub(crate) fn nes_cpu_location(address: u16) -> CpuLocation {
    CpuLocation {
        space: NES_CPU_SPACE,
        address: u64::from(address),
    }
}
