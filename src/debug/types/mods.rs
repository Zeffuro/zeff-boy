use std::path::PathBuf;

use crate::mods::ModEntry;

pub(crate) struct ModState {
    pub(crate) entries: Vec<ModEntry>,
    pub(crate) mods_dir: Option<PathBuf>,
    pub(crate) status_message: Option<String>,
    pub(crate) needs_reload: bool,
}

impl ModState {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
            mods_dir: None,
            status_message: None,
            needs_reload: false,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.mods_dir = None;
        self.status_message = None;
        self.needs_reload = false;
    }

    pub(crate) fn enabled_count(&self) -> usize {
        self.entries.iter().filter(|m| m.enabled).count()
    }
}
