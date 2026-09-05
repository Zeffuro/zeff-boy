use crate::emulator::Emulator;
use crate::hardware::cartridge::RtcDateTime;
use zeff_emu_common::save_ram::SaveRamKind;

impl Emulator {
    pub fn save_ram_kind(&self) -> SaveRamKind {
        self.bus.cartridge.save_ram_kind()
    }

    pub fn has_battery(&self) -> bool {
        self.bus.cartridge.has_battery()
    }

    pub fn has_rtc(&self) -> bool {
        self.bus.cartridge.has_rtc()
    }

    pub fn rtc_date_time(&self) -> Option<RtcDateTime> {
        self.bus.cartridge.rtc_date_time()
    }

    pub fn set_rtc_date_time(&mut self, date_time: RtcDateTime) -> bool {
        self.bus.cartridge.set_rtc_date_time(date_time)
    }

    pub fn dump_battery_sram(&self) -> Option<Vec<u8>> {
        self.bus.cartridge.dump_battery_data()
    }

    pub fn copy_battery_sram_into(&self, output: &mut Vec<u8>) -> bool {
        let Some(bytes) = self.bus.cartridge.battery_data() else {
            return false;
        };
        output.resize(bytes.len(), 0);
        output.copy_from_slice(bytes);
        true
    }

    pub fn dump_rtc_persistence_state(&self) -> Option<Vec<u8>> {
        self.bus.cartridge.dump_rtc_persistence_state()
    }

    pub fn dump_complete_rtc_persistence(&self) -> Option<Vec<u8>> {
        self.bus.cartridge.dump_complete_rtc_persistence()
    }

    pub fn load_complete_rtc_persistence(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        let mut candidate = self.clone();
        candidate
            .bus
            .cartridge
            .load_complete_rtc_persistence(bytes)?;
        *self = candidate;
        Ok(())
    }

    pub fn load_battery_sram(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.bus.cartridge.load_battery_data(bytes)
    }

    pub fn encode_state(&self) -> anyhow::Result<Vec<u8>> {
        crate::save_state::encode_state(self)
    }

    pub fn load_state(&mut self, data: &[u8]) -> anyhow::Result<()> {
        let mut candidate = self.clone();
        crate::save_state::decode_state(&mut candidate, data)?;
        candidate.bus.apu.clear_host_output_after_state_load();
        candidate.opcode_log.clear();
        candidate.instruction_trace.clear();
        *self = candidate;
        Ok(())
    }

    pub fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.load_state(&bytes)
    }
}
