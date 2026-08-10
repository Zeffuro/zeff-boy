use crate::debug;
use crate::emu_thread::MemorySearchRequest;

pub(super) fn parse_pending_search(
    state: &mut impl SearchableState,
) -> Option<MemorySearchRequest> {
    if !state.search_pending() {
        return None;
    }
    state.set_search_pending(false);
    debug::hex_search::parse_search_query(state.search_query(), state.search_mode()).map(
        |pattern| MemorySearchRequest {
            pattern,
            max_results: state.search_max_results(),
        },
    )
}

pub(super) trait SearchableState {
    fn search_pending(&self) -> bool;
    fn set_search_pending(&mut self, v: bool);
    fn search_query(&self) -> &str;
    fn search_mode(&self) -> crate::debug::MemorySearchMode;
    fn search_max_results(&self) -> usize;
}

impl SearchableState for crate::debug::MemoryViewerState {
    fn search_pending(&self) -> bool {
        self.search_pending
    }
    fn set_search_pending(&mut self, v: bool) {
        self.search_pending = v;
    }
    fn search_query(&self) -> &str {
        &self.search_query
    }
    fn search_mode(&self) -> crate::debug::MemorySearchMode {
        self.search_mode
    }
    fn search_max_results(&self) -> usize {
        self.search_max_results
    }
}

impl SearchableState for crate::debug::RomViewerState {
    fn search_pending(&self) -> bool {
        self.search_pending
    }
    fn set_search_pending(&mut self, v: bool) {
        self.search_pending = v;
    }
    fn search_query(&self) -> &str {
        &self.search_query
    }
    fn search_mode(&self) -> crate::debug::MemorySearchMode {
        self.search_mode
    }
    fn search_max_results(&self) -> usize {
        self.search_max_results
    }
}
