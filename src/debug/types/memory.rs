use std::collections::HashMap;

use crate::emu_backend::ActiveSystem;
use zeff_emu_common::address::Address;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemorySearchMode {
    ByteValue,
    ByteSequence,
    AsciiString,
}

#[derive(Clone)]
pub(crate) struct MemorySearchResult {
    pub(crate) address: Address,
    pub(crate) matched_bytes: Vec<u8>,
}

#[derive(Clone)]
pub(crate) struct MemoryBookmark {
    pub(crate) address: Address,
    pub(crate) label: String,
}

#[derive(Clone, Copy)]
pub(crate) struct MemoryByteDiff {
    pub(crate) address: Address,
    pub(crate) old: u8,
    pub(crate) new: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MemoryAddressSpace {
    pub(crate) min: Address,
    pub(crate) max_start: Address,
    pub(crate) default_start: Address,
    pub(crate) addr_width: usize,
}

impl MemoryAddressSpace {
    pub(crate) const GB_NES: Self = Self {
        min: 0,
        max_start: 0xFF00,
        default_start: 0,
        addr_width: 4,
    };

    pub(crate) const GBA: Self = Self {
        min: 0,
        max_start: 0x0E00_FF00,
        default_start: 0x0200_0000,
        addr_width: 8,
    };

    pub(crate) fn for_system(system: ActiveSystem) -> Self {
        match system {
            ActiveSystem::GameBoy | ActiveSystem::Nes => Self::GB_NES,
            ActiveSystem::GameBoyAdvance => Self::GBA,
        }
    }

    pub(crate) fn clamp_start(self, addr: Address) -> Address {
        addr.clamp(self.min, self.max_start) & !0xF
    }

    pub(crate) fn format(self, addr: Address) -> String {
        match self.addr_width {
            4 => format!("{addr:04X}"),
            6 => format!("{addr:06X}"),
            _ => format!("{addr:08X}"),
        }
    }
}

pub(crate) struct MemoryViewerState {
    pub(crate) address_space: MemoryAddressSpace,
    pub(crate) view_start: Address,
    pub(crate) jump_input: String,
    pub(crate) prev_start: Option<Address>,
    pub(crate) prev_bytes: Vec<u8>,
    pub(crate) flash_ticks: Vec<u8>,
    pub(crate) edit_addr: Option<Address>,
    pub(crate) edit_addr_input: String,
    pub(crate) edit_value: String,
    pub(crate) enable_editing: bool,
    pub(crate) search_query: String,
    pub(crate) search_mode: MemorySearchMode,
    pub(crate) search_results: Vec<MemorySearchResult>,
    pub(crate) search_max_results: usize,
    pub(crate) search_pending: bool,
    pub(crate) tbl_map: HashMap<u8, String>,
    pub(crate) tbl_path: Option<String>,
    pub(crate) inspector_addr_input: String,
    pub(crate) inspector_addr: Option<Address>,
    pub(crate) bookmark_addr_input: String,
    pub(crate) bookmark_label_input: String,
    pub(crate) bookmarks: Vec<MemoryBookmark>,
    pub(crate) recent_diffs: Vec<MemoryByteDiff>,
    pub(crate) pattern_query: String,
    pub(crate) pattern_max_results: usize,
    pub(crate) pattern_results: Vec<MemorySearchResult>,
    pub(crate) pattern_error: Option<String>,
}

impl MemoryViewerState {
    pub(crate) fn new() -> Self {
        Self {
            address_space: MemoryAddressSpace::GB_NES,
            view_start: 0,
            jump_input: String::from("0000"),
            prev_start: None,
            prev_bytes: Vec::new(),
            flash_ticks: vec![0; 256],
            edit_addr: None,
            edit_addr_input: String::new(),
            edit_value: String::new(),
            enable_editing: false,
            search_query: String::new(),
            search_mode: MemorySearchMode::ByteValue,
            search_results: Vec::new(),
            search_max_results: 256,
            search_pending: false,
            tbl_map: HashMap::new(),
            tbl_path: None,
            inspector_addr_input: String::new(),
            inspector_addr: None,
            bookmark_addr_input: String::new(),
            bookmark_label_input: String::new(),
            bookmarks: Vec::new(),
            recent_diffs: Vec::new(),
            pattern_query: String::new(),
            pattern_max_results: 64,
            pattern_results: Vec::new(),
            pattern_error: None,
        }
    }

    pub(crate) fn configure_for_system(&mut self, system: ActiveSystem) {
        let address_space = MemoryAddressSpace::for_system(system);
        if self.address_space == address_space {
            return;
        }
        self.address_space = address_space;
        self.view_start = address_space.default_start;
        self.jump_input = address_space.format(self.view_start);
        self.prev_start = None;
        self.prev_bytes.clear();
        self.recent_diffs.clear();
        self.flash_ticks.fill(0);
        self.edit_addr = None;
        self.inspector_addr = None;
    }
}

#[derive(Clone)]
pub(crate) struct RomSearchResult {
    pub(crate) offset: u32,
    pub(crate) matched_bytes: Vec<u8>,
}

pub(crate) struct RomViewerState {
    pub(crate) view_start: u32,
    pub(crate) jump_input: String,
    pub(crate) rom_size: u32,
    pub(crate) tbl_map: HashMap<u8, String>,
    pub(crate) tbl_path: Option<String>,
    pub(crate) search_query: String,
    pub(crate) search_mode: MemorySearchMode,
    pub(crate) search_results: Vec<RomSearchResult>,
    pub(crate) search_max_results: usize,
    pub(crate) search_pending: bool,
    pub(crate) inspector_addr_input: String,
    pub(crate) inspector_addr: Option<u32>,
}

impl RomViewerState {
    pub(crate) fn new() -> Self {
        Self {
            view_start: 0,
            jump_input: String::from("000000"),
            rom_size: 0,
            tbl_map: HashMap::new(),
            tbl_path: None,
            search_query: String::new(),
            search_mode: MemorySearchMode::ByteValue,
            search_results: Vec::new(),
            search_max_results: 256,
            search_pending: false,
            inspector_addr_input: String::new(),
            inspector_addr: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gba_memory_viewer_uses_wide_default_start() {
        let mut state = MemoryViewerState::new();
        state.configure_for_system(ActiveSystem::GameBoyAdvance);
        assert_eq!(state.view_start, 0x0200_0000);
        assert_eq!(state.jump_input, "02000000");
        assert_eq!(state.address_space.addr_width, 8);
    }

    #[test]
    fn gb_memory_viewer_uses_16_bit_space() {
        let mut state = MemoryViewerState::new();
        state.configure_for_system(ActiveSystem::GameBoyAdvance);
        state.configure_for_system(ActiveSystem::GameBoy);
        assert_eq!(state.view_start, 0);
        assert_eq!(state.jump_input, "0000");
        assert_eq!(state.address_space.max_start, 0xFF00);
    }
}
