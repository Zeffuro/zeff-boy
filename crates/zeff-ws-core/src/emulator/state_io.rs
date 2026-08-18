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

    pub fn load_battery_sram(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.bus.cartridge.load_battery_data(bytes)
    }

    pub fn encode_state(&self) -> anyhow::Result<Vec<u8>> {
        crate::save_state::encode_state(self)
    }

    pub fn load_state(&mut self, data: &[u8]) -> anyhow::Result<()> {
        crate::save_state::decode_state(self, data)?;
        self.opcode_log.clear();
        self.instruction_trace.clear();
        Ok(())
    }

    pub fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.load_state(&bytes)
    }
}
