use super::*;

const CALLER: u32 = 0x60008;

fn mask(value: u8) -> Vec<CheatPatch> {
    vec![
        patch(CALLER + 1, value),
        patch(0x1234, value + 0x70),
        patch(0x1235, 0x88),
    ]
}

pub(super) fn donor(region: Region, concurrent: bool) -> Emulator {
    let mut bytes = image(
        START,
        &[
            0x13fc, 0x85, 0xc0, 0x11, 0x13fc, 6, 0xc0, 0x11, 0x13fc, 0x90, 0xc0, 0x11, 0x13fc, 0,
            0xa1, 0x30f1, 0x33fc, 0xa55a, 0, 0x1234, 0x4eb9, 6, 0, 0x60fe, 0x4e71,
        ],
    );
    words(&mut bytes, 0x60000, &[0x6106, 0x4e75, 0x4e71, 0x4e71]);
    words(&mut bytes, CALLER, &[0x7001, 0x4ef9, 6, 8]);
    let mut actual = machine(bytes, region, concurrent);
    actual.install_rom_patches(&mask(7)).unwrap();
    actual.set_call_stack_recording(true);
    for _ in 0..7 {
        actual.step_instruction().unwrap();
    }
    assert_eq!(actual.registers().pc, CALLER);
    assert_eq!(actual.queued_words(), Some([0x7007, 0x4ef9]));
    let frames = actual.call_stack().copied().collect::<Vec<_>>();
    assert_eq!(frames.len(), 2);
    assert_eq!(
        (
            frames[0].kind,
            frames[0].source_pc,
            frames[0].target_pc,
            frames[0].return_pc
        ),
        (ControlFlowEntryKind::Jsr, START + 40, 0x60000, START + 46)
    );
    assert_eq!(
        (
            frames[1].kind,
            frames[1].source_pc,
            frames[1].target_pc,
            frames[1].return_pc
        ),
        (ControlFlowEntryKind::Bsr, 0x60000, CALLER, 0x60002)
    );
    ram(
        &mut actual,
        RAM_TARGET,
        &[
            0x7633, 0x33fc, 0xccdd, 0, 0x1234, 0x13fc, 1, 0xa1, 0x30f1, 0x33fc, 0x1122, 0, 0x1234,
            0x33fc, 0xbeef, 0xff, 0x0400, 0x13fc, 0x8a, 0xc0, 0x11, 0x13fc, 2, 0xc0, 0x11, 0x13fc,
            0x9f, 0xc0, 0x11, 0x4e75, 0x4e71,
        ],
    );
    actual
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_actual_call_owned_undo_restores_nested_history_backing_control_and_pcm_with_current_mask()
 {
    for region in REGIONS {
        for concurrent in [false, true] {
            let source = donor(region, concurrent);
            let raw = source.encode_state().unwrap();
            let frontend = source.encode_frontend_state().unwrap();
            let checkpoint = source.encode_debugger_checkpoint().unwrap();
            let frames = source.call_stack().copied().collect::<Vec<_>>();
            for current in [mask(7), mask(9), vec![]] {
                let mut actual = source.clone();
                let context = actual.begin_debug_call(RAM_TARGET).unwrap();
                let entered = actual.call_stack().copied().collect::<Vec<_>>();
                assert_eq!(&entered[..2], frames.as_slice());
                assert_eq!(entered.len(), 3);
                let frame = entered[2];
                assert_eq!(
                    (
                        frame.kind,
                        frame.source_pc,
                        frame.target_pc,
                        frame.return_pc
                    ),
                    (ControlFlowEntryKind::Debugger, CALLER, RAM_TARGET, CALLER)
                );
                assert_eq!(frame.stack.before, source.registers().stack_pointer());
                assert_eq!(frame.stack.after, frame.stack.before - 4);
                assert_eq!(frame.stack.pc_slot, frame.stack.after);
                for opcode in [
                    0x7633, 0x33fc, 0x13fc, 0x33fc, 0x33fc, 0x13fc, 0x13fc, 0x13fc, 0x4e75,
                ] {
                    let step = actual.step_instruction().unwrap();
                    assert!(
                        matches!(step.cpu, TimedOutcome::Instruction(step) if step.opcode == opcode)
                    );
                }
                assert!(actual.debug_call_returned(&context));
                actual.finish_debug_call(context).unwrap();
                assert_eq!(actual.call_stack().copied().collect::<Vec<_>>(), frames);
                assert_eq!(actual.call_stack_status(), source.call_stack_status());
                assert_eq!(actual.registers().d[3], 0x33);
                assert_eq!(&actual.work_ram()[0x400..0x402], &[0xbe, 0xef]);
                assert_eq!(actual.psg_snapshot().periods[0], 0x2a);
                assert_eq!(actual.psg_snapshot().volumes[0], 15);
                assert_ne!(actual.bus.cartridge.download, source.bus.cartridge.download);
                let mut exposed = actual.clone();
                exposed.clear_rom_patches();
                assert_eq!(exposed.peek(0x1234), Some(0xcc));
                assert_eq!(exposed.peek(0x1235), Some(0xdd));
                assert!(actual.cpu_clock().0 > source.cpu_clock().0);
                actual.install_rom_patches(&current).unwrap();
                actual.decode_debugger_checkpoint(&checkpoint).unwrap();
                assert_eq!(actual.encode_state().unwrap(), raw);
                assert_eq!(actual.encode_frontend_state().unwrap(), frontend);
                assert_eq!(actual.encode_debugger_checkpoint().unwrap(), checkpoint);
                assert_eq!(actual.bus.cartridge.download, source.bus.cartridge.download);
                assert_eq!(
                    actual.cpu.queued_instruction_inputs(),
                    source.cpu.queued_instruction_inputs()
                );
                assert_eq!(actual.queued_words(), Some([0x7007, 0x4ef9]));
                assert_eq!(actual.psg_snapshot(), source.psg_snapshot());
                unknown_queue(&actual);
                let mut expected = source.clone();
                expected.install_rom_patches(&current).unwrap();
                same(&actual, &expected);
                actual.step_instruction().unwrap();
                expected.step_instruction().unwrap();
                assert_eq!(actual.registers().d[0], 7);
                same(&actual, &expected);
                for _ in 0..2 {
                    actual.step_instruction().unwrap();
                    expected.step_instruction().unwrap();
                    same(&actual, &expected);
                }
                let value = if current.is_empty() {
                    1
                } else if current == mask(7) {
                    7
                } else {
                    9
                };
                assert_eq!(actual.registers().d[0], value);
                future_field(&mut actual, &mut expected);
            }
        }
    }
}
