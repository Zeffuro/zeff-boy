use super::Emulator;
use zeff_emu_common::address::Address;
use zeff_emu_common::audio_trace::AudioTraceInvalidation;

pub const DEBUG_RAM_RANGE: (u32, u32) = (0xff0000, 0xffffff);
pub const DEBUG_MEMORY_RANGES: &[(u32, u32)] = &[
    (0xa10003, 0xa10003),
    (0xa10005, 0xa10005),
    (0xa10007, 0xa10007),
    (0xa10009, 0xa10009),
    (0xa1000b, 0xa1000b),
    (0xa1000d, 0xa1000d),
    DEBUG_RAM_RANGE,
];

impl Emulator {
    pub fn debug_dma_activity_epoch(&self) -> u64 {
        self.bus.vdp.dma_activity_epoch()
    }

    pub fn validate_debug_ram_writes(writes: &[(Address, u8)]) -> Result<(), Address> {
        match writes
            .iter()
            .find(|(address, _)| !(DEBUG_RAM_RANGE.0..=DEBUG_RAM_RANGE.1).contains(address))
        {
            Some(&(address, _)) => Err(address),
            None => Ok(()),
        }
    }

    pub fn debug_write_ram(&mut self, writes: &[(Address, u8)]) -> Result<(), Address> {
        Self::validate_debug_ram_writes(writes)?;
        for &(address, value) in writes {
            self.bus.ram[address as usize & 0xffff] = value;
        }
        if !writes.is_empty() {
            self.invalidate_audio_trace(AudioTraceInvalidation::ExternalMutation);
        }
        Ok(())
    }
}

impl Emulator {
    fn debug_input_candidate(
        &self,
        writes: &[(Address, u8)],
    ) -> Result<Option<crate::hardware::input::Input>, Address> {
        for &(address, _) in writes {
            if !DEBUG_MEMORY_RANGES
                .iter()
                .any(|&(start, end)| (start..=end).contains(&address))
            {
                return Err(address);
            }
        }
        if !writes
            .iter()
            .any(|&(address, _)| address < DEBUG_RAM_RANGE.0)
        {
            return Ok(None);
        }
        let now = self.bus.input_state_tick().map_err(|_| writes[0].0)?;
        let mut input = self.bus.input.clone();
        for &(address, value) in writes {
            let accepted = match address {
                0xa10003 | 0xa10005 | 0xa10007 => input.write_data(
                    ((address - 0xa10003) / 2) as usize,
                    value,
                    now,
                    self.bus.region,
                ),
                0xa10009 | 0xa1000b | 0xa1000d => input.write_control(
                    ((address - 0xa10009) / 2) as usize,
                    value,
                    now,
                    self.bus.region,
                ),
                _ => true,
            };
            if !accepted {
                return Err(address);
            }
        }
        Ok(Some(input))
    }

    pub fn validate_debug_memory_writes(&self, writes: &[(Address, u8)]) -> Result<(), Address> {
        self.debug_input_candidate(writes).map(|_| ())
    }

    pub fn debug_write_memory(&mut self, writes: &[(Address, u8)]) -> Result<(), Address> {
        let input = self.debug_input_candidate(writes)?;
        if let Some(input) = input {
            self.bus.input = input;
        }
        for &(address, value) in writes {
            if (DEBUG_RAM_RANGE.0..=DEBUG_RAM_RANGE.1).contains(&address) {
                self.bus.ram[address as usize & 0xffff] = value;
            }
        }
        if !writes.is_empty() {
            self.invalidate_audio_trace(AudioTraceInvalidation::ExternalMutation);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
