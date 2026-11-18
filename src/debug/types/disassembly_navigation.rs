use super::DebugWindowState;
use crate::debug::DisassemblyTarget;

impl DebugWindowState {
    pub(crate) fn navigate_disassembly(&mut self, target: Option<DisassemblyTarget>) {
        if self.disasm_target == target {
            return;
        }
        self.disasm_back.push(self.disasm_target);
        self.disasm_forward.clear();
        self.disasm_target = target;
        self.last_disasm_pc = None;
        self.last_disasm_mapping = None;
    }

    pub(crate) fn navigate_disassembly_history(&mut self, back: bool) {
        let (from, to) = if back {
            (&mut self.disasm_back, &mut self.disasm_forward)
        } else {
            (&mut self.disasm_forward, &mut self.disasm_back)
        };
        let Some(target) = from.pop() else {
            return;
        };
        to.push(self.disasm_target);
        self.disasm_target = target;
        self.last_disasm_pc = None;
        self.last_disasm_mapping = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::CpuDisassemblyTarget;

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn disassembly_history_preserves_source_cpu_and_follow_pc_domains() {
        let mut state = DebugWindowState::new();
        let cpu = Some(DisassemblyTarget::Cpu(CpuDisassemblyTarget {
            cpu_address: 0x200,
            storage_offset: None,
            bank: None,
            thumb: None,
        }));
        let source = Some(DisassemblyTarget::RomStorage(u64::MAX));
        state.navigate_disassembly(cpu);
        state.navigate_disassembly(source);
        state.navigate_disassembly(None);
        assert_eq!(state.disasm_back, [None, cpu, source]);
        state.navigate_disassembly_history(true);
        assert_eq!(state.disasm_target, source);
        state.navigate_disassembly_history(true);
        assert_eq!(state.disasm_target, cpu);
        state.navigate_disassembly_history(false);
        assert_eq!(state.disasm_target, source);
        state.navigate_disassembly(None);
        assert!(state.disasm_forward.is_empty());
        assert!(state.last_disasm_pc.is_none() && state.last_disasm_mapping.is_none());
    }
}
