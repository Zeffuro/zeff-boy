use super::*;

impl Bus {
    pub fn peek(&self, logical: u32) -> Option<u8> {
        self.peek_sampled(logical).map(|byte| byte.value)
    }

    fn peek_sampled(&self, logical: u32) -> Option<SampledByte> {
        let address = logical & BUS_ADDRESS_MASK;
        if let Some(value) = self.peek_svp(address) {
            return value.map(SampledByte::unknown);
        }
        if let Some(value) = self.tmss.as_ref().and_then(|tmss| tmss.read_boot(address)) {
            return Some(SampledByte::unknown(value));
        }
        if self
            .cartridge
            .parallel_adapter
            .as_ref()
            .is_some_and(|board| board.guarded(address))
        {
            return None;
        }
        if let Some(value) = self
            .cartridge
            .parallel_adapter
            .as_ref()
            .and_then(|board| board.read(address))
        {
            return Some(SampledByte::unknown(value));
        }
        if let Some(board) = &self.cartridge.sf001
            && board.decode(address) != crate::cartridge::sf001::Location::Unclaimed
        {
            return self.cartridge.read_sampled(address);
        }
        if let Some(value) = self
            .cartridge
            .protection
            .and_then(|profile| profile.read(address))
        {
            return Some(SampledByte::unknown(value));
        }
        if let Some(value) = self
            .cartridge
            .registers
            .as_ref()
            .and_then(|registers| registers.read(address))
        {
            return Some(SampledByte::unknown(value));
        }
        match address {
            address
                if address <= CARTRIDGE_END || self.cartridge.expansion_save_contains(address) =>
            {
                self.cartridge.read_sampled(address)
            }
            WORK_RAM_START..=BUS_ADDRESS_MASK => Some(SampledByte::unknown(
                self.ram[address as usize & WORK_RAM_MASK],
            )),
            SOUND_APERTURE_START..=SOUND_APERTURE_END => {
                let sound_address = (address & SOUND_MAIN_MASK) as u16;
                if (SOUND_OPN2_START..=SOUND_OPN2_END).contains(&sound_address) {
                    Some(SampledByte::unknown(if sound_address & 3 == 0 {
                        self.audio.opn2_status_at(self.now.get())
                    } else {
                        0
                    }))
                } else {
                    self.active_sound_memory()
                        .read(sound_address)
                        .map(SampledByte::unknown)
                }
            }
            _ => self.read_input(address).map(SampledByte::unknown),
        }
    }

    pub(super) fn read(&mut self, access: BusAccess) -> Result<u16, BusError> {
        self.read_owned(access, None, false)
            .map(|input| input.value)
    }

    pub(super) fn read_owned(
        &mut self,
        access: BusAccess,
        input_begin: Option<u64>,
        main_data: bool,
    ) -> Result<InstructionInput, BusError> {
        self.preflight_svp(access, Direction::Read)?;
        self.preflight_bounded_odd(access, Direction::Read)?;
        self.preflight_bounded_switched(access, None)?;
        self.preflight_save_latch(access, Direction::Read)?;
        self.preflight_download(access, Direction::Read)?;
        #[cfg(feature = "first-failure-diagnostic")]
        crate::diagnostic::service_entry(self, access, crate::diagnostic::Callback::Read);
        let undriven = self.closed_sound_read(access);
        let result = input_begin
            .and_then(|begin| {
                self.read_input_cycle(access, begin)
                    .map(|result| result.map(InstructionInput::unknown))
            })
            .unwrap_or_else(|| self.read_mapped(access, main_data));
        self.prefetch = match (access.kind, access.lanes, &result) {
            (_, _, Ok(_)) if undriven => self.prefetch,
            (AccessKind::Prefetch, ByteLanes::Both, Ok(value))
                if self
                    .tmss
                    .as_ref()
                    .and_then(|tmss| tmss.read_boot(access.bus_address))
                    .is_some()
                    || (access.bus_address <= CARTRIDGE_END
                        && self
                            .cartridge
                            .rom_source_offset(access.bus_address + 1)
                            .is_some())
                    || (access.bus_address <= CARTRIDGE_END
                        && self.cartridge.download.is_some())
                    || (access.bus_address <= CARTRIDGE_END
                        && self.cartridge.sf001.as_ref().is_some_and(|board| {
                            matches!(
                                board.decode(access.bus_address),
                                crate::cartridge::sf001::Location::Sram(_)
                            )
                        }))
                    || (WORK_RAM_START..=BUS_ADDRESS_MASK).contains(&access.bus_address)
                    || self
                        .cartridge
                        .parallel_adapter
                        .as_ref()
                        .is_some_and(|board| board.contains(access.bus_address)) =>
            {
                Prefetch::Full(value.value)
            }
            _ => Prefetch::Empty,
        };
        result
    }

    fn read_mapped(
        &mut self,
        access: BusAccess,
        main_data: bool,
    ) -> Result<InstructionInput, BusError> {
        if let Some(result) = self.read_svp(access) {
            return result.map(InstructionInput::unknown);
        }
        self.preflight_tmss(access, None)?;
        if let Some(result) = self.read_tmss(access) {
            return result.map(InstructionInput::unknown);
        }
        if let Some(result) = self.read_download(access, main_data) {
            return result.map(InstructionInput::unknown);
        }
        if let Some(result) = self.read_bounded_odd(access) {
            return result.map(InstructionInput::unknown);
        }
        if let Some(result) = self.read_bounded_switched(access) {
            return result;
        }
        if let Some(result) = self.read_realtec(access) {
            return result;
        }
        if let Some(result) = self.read_parallel_adapter(access) {
            return result.map(InstructionInput::unknown);
        }
        if let Some(result) = self.read_extended_ssf(access) {
            return result;
        }
        if let Some(result) = self.read_sf004(access) {
            return result;
        }
        if let Some(result) = self.read_sf002(access) {
            return result;
        }
        if let Some(result) = self.read_sf001(access) {
            return result;
        }
        if let Some(result) = self.read_vdp(access, main_data) {
            return result.map(InstructionInput::unknown);
        }
        if let Some(value) = self.read_sound(access) {
            return Ok(InstructionInput::unknown(value));
        }
        if let Some(result) = self.read_unused_input(access) {
            return result.map(InstructionInput::unknown);
        }
        let address = access.bus_address;
        if let Some(result) = self.read_lock_on_save(access) {
            return result.map(InstructionInput::unknown);
        }
        if let Some(result) = self.read_registers(access) {
            return result.map(InstructionInput::unknown);
        }
        let failure = || BusError::UnsupportedAccess {
            address,
            direction: Direction::Read,
        };
        if let Some(value) = self
            .cartridge
            .save
            .as_ref()
            .and_then(|save| save.serial_read(address))
        {
            return if access.lanes == ByteLanes::Upper {
                self.prefetch
                    .upper()
                    .map(|value| u16::from(value) << 8)
                    .ok_or_else(failure)
                    .map(InstructionInput::unknown)
            } else {
                Ok(InstructionInput::unknown(value))
            };
        }
        if (SOUND_APERTURE_START..=SOUND_APERTURE_END).contains(&address) {
            return Err(failure());
        }
        if main_data && self.unused_cartridge_observation(access) {
            return self
                .prefetch
                .sample(access.lanes)
                .ok_or_else(failure)
                .map(InstructionInput::unknown);
        }
        let high = self.peek_sampled(address).ok_or_else(failure)?;
        let low = self
            .peek_sampled(address.wrapping_add(1))
            .ok_or_else(failure)?;
        Ok(instruction_input::selected(access, high, low))
    }
}
