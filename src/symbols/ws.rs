use crate::emu_backend::WsBackend;

use super::{
    AddressSpaceId, CpuLocation, DebugAccess, DebugAddressResolver, ExecMode, ImageId, RegionId,
    ResolvedDebugLocation, StorageLocation,
};

const WS_CPU_SPACE: AddressSpaceId = AddressSpaceId(0);
const WS_ROM_IMAGE: ImageId = ImageId(0);
const WS_ROM_REGION: RegionId = RegionId(0);

impl DebugAddressResolver for WsBackend {
    fn resolve_exec(&self, cpu: CpuLocation) -> ResolvedDebugLocation {
        let storage = u32::try_from(cpu.address)
            .ok()
            .and_then(|address| self.emu.rom_offset_for_cpu_address(address))
            .map(|offset| StorageLocation {
                image: WS_ROM_IMAGE,
                region: WS_ROM_REGION,
                offset: offset as u64,
            });
        ResolvedDebugLocation {
            cpu,
            storage,
            bank: storage.map(|location| (location.offset / 0x10000) as u32),
            exec_mode: ExecMode::V30,
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

pub(crate) fn ws_cpu_location(address: u32) -> CpuLocation {
    CpuLocation {
        space: WS_CPU_SPACE,
        address: u64::from(address),
    }
}
