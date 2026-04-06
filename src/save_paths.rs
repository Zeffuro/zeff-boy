#[cfg(not(target_arch = "wasm32"))]
use anyhow::Context;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(not(target_arch = "wasm32"))]
fn save_root_path() -> PathBuf {
    if let Some(config_dir) = dirs::config_dir() {
        return config_dir.join("zeff-boy").join("saves");
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("saves")
}

#[cfg(not(target_arch = "wasm32"))]
fn system_save_dir(system_subdir: &str) -> PathBuf {
    save_root_path().join(system_subdir)
}

pub(crate) fn slot_path(
    system_subdir: &str,
    state_ext: &str,
    rom_hash: [u8; 32],
    slot: u8,
) -> anyhow::Result<PathBuf> {
    if slot > 9 {
        anyhow::bail!("invalid save-state slot {slot} (must be 0–9)");
    }
    let hash_hex = hex_hash(&rom_hash);
    #[cfg(not(target_arch = "wasm32"))]
    let mut path = system_save_dir(system_subdir);
    #[cfg(target_arch = "wasm32")]
    let mut path = PathBuf::from(system_subdir);
    path.push(format!("{hash_hex}_slot{slot}.{state_ext}"));
    Ok(path)
}

pub(crate) fn auto_save_path(system_subdir: &str, state_ext: &str, rom_hash: [u8; 32]) -> PathBuf {
    let hash_hex = hex_hash(&rom_hash);
    #[cfg(not(target_arch = "wasm32"))]
    let mut path = system_save_dir(system_subdir);
    #[cfg(target_arch = "wasm32")]
    let mut path = PathBuf::from(system_subdir);
    path.push(format!("{hash_hex}_auto.{state_ext}"));
    path
}

#[cfg(not(target_arch = "wasm32"))]
fn atomic_write_file(path: &Path, bytes: &[u8], label: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {label} directory"))?;
    }

    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let ext = path.extension().and_then(|v| v.to_str()).unwrap_or("tmp");
    let tmp_path = path.with_extension(format!("{ext}.tmp.{suffix}"));

    let write_result = (|| -> anyhow::Result<()> {
        let mut file = std::fs::File::create(&tmp_path)
            .with_context(|| format!("failed to create temp {label}: {}", tmp_path.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("failed to write temp {label}: {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to flush temp {label}: {}", tmp_path.display()))?;
        Ok(())
    })();

    if let Err(err) = write_result {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }

    if path.exists() {
        let _ = std::fs::remove_file(path);
    }

    if let Err(err) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err).with_context(|| format!("failed to finalize {label}: {}", path.display()));
    }

    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write_state_bytes_to_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    atomic_write_file(path, bytes, "save state")
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn write_state_bytes_to_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let key = format!("zeff-state-{}", path.display());
    let encoded = const_hex::encode(bytes);
    let storage = web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .ok_or_else(|| anyhow::anyhow!("localStorage unavailable"))?;
    storage.set_item(&key, &encoded)
        .map_err(|_| anyhow::anyhow!("failed to write to localStorage"))?;
    Ok(())
}

fn hex_hash(hash: &[u8; 32]) -> String {
    const_hex::encode(hash)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write_sram_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    atomic_write_file(path, bytes, "save file")
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn write_sram_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    write_state_bytes_to_file(path, bytes)
}

pub(crate) fn sram_path_for_rom(rom_path: &Path) -> PathBuf {
    rom_path.with_extension("sav")
}

pub(crate) fn flush_battery_sram(
    rom_path: &Path,
    sram_bytes: Option<Vec<u8>>,
) -> anyhow::Result<Option<String>> {
    let Some(bytes) = sram_bytes else {
        return Ok(None);
    };
    let save_path = sram_path_for_rom(rom_path);
    write_sram_file(&save_path, &bytes)?;
    Ok(Some(save_path.display().to_string()))
}

pub(crate) fn try_load_battery_sram(
    rom_path: &Path,
    system_label: &str,
    has_battery: bool,
    load_fn: impl FnOnce(&[u8]) -> anyhow::Result<()>,
) -> anyhow::Result<Option<String>> {
    if !has_battery {
        return Ok(None);
    }
    let save_path = sram_path_for_rom(rom_path);

    #[cfg(not(target_arch = "wasm32"))]
    {
        if !save_path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(&save_path)
            .with_context(|| format!("failed to read {system_label} save {}", save_path.display()))?;
        load_fn(&bytes)?;
        Ok(Some(save_path.display().to_string()))
    }

    #[cfg(target_arch = "wasm32")]
    {
        let key = format!("zeff-state-{}", save_path.display());
        let storage = web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten());
        if let Some(storage) = storage {
            if let Ok(Some(hex)) = storage.get_item(&key) {
                if let Ok(bytes) = const_hex::decode(&hex) {
                    load_fn(&bytes)?;
                    return Ok(Some(save_path.display().to_string()));
                }
            }
        }
        Ok(None)
    }
}
