use std::path::{Path, PathBuf};

pub(crate) struct BackendPaths {
    rom_path: PathBuf,
    source_path: PathBuf,
}

impl BackendPaths {
    pub(crate) fn new(rom_path: PathBuf) -> Self {
        Self::with_source_path(rom_path.clone(), rom_path)
    }

    pub(crate) fn with_source_path(rom_path: PathBuf, source_path: PathBuf) -> Self {
        Self {
            rom_path,
            source_path,
        }
    }

    pub(crate) fn rom_path(&self) -> &Path {
        &self.rom_path
    }

    pub(crate) fn source_path(&self) -> &Path {
        &self.source_path
    }
}
