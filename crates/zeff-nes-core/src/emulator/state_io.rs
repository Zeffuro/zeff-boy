use super::Emulator;
use anyhow::Result;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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

    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_path = path.with_extension(format!("sav.tmp.{suffix}"));

    let mut file = std::fs::File::create(&tmp_path).map_err(|e| {
        anyhow::anyhow!("failed to create temp NES save {}: {e}", tmp_path.display())
    })?;
    file.write_all(bytes).map_err(|e| {
        anyhow::anyhow!("failed to write temp NES save {}: {e}", tmp_path.display())
    })?;
    file.sync_all().map_err(|e| {
        anyhow::anyhow!("failed to flush temp NES save {}: {e}", tmp_path.display())
    })?;
    drop(file);

    // On Windows, rename fails if destination exists
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    std::fs::rename(&tmp_path, path).map_err(|e| {
        anyhow::anyhow!("failed to finalize NES save {}: {e}", path.display())
    })?;
    Ok(())
}

fn load_save_file(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("failed to read NES save {}: {e}", path.display()))
}

