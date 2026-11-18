use super::*;

#[derive(Clone, Default)]
pub(super) struct RomBreakpoints {
    pub(super) offsets: Vec<u64>,
    pub(super) hit: Option<u64>,
}

impl RomBreakpoints {
    fn add(&mut self, offset: u64) {
        if let Err(index) = self.offsets.binary_search(&offset) {
            self.offsets.insert(index, offset);
        }
    }

    fn remove(&mut self, offset: u64) {
        if let Ok(index) = self.offsets.binary_search(&offset) {
            self.offsets.remove(index);
        }
    }

    pub(super) fn check(&mut self, source: Option<u64>) -> bool {
        if let Some(source) = source.filter(|source| self.offsets.binary_search(source).is_ok()) {
            self.hit = Some(source);
            true
        } else {
            false
        }
    }
}

impl MdBackend {
    pub(in crate::emu_backend) fn supports_rom_breakpoints(&self) -> bool {
        self.core().supports_rom_breakpoints()
    }

    pub(in crate::emu_backend::md) fn apply_rom_breakpoint_actions(
        &mut self,
        actions: &DebugUiActions,
    ) {
        if self.rollback_owned() {
            return;
        }
        if !self.supports_rom_breakpoints() {
            return;
        }
        let length = self.core().rom_bytes().len();
        let valid = |offset: u64| usize::try_from(offset).is_ok_and(|offset| offset < length);
        for offset in actions
            .add_rom_breakpoints
            .iter()
            .copied()
            .filter(|&offset| valid(offset))
        {
            self.debugger.rom.add(offset);
        }
        for offset in actions
            .remove_rom_breakpoints
            .iter()
            .copied()
            .filter(|&offset| valid(offset))
        {
            self.debugger.rom.remove(offset);
        }
        for offset in actions
            .toggle_rom_breakpoints
            .iter()
            .copied()
            .filter(|&offset| valid(offset))
        {
            if self.debugger.rom.offsets.binary_search(&offset).is_ok() {
                self.debugger.rom.remove(offset);
            } else {
                self.debugger.rom.add(offset);
            }
        }
    }
}
