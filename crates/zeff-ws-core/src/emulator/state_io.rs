use crate::emulator::Emulator;
use zeff_emu_common::save_ram::SaveRamKind;

impl Emulator {
    pub fn has_battery(&self) -> bool {
        self.bus.cartridge.has_battery()
    }

    pub fn save_ram_kind(&self) -> SaveRamKind {
        self.bus.cartridge.save_ram_kind()
    }

    pub fn dump_battery_sram(&self) -> Option<Vec<u8>> {
        self.bus.cartridge.dump_battery_data()
    }

    pub fn dump_rtc_persistence_state(&self) -> Option<Vec<u8>> {
        self.bus.dump_rtc_persistence_state()
    }

    pub fn dump_complete_rtc_persistence(&self) -> Option<Vec<u8>> {
        self.bus.dump_complete_rtc_persistence()
    }

    pub fn load_complete_rtc_persistence(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        let mut candidate = self.clone();
        candidate.bus.load_complete_rtc_persistence(bytes)?;
        *self = candidate;
        Ok(())
    }

    pub fn load_battery_sram(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        let mut candidate = self.clone();
        candidate.bus.load_battery_persistence(bytes)?;
        *self = candidate;
        Ok(())
    }

    pub fn encode_state(&self) -> anyhow::Result<Vec<u8>> {
        crate::save_state::encode_state(self)
    }

    pub fn load_state(&mut self, data: &[u8]) -> anyhow::Result<()> {
        let mut candidate = self.clone();
        crate::save_state::decode_state(&mut candidate, data)?;
        candidate.opcode_log.clear();
        candidate.instruction_trace.clear();
        *self = candidate;
        Ok(())
    }

    pub fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.load_state(&bytes)
    }
}
