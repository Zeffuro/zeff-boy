use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn save_root_path() -> PathBuf {
    if let Some(config_dir) = dirs::config_dir() {
        return config_dir.join("zeff-boy").join("saves");
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("saves")
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
    let mut path = save_root_path().join(system_subdir);
    path.push(format!("{hash_hex}_slot{slot}.{state_ext}"));
    Ok(path)
}

pub(crate) fn auto_save_path(
    system_subdir: &str,
    state_ext: &str,
    rom_hash: [u8; 32],
) -> PathBuf {
    let hash_hex = hex_hash(&rom_hash);
    let mut path = save_root_path().join(system_subdir);
    path.push(format!("{hash_hex}_auto.{state_ext}"));
    path
}

pub(crate) fn write_state_bytes_to_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("failed to create save-state directory: {e}"))?;
    }

    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let ext = path.extension().and_then(|v| v.to_str()).unwrap_or("state");
    let tmp_path = path.with_extension(format!("{ext}.tmp.{suffix}"));

    {
        let mut file = std::fs::File::create(&tmp_path)
            .map_err(|e| anyhow::anyhow!("failed to create temp save state: {}: {e}", tmp_path.display()))?;
        file.write_all(bytes)
            .map_err(|e| anyhow::anyhow!("failed to write temp save state: {}: {e}", tmp_path.display()))?;
        file.sync_all()
            .map_err(|e| anyhow::anyhow!("failed to flush temp save state: {}: {e}", tmp_path.display()))?;
    }

    if path.exists() {
        let _ = std::fs::remove_file(path);
    }

    std::fs::rename(&tmp_path, path)
        .map_err(|e| anyhow::anyhow!("failed to finalize save state: {}: {e}", path.display()))?;
    Ok(())
}

fn hex_hash(hash: &[u8; 32]) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn write_sram_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("failed to create save directory: {e}"))?;
    }

    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let ext = path.extension().and_then(|v| v.to_str()).unwrap_or("sav");
    let tmp_path = path.with_extension(format!("{ext}.tmp.{suffix}"));

    {
        let mut file = std::fs::File::create(&tmp_path)
            .map_err(|e| anyhow::anyhow!("failed to create temp save file: {}: {e}", tmp_path.display()))?;
        file.write_all(bytes)
            .map_err(|e| anyhow::anyhow!("failed to write temp save file: {}: {e}", tmp_path.display()))?;
        file.sync_all()
            .map_err(|e| anyhow::anyhow!("failed to flush temp save file: {}: {e}", tmp_path.display()))?;
    }

    if path.exists() {
        let _ = std::fs::remove_file(path);
    }

    std::fs::rename(&tmp_path, path)
        .map_err(|e| anyhow::anyhow!("failed to finalize save file: {}: {e}", path.display()))?;
    Ok(())
}

