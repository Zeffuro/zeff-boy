use super::*;

fn words(set: &mut Vec<crate::cheats::CheatPatch>, source: u32, words: &[u16]) {
    for (index, word) in words.iter().enumerate() {
        set.extend(codes(&format!(
            "{:06X}:{word:04X}",
            source + index as u32 * 2
        )));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_rom_breakpoints_use_queued_bank_origin_after_real_control_with_equal_bytes() {
    let mut backend = fixture(b'U');
    let mut patches = Vec::new();
    words(&mut patches, 0x200, &[0x4ef9, 0x0108, 0x0200]);
    for source in [0x080200, 0x480200] {
        words(
            &mut patches,
            source,
            &[0x13fc, 9, 0x00a1, 0x30f3, 0x4e71, 0x4e71, 0x60fe],
        );
    }
    assert!(update(&mut backend, &mut Vec::new(), patches).is_none());
    backend.reset();
    let mut stop = DebugUiActions::none();
    stop.add_breakpoint = Some(0x080200);
    let mut fault = WorkerRuntimeFault::default();
    let first = deliver(&mut backend, 1, stop, false, false, &mut fault);
    assert!(first.runtime_fault.is_none());
    assert_eq!(
        first.ui_data.cpu_debug.unwrap().hit_breakpoint,
        Some(0x080200)
    );
    let mut run = DebugUiActions::none();
    run.remove_breakpoints.push(0x080200);
    run.add_rom_breakpoints = vec![0x080208, 0x48020a];
    run.trace_enabled = Some(false);
    let old = deliver(&mut backend, 1, run, false, true, &mut fault);
    assert!(old.runtime_fault.is_none());
    let cpu = old.ui_data.cpu_debug.unwrap();
    assert_eq!(cpu.hit_rom_breakpoint, Some(0x080208));
    assert_eq!(cpu.recent_opcodes[0].storage_offset, Some(0x080208));
    assert_eq!(cpu.recent_opcodes[0].bytes, [0x4e, 0x71]);
    assert_eq!(
        machine(&backend).rom_offset_for_cpu_address(0x080208),
        Some(0x480208)
    );
    assert_eq!(machine(&backend).registers().pc & 0xffffff, 0x08020a);
    let next = deliver(
        &mut backend,
        1,
        DebugUiActions::none(),
        false,
        true,
        &mut fault,
    );
    assert!(next.runtime_fault.is_none());
    let cpu = next.ui_data.cpu_debug.unwrap();
    assert_eq!(cpu.hit_rom_breakpoint, Some(0x48020a));
    assert_eq!(cpu.recent_opcodes[0].storage_offset, Some(0x48020a));
    assert_eq!(cpu.recent_opcodes[0].bytes, [0x4e, 0x71]);
}
