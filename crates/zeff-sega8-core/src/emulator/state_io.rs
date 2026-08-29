use crate::emulator::Emulator;
use zeff_emu_common::save_ram::SaveRamKind;

impl Emulator {
    pub fn save_ram_kind(&self) -> SaveRamKind {
        self.bus.cartridge.save_ram_kind()
    }

    pub fn has_battery(&self) -> bool {
        self.save_ram_kind().is_battery_backed()
    }

    pub fn dump_battery_sram(&self) -> Option<Vec<u8>> {
        self.has_battery()
            .then(|| self.bus.cartridge_ram().to_vec())
    }

    pub fn load_battery_sram(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        if !self.has_battery() {
            anyhow::bail!(
                "Sega 8-bit ROM does not declare known battery-backed SRAM; classified as {:?}",
                self.save_ram_kind()
            );
        }
        self.bus.load_cartridge_ram(bytes)
    }

    pub fn encode_state(&self) -> anyhow::Result<Vec<u8>> {
        crate::save_state::encode_state(self)
    }

    pub fn load_state(&mut self, data: &[u8]) -> anyhow::Result<()> {
        let mut candidate = self.clone();
        crate::save_state::decode_state(&mut candidate, data)?;
        *self = candidate;
        Ok(())
    }

    pub fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.load_state(&bytes)
    }
}
