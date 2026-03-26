use super::Emulator;
use anyhow::Result;
use std::path::{Path, PathBuf};

impl Emulator {
    pub fn flush_battery_sram(&self) -> Result<Option<String>> {
        if !self.bus.cartridge.header().has_battery {
            return Ok(None);
        }

        let Some(bytes) = self.bus.cartridge.dump_battery_data() else {
            return Ok(None);
        };
        if bytes.is_empty() {
            return Ok(None);
        }

        let save_path = save_file_path_for_rom(&self.rom_path);
        write_save_file(&save_path, &bytes)?;
        Ok(Some(save_path.display().to_string()))
    }

    pub(super) fn try_load_battery_sram(&mut self) -> Result<Option<String>> {
        if !self.bus.cartridge.header().has_battery {
            return Ok(None);
        }

        let save_path = save_file_path_for_rom(&self.rom_path);
        if !save_path.exists() {
            return Ok(None);
        }

        let bytes = load_save_file(&save_path)?;
        self.bus.cartridge.load_battery_data(&bytes)?;
        Ok(Some(save_path.display().to_string()))
    }
}

fn save_file_path_for_rom(path: &Path) -> PathBuf {
    let mut save = path.to_path_buf();
    save.set_extension("sav");
    save
}

fn write_save_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            anyhow::anyhow!("failed to create NES save directory {}: {e}", parent.display())
        })?;
    }
    std::fs::write(path, bytes)
        .map_err(|e| anyhow::anyhow!("failed to write NES save {}: {e}", path.display()))?;
    Ok(())
}

fn load_save_file(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("failed to read NES save {}: {e}", path.display()))
}

