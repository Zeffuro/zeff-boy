use crate::emu_backend::PceBackend;

use super::{
    AddressSpaceId, CpuLocation, DebugAccess, DebugAddressResolver, ExecMode, ImageId, RegionId,
    ResolvedDebugLocation, StorageLocation,
};

const PCE_CPU_SPACE: AddressSpaceId = AddressSpaceId(0);
const PCE_ROM_IMAGE: ImageId = ImageId(0);
const PCE_ROM_REGION: RegionId = RegionId(0);

impl DebugAddressResolver for PceBackend {
    fn resolve_exec(&self, cpu: CpuLocation) -> ResolvedDebugLocation {
        let Ok(address) = u16::try_from(cpu.address) else {
            return ResolvedDebugLocation {
                cpu,
                storage: None,
                bank: None,
                exec_mode: ExecMode::Unknown,
                mapping_epoch: self.mapping_epoch(),
            };
        };
        let page = self.debug_cpu_snapshot().physical_page(address);
        let storage = self
            .rom_offset_for_cpu_address(address)
            .map(|offset| StorageLocation {
                image: PCE_ROM_IMAGE,
                region: PCE_ROM_REGION,
                offset: u64::from(offset),
            });
        ResolvedDebugLocation {
            cpu,
            storage,
            bank: Some(u32::from(page)),
            exec_mode: ExecMode::Unknown,
            mapping_epoch: self.mapping_epoch(),
        }
    }

    fn resolve_data(&self, cpu: CpuLocation, _access: DebugAccess) -> ResolvedDebugLocation {
        self.resolve_exec(cpu)
    }

    fn mapping_epoch(&self) -> u64 {
        self.rom_mapping_token()
    }
}

pub(crate) const fn pce_cpu_location(address: u16) -> CpuLocation {
    CpuLocation {
        space: PCE_CPU_SPACE,
        address: address as u64,
    }
}

pub(crate) const fn pce_static_location(address: u16, physical_page: u8) -> ResolvedDebugLocation {
    ResolvedDebugLocation {
        cpu: pce_cpu_location(address),
        storage: None,
        bank: Some(physical_page as u32),
        exec_mode: ExecMode::Unknown,
        mapping_epoch: 0,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::symbols::SymbolSession;

    fn rom_with_program() -> Vec<u8> {
        let mut rom = vec![0xEA; 0x2000];
        rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
        rom
    }

    #[test]
    fn runtime_resolver_uses_the_active_mpr_page() {
        let backend = PceBackend::new(rom_with_program(), PathBuf::from("test.pce")).unwrap();
        let location = backend.resolve_exec(pce_cpu_location(0xE000));

        assert_eq!(location.bank, Some(0));
        assert_eq!(location.storage.unwrap().offset, 0);
        assert_eq!(location.mapping_epoch, backend.rom_mapping_token());
    }

    #[test]
    fn static_locations_select_only_the_matching_physical_page() {
        let mut session = SymbolSession::default();
        let context = crate::symbols::import::ImportContext {
            target: crate::symbols::import::TargetInfo {
                system: zeff_emu_common::system::System::Pce,
            },
            image: PCE_ROM_IMAGE,
            rom_region: PCE_ROM_REGION,
            cpu_space: PCE_CPU_SPACE,
            source_name: None,
        };
        let module = crate::symbols::import::import_symbols(
            "game.sym",
            b"01 A123 first\n02 A123 second",
            &context,
        )
        .unwrap();
        session.store.extend(module.symbols);

        assert_eq!(
            session.symbol_name_at_debug_location(pce_static_location(0xA123, 1)),
            Some("first")
        );
        assert_eq!(
            session.symbol_name_at_debug_location(pce_static_location(0xA123, 2)),
            Some("second")
        );
        assert_eq!(
            session.symbol_name_at_debug_location(pce_static_location(0xA123, 3)),
            None
        );
    }
}
