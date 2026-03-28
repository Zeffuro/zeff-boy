use anyhow::Context;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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
    let mut path = system_save_dir(system_subdir);
    path.push(format!("{hash_hex}_slot{slot}.{state_ext}"));
    Ok(path)
}

pub(crate) fn auto_save_path(
    system_subdir: &str,
    state_ext: &str,
    rom_hash: [u8; 32],
) -> PathBuf {
    let hash_hex = hex_hash(&rom_hash);
    let mut path = system_save_dir(system_subdir);
    path.push(format!("{hash_hex}_auto.{state_ext}"));
    path
}

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

    {
        let mut file = std::fs::File::create(&tmp_path)
            .with_context(|| format!("failed to create temp {label}: {}", tmp_path.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("failed to write temp {label}: {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to flush temp {label}: {}", tmp_path.display()))?;
    }

    if path.exists() {
        let _ = std::fs::remove_file(path);
    }

    std::fs::rename(&tmp_path, path)
        .with_context(|| format!("failed to finalize {label}: {}", path.display()))?;
    Ok(())
}

pub(crate) fn write_state_bytes_to_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    atomic_write_file(path, bytes, "save state")
}

fn hex_hash(hash: &[u8; 32]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(64);
    for b in hash {
        let _ = write!(s, "{b:02x}");
    }
    s
}

pub(crate) fn write_sram_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    atomic_write_file(path, bytes, "save file")
}

