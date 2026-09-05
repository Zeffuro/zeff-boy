use crate::core::CoreState;
use anyhow::{Context, ensure};
use std::os::raw::c_void;
use std::sync::Mutex;

pub(crate) static SRAM: Mutex<SramBridge> = Mutex::new(SramBridge::new());

pub(crate) fn import_external(state: &mut CoreState) -> anyhow::Result<()> {
    crate::callbacks::lock(&SRAM).import_external(state)
}

pub(crate) fn publish(state: &CoreState) -> anyhow::Result<()> {
    crate::callbacks::lock(&SRAM).publish(state)
}

pub(crate) fn import_and_publish(state: &mut CoreState) -> anyhow::Result<()> {
    let mut bridge = crate::callbacks::lock(&SRAM);
    bridge.import_external(state)?;
    bridge.publish(state)
}

pub(crate) struct SramBridge {
    exposed: Vec<u8>,
    last_published: Vec<u8>,
}

impl SramBridge {
    const fn new() -> Self {
        Self {
            exposed: Vec::new(),
            last_published: Vec::new(),
        }
    }

    pub fn initialize(&mut self, state: &CoreState) {
        self.exposed = state.battery_sram().unwrap_or_default();
        self.last_published.clone_from(&self.exposed);
    }

    pub fn clear(&mut self) {
        self.exposed.clear();
        self.last_published.clear();
    }

    pub fn data(&mut self) -> *mut c_void {
        if self.exposed.is_empty() {
            std::ptr::null_mut()
        } else {
            self.exposed.as_mut_ptr().cast()
        }
    }

    pub fn len(&self) -> usize {
        self.exposed.len()
    }

    #[cfg(test)]
    pub fn is_published(&self) -> bool {
        self.exposed == self.last_published
    }

    #[cfg(test)]
    pub fn truncate_exposed(&mut self, len: usize) {
        self.exposed.truncate(len);
    }

    pub fn import_external(&mut self, state: &mut CoreState) -> anyhow::Result<()> {
        if self.exposed == self.last_published {
            return Ok(());
        }

        let current = state
            .battery_sram()
            .context("loaded core no longer exposes save RAM")?;
        ensure!(
            current.len() == self.exposed.len(),
            "save RAM size changed from {} to {} bytes",
            self.exposed.len(),
            current.len()
        );
        state.load_battery_sram(&self.exposed)?;
        self.publish(state)
    }

    pub fn publish(&mut self, state: &CoreState) -> anyhow::Result<()> {
        state.sync_sram_to_buf(&mut self.exposed)?;
        ensure!(
            self.exposed.len() == self.last_published.len(),
            "save RAM publication changed size"
        );
        self.last_published.copy_from_slice(&self.exposed);
        Ok(())
    }
}
