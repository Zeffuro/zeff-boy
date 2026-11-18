use super::*;

fn md_editing(enabled: bool) -> MemoryEditingCapabilities {
    MemoryEditingCapabilities {
        ranges: zeff_md_core::emulator::DEBUG_MEMORY_RANGES,
        enabled,
        requires_pause: true,
    }
}

fn edit_state(address: u32) -> MemoryViewerState {
    let mut state = MemoryViewerState::new();
    state.enable_editing = true;
    state.edit_addr = Some(address);
    state.edit_value = "A5".into();
    state
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_memory_edit_action_accepts_only_canonical_ram() {
    for address in [0xff0000, 0xff0040, 0xffffff] {
        let mut state = edit_state(address);
        assert_eq!(
            commit_memory_edit(&mut state, Some(md_editing(true))),
            Some((address, 0xa5))
        );
        assert_eq!(state.edit_addr, None);
    }
    for address in [0, 0xa00000, 0xc00000, 0xfeffff, 0x1000000, u32::MAX] {
        let mut state = edit_state(address);
        assert_eq!(commit_memory_edit(&mut state, Some(md_editing(true))), None);
        assert_eq!(state.edit_addr, Some(address));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn memory_edit_action_rechecks_readiness_setting_and_input_at_commit() {
    let mut state = edit_state(0xff0040);
    assert_eq!(
        commit_memory_edit(&mut state, Some(md_editing(false))),
        None
    );
    assert_eq!(commit_memory_edit(&mut state, None), None);
    state.enable_editing = false;
    assert_eq!(commit_memory_edit(&mut state, Some(md_editing(true))), None);
    state.enable_editing = true;
    state.edit_value = "GG".into();
    assert_eq!(commit_memory_edit(&mut state, Some(md_editing(true))), None);
    assert_eq!(state.edit_addr, Some(0xff0040));
    state.edit_value = "00".into();
    assert_eq!(
        commit_memory_edit(&mut state, Some(md_editing(true))),
        Some((0xff0040, 0))
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn peer_memory_edit_action_preserves_running_full_cpu_range() {
    let editing = MemoryEditingCapabilities {
        ranges: &[(0, u32::MAX)],
        enabled: true,
        requires_pause: false,
    };
    for address in [0, 0xff0040, u32::MAX] {
        let mut state = edit_state(address);
        assert_eq!(
            commit_memory_edit(&mut state, Some(editing)),
            Some((address, 0xa5))
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_memory_edit_action_admits_exactly_six_parallel_register_bytes() {
    for address in 0xa10000..=0xa1001f {
        let mut state = edit_state(address);
        let expected = matches!(
            address,
            0xa10003 | 0xa10005 | 0xa10007 | 0xa10009 | 0xa1000b | 0xa1000d
        );
        assert_eq!(
            commit_memory_edit(&mut state, Some(md_editing(true))),
            expected.then_some((address, 0xa5))
        );
        let mut high = edit_state(address + 0x1000000);
        assert_eq!(commit_memory_edit(&mut high, Some(md_editing(true))), None);
        let mut paused = edit_state(address);
        assert_eq!(
            commit_memory_edit(&mut paused, Some(md_editing(false))),
            None
        );
    }
}
