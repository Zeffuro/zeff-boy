use super::*;
use zeff_m68k::bus::FunctionCode;

impl Bus {
    pub(super) fn unused_cartridge_observation(&self, access: BusAccess) -> bool {
        if self.download_observation(access) {
            return true;
        }
        let lane_address = access.bus_address | u32::from(access.lanes == ByteLanes::Lower);
        if self.rmw.is_some()
            || access.kind != AccessKind::Data
            || !matches!(
                access.function_code,
                FunctionCode::UserData | FunctionCode::SupervisorData
            )
            || access.bus_address & 1 != 0
            || access.logical_address & BUS_ADDRESS_MASK != lane_address
        {
            return false;
        }
        let cartridge = &self.cartridge;
        let sf001_unused = cartridge.save.is_none()
            && cartridge
                .sf001
                .as_ref()
                .is_some_and(|board| board.unclaimed_expansion(access.bus_address));
        let parallel = cartridge.sf001.is_none()
            && cartridge
                .save
                .as_ref()
                .is_some_and(crate::cartridge::SaveMemory::unclaimed_parallel_8k);
        let unprotected =
            cartridge.unprotected_rom.is_some() && cartridge.unprotected_rom_compatible();
        let unused_expansion =
            (CARTRIDGE_END + 1..=CARTRIDGE_UNUSED_END).contains(&access.bus_address);
        let unused_time = unprotected
            && cartridge
                .unprotected_rom
                .is_some_and(|board| board.unclaimed_time(access.bus_address));
        (unused_expansion
            && (cartridge.unprotected_rom.is_none() && (parallel || sf001_unused) || unprotected)
            || unused_time)
            && cartridge.linear_rom.is_none()
            && cartridge.parallel_adapter.is_none()
            && cartridge.registers.is_none()
            && cartridge.protection.is_none()
            && cartridge.sf002.is_none()
            && cartridge.sf004.is_none()
            && cartridge.extended_ssf.is_none()
            && cartridge.realtec.is_none()
            && cartridge.ssf2.is_none()
            && cartridge.lock_on.is_none()
            && cartridge.lock_on_save.is_none()
    }
}
