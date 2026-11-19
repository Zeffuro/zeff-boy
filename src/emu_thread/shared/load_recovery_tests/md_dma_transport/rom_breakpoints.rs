use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_rom_breakpoint_keeps_dma_stop_information_at_the_real_service_boundary() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        for trace in [false, true] {
            let mut reference = before_arm(b'U', timing, &[0x4e71, 0x60fe, 0x4e71]);
            let (pc, opcode) = first_external_boundary(&mut reference);
            assert!(pc <= 0x260, "DMA witness repeated the terminal loop");
            let mut backend = before_arm(b'U', timing, &[0x4e71, 0x60fe, 0x4e71]);
            let mut actions = dma_actions(true, trace);
            actions.add_rom_breakpoints.push(u64::from(pc));
            let result = crate::emu_thread::shared::md_pc_breakpoint_tests::deliver(
                &mut backend,
                1,
                actions,
                false,
                false,
                &mut WorkerRuntimeFault::default(),
            );
            assert!(result.runtime_fault.is_none());
            let cpu = result.ui_data.cpu_debug.unwrap();
            assert_eq!(cpu.hit_rom_breakpoint, Some(u64::from(pc)));
            assert_eq!(cpu.hit_event, Some(DebugEvent::Dma));
            assert_eq!(cpu.recent_opcodes[0].bytes, opcode);
            assert_eq!(cpu.recent_opcodes[0].storage_offset, Some(u64::from(pc)));
            assert_eq!(
                backend.encode_state_bytes().unwrap(),
                reference.encode_state_bytes().unwrap()
            );
        }
    }
}
