use super::*;
use crate::debug::{DebugTarget, DisassemblyTarget};

#[derive(Default)]
pub(crate) struct TargetNavigation {
    target: Option<DisassemblyTarget>,
    back: Vec<Option<DisassemblyTarget>>,
    forward: Vec<Option<DisassemblyTarget>>,
    source: SourceViewerState,
    browser: SymbolBrowserState,
    coverage: ExecutionCoverage,
    cpu: CpuDebugViewState,
}

impl DebugWindowState {
    pub(crate) fn reset_target_sessions(&mut self) {
        self.debug_target = DebugTarget::Primary;
        self.debug_revision = self.debug_revision.saturating_add(1);
        self.other_target = TargetNavigation::default();
        self.disasm_target = None;
        self.disasm_back.clear();
        self.disasm_forward.clear();
        self.last_disasm_pc = None;
        self.last_disasm_mapping = None;
        self.source_viewer = SourceViewerState::default();
        self.symbol_browser = SymbolBrowserState::default();
        self.execution_coverage.reset_backend();
    }

    pub(crate) fn select_debug_target(&mut self, target: DebugTarget) -> bool {
        if target == self.debug_target {
            return true;
        }
        let Some(revision) = self.debug_revision.checked_add(1) else {
            return false;
        };
        self.debug_revision = revision;
        self.debug_target = target;
        let other = &mut self.other_target;
        std::mem::swap(&mut self.disasm_target, &mut other.target);
        std::mem::swap(&mut self.disasm_back, &mut other.back);
        std::mem::swap(&mut self.disasm_forward, &mut other.forward);
        std::mem::swap(&mut self.source_viewer, &mut other.source);
        std::mem::swap(&mut self.symbol_browser, &mut other.browser);
        std::mem::swap(&mut self.execution_coverage, &mut other.coverage);
        std::mem::swap(&mut self.cpu_view, &mut other.cpu);
        self.last_disasm_pc = None;
        self.last_disasm_mapping = None;
        true
    }

    pub(crate) fn merge_primary_coverage(
        &mut self,
        batch: &crate::ui::InstructionTraceBatch,
        symbols: &crate::symbols::SymbolSession,
    ) {
        if self.debug_target == DebugTarget::Primary {
            self.execution_coverage.merge(batch, symbols);
        } else {
            self.other_target.coverage.merge(batch, symbols);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn target_selection_preserves_independent_navigation_and_revision() {
        let mut state = DebugWindowState::new();
        let main = Some(DisassemblyTarget::Cpu(
            crate::debug::CpuDisassemblyTarget::m68000(0x200),
        ));
        state.navigate_disassembly(main);
        assert!(state.select_debug_target(DebugTarget::Sound));
        assert!(state.disasm_target.is_none() && state.disasm_back.is_empty());
        let sound = Some(DisassemblyTarget::Cpu(crate::debug::CpuDisassemblyTarget {
            cpu_address: 0x201,
            storage_offset: None,
            bank: None,
            thumb: None,
        }));
        state.navigate_disassembly(sound);
        assert!(state.select_debug_target(DebugTarget::Primary));
        assert_eq!(state.disasm_target, main);
        assert!(state.select_debug_target(DebugTarget::Sound));
        assert_eq!(state.disasm_target, sound);
        assert_eq!(state.debug_revision, 3);
        state.debug_revision = u64::MAX;
        assert!(!state.select_debug_target(DebugTarget::Primary));
        assert_eq!(state.debug_target, DebugTarget::Sound);
        state.reset_target_sessions();
        assert_eq!(state.debug_target, DebugTarget::Primary);
        assert_eq!(state.debug_revision, u64::MAX);
        assert!(state.disasm_target.is_none());
        assert!(state.disasm_back.is_empty() && state.disasm_forward.is_empty());
        assert!(!state.select_debug_target(DebugTarget::Sound));
        state.reset_target_sessions();
        assert_eq!(state.debug_target, DebugTarget::Primary);
        assert_eq!(state.debug_revision, u64::MAX);
    }
}
