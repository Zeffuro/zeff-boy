use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn load_rom(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut f = File::open(path)?;
    let mut buffer = Vec::new();
    f.read_to_end(&mut buffer)?;
    Ok(buffer)
}

pub fn save_file_path_for_rom(path: &Path) -> PathBuf {
    path.with_extension("sav")
}

pub fn load_save_file(path: &Path) -> std::io::Result<Vec<u8>> {
    fs::read(path)
}

pub fn write_save_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut tmp_path = path.to_path_buf();
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let ext = path.extension().and_then(|v| v.to_str()).unwrap_or("sav");
    tmp_path.set_extension(format!("{ext}.tmp.{suffix}"));

    {
        let mut file = File::create(&tmp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }

    if path.exists() {
        let _ = fs::remove_file(path);
    }

    fs::rename(&tmp_path, path)
}
