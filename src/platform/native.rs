use std::io::Write;
use std::path::{Path, PathBuf};

pub(crate) struct FileDialog {
    inner: rfd::FileDialog,
}

impl FileDialog {
    pub(crate) fn new() -> Self {
        Self {
            inner: rfd::FileDialog::new(),
        }
    }

    pub(crate) fn set_title(mut self, title: &str) -> Self {
        self.inner = self.inner.set_title(title);
        self
    }

    pub(crate) fn add_filter(mut self, name: &str, extensions: &[&str]) -> Self {
        self.inner = self.inner.add_filter(name, extensions);
        self
    }

    pub(crate) fn set_file_name(mut self, name: &str) -> Self {
        self.inner = self.inner.set_file_name(name);
        self
    }

    pub(crate) fn set_directory(mut self, dir: impl Into<PathBuf>) -> Self {
        self.inner = self.inner.set_directory(dir.into());
        self
    }

    pub(crate) fn pick_file(self) -> Option<PathBuf> {
        self.inner.pick_file()
    }

    pub(crate) fn save_file(self) -> Option<PathBuf> {
        self.inner.save_file()
    }
}

pub(crate) fn screenshots_dir() -> PathBuf {
    if let Some(config_dir) = dirs::config_dir() {
        return config_dir.join("zeff-boy").join("screenshots");
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("screenshots")
}

pub(crate) fn timestamp_string() -> String {
    chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string()
}

pub(crate) fn format_file_modified_time(meta: &std::fs::Metadata) -> Option<String> {
    let modified = meta.modified().ok()?;
    let dt: chrono::DateTime<chrono::Local> = modified.into();
    Some(dt.format("%b %d %H:%M").to_string())
}

pub(crate) fn open_url(url: &str) {
    if let Err(e) = open::that(url) {
        log::warn!("failed to open '{url}': {e}");
    }
}

pub(crate) fn init_logging() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
}

pub(crate) fn save_dir(system_subdir: &str) -> PathBuf {
    save_root_path().join(system_subdir)
}

pub(crate) fn settings_dir() -> PathBuf {
    config_dir_path()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

pub(crate) fn write_save_data(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    use anyhow::Context;
    use std::time::{SystemTime, UNIX_EPOCH};

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create save directory: {}", parent.display()))?;
    }

    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let ext = path.extension().and_then(|v| v.to_str()).unwrap_or("tmp");
    let tmp_path = path.with_extension(format!("{ext}.tmp.{suffix}"));

    let write_result = (|| -> anyhow::Result<()> {
        let mut file = std::fs::File::create(&tmp_path)
            .with_context(|| format!("failed to create temp file: {}", tmp_path.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("failed to write temp file: {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to flush temp file: {}", tmp_path.display()))?;
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
        return Err(err)
            .map_err(|e| anyhow::anyhow!("failed to finalize save: {}: {e}", path.display()));
    }

    Ok(())
}

pub(crate) fn save_data_exists(path: &Path) -> bool {
    path.exists()
}

pub(crate) fn read_save_data(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(std::fs::read(path)?))
}

pub(crate) fn load_settings_json() -> Option<String> {
    if let Some(dir) = config_dir_path() {
        let config_path = dir.join("settings.json");
        if let Ok(bytes) = std::fs::read(&config_path) {
            return String::from_utf8(bytes).ok();
        }

        let legacy_path = legacy_settings_path();
        if let Ok(bytes) = std::fs::read(&legacy_path)
            && let Ok(json) = String::from_utf8(bytes)
        {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(&config_path, &json);
            return Some(json);
        }

        return None;
    }

    let legacy = legacy_settings_path();
    let bytes = std::fs::read(&legacy).ok()?;
    String::from_utf8(bytes).ok()
}

pub(crate) fn save_settings_json(json: &str) {
    let path = if let Some(dir) = config_dir_path() {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::error!("failed to create settings directory {}: {e}", dir.display());
            return;
        }
        dir.join("settings.json")
    } else {
        legacy_settings_path()
    };

    if let Err(e) = std::fs::write(&path, json) {
        log::error!("failed to write settings to {}: {e}", path.display());
    }
}

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

fn config_dir_path() -> Option<PathBuf> {
    dirs::config_dir().map(|base| base.join("zeff-boy"))
}

#[allow(dead_code)]
pub(crate) fn download_file(_filename: &str, _bytes: &[u8]) {}

fn legacy_settings_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("settings.json")
}
