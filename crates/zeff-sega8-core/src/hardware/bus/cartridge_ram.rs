use super::Bus;
use crate::hardware::constants::SMS_CARTRIDGE_RAM_SIZE;

impl Bus {
    pub fn cartridge_ram(&self) -> &[u8; SMS_CARTRIDGE_RAM_SIZE] {
        &self.cartridge_ram
    }

    pub fn cartridge_ram_visible(&self) -> &[u8] {
        &self.cartridge_ram[..self.cartridge.save_ram_kind().size()]
    }

    pub fn load_cartridge_ram(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        let expected_len = self.cartridge.save_ram_kind().size();
        if expected_len == 0 || bytes.len() != expected_len {
            anyhow::bail!(
                "Sega 8-bit cartridge RAM size mismatch: got {} bytes, expected {}",
                bytes.len(),
                expected_len
            );
        }
        self.cartridge_ram.fill(0);
        self.cartridge_ram[..expected_len].copy_from_slice(bytes);
        Ok(())
    }

    pub(super) fn sega_mapper_ram_offset(&self, addr: u16) -> Option<usize> {
        let size = self.cartridge.save_ram_kind().size();
        (size != 0).then(|| self.mapper.slot2_cartridge_ram_offset(addr) % size)
    }
}
