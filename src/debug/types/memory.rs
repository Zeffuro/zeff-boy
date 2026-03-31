use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemorySearchMode {
    ByteValue,
    ByteSequence,
    AsciiString,
}

#[derive(Clone)]
pub(crate) struct MemorySearchResult {
    pub(crate) address: u16,
    pub(crate) matched_bytes: Vec<u8>,
}

pub(crate) struct MemoryViewerState {
    pub(crate) view_start: u16,
    pub(crate) jump_input: String,
    pub(crate) prev_start: Option<u16>,
    pub(crate) prev_bytes: Vec<u8>,
    pub(crate) flash_ticks: Vec<u8>,
    pub(crate) edit_addr: Option<u16>,
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
}

impl MemoryViewerState {
    pub(crate) fn new() -> Self {
        Self {
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
        }
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
        }
    }
}
