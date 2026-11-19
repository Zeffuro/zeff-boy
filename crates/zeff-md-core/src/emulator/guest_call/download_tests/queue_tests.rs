use super::*;

fn caller_for(count: usize) -> u32 {
    START + 8 + count as u32 * 8 + 6
}

fn written_target(
    target: u32,
    callee: &[u16],
    region: Region,
    concurrent: bool,
) -> (Emulator, u32) {
    let caller = caller_for(callee.len());
    let mut program = vec![0x13fc, 0, 0xa1, 0x30f1];
    for (index, value) in callee.iter().enumerate() {
        let address = target + index as u32 * 2;
        program.extend_from_slice(&[0x33fc, *value, (address >> 16) as u16, address as u16]);
    }
    program.extend_from_slice(&[
        0x4ef9,
        (caller >> 16) as u16,
        caller as u16,
        0x7001,
        0x4ef9,
        (caller >> 16) as u16,
        caller as u16,
    ]);
    (machine(image(START, &program), region, concurrent), caller)
}

fn masks(target: u32, caller: u32, value: u8) -> Vec<CheatPatch> {
    let mut result = vec![patch(caller + 1, value)];
    if target + 1 < 0x300000 {
        result.push(patch(target + 1, value));
    }
    if target + 5 < 0x300000 {
        result.push(patch(target + 5, value + 4));
    }
    result
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_guest_written_targets_keep_historical_call_samples_and_current_return_refill() {
    let caller = caller_for(13);
    let callee = [
        0x7001,
        0x7202,
        0x7403,
        0x33fc,
        0x7005,
        (caller >> 16) as u16,
        caller as u16,
        0x33fc,
        0x4ef9,
        (caller >> 16) as u16,
        (caller + 2) as u16,
        0x4e75,
        0x4e71,
    ];
    for region in REGIONS {
        for concurrent in [false, true] {
            for target in [0x60000, 0x2ffffe, 0x300000] {
                for transition in 0..4 {
                    let (mut actual, pc) = written_target(target, &callee, region, concurrent);
                    assert_eq!(pc, caller);
                    let initial = if transition == 0 {
                        vec![]
                    } else {
                        masks(target, caller, 7)
                    };
                    actual.install_rom_patches(&initial).unwrap();
                    for _ in 0..callee.len() + 2 {
                        actual.step_instruction().unwrap();
                    }
                    assert_eq!(actual.registers().pc, caller);
                    let historical = if initial.is_empty() { 0x7001 } else { 0x7007 };
                    assert_eq!(actual.queued_words(), Some([historical, 0x4ef9]));
                    for (index, value) in callee.iter().enumerate() {
                        let address = target + index as u32 * 2;
                        let board = actual.bus.cartridge.download.as_ref().unwrap();
                        assert_eq!(board.read(address), Some((value >> 8) as u8));
                        assert_eq!(board.read(address + 1), Some(*value as u8));
                    }
                    let before = actual.clone();
                    let entry = match transition {
                        1 => masks(target, caller, 7),
                        2 => masks(target, caller, 9),
                        _ => vec![],
                    };
                    let historical_queue = actual.cpu.queued_instruction_inputs();
                    actual.install_rom_patches(&entry).unwrap();
                    assert_eq!(actual.cpu.queued_instruction_inputs(), historical_queue);
                    assert_eq!(
                        actual.encode_state().unwrap(),
                        before.encode_state().unwrap()
                    );
                    let before = actual.clone();
                    assert_eq!(actual.check_debug_call(target), Ok(()));
                    same(&actual, &before);
                    let context = actual.begin_debug_call(target).unwrap();
                    let sampled = if target < 0x300000 && transition == 1 {
                        0x7007
                    } else if target < 0x300000 && transition == 2 {
                        0x7009
                    } else {
                        0x7001
                    };
                    assert_eq!(actual.queued_words(), Some([sampled, 0x7202]));
                    unknown_queue(&actual);
                    let queued = actual.cpu.queued_instruction_inputs();
                    let raw = actual.encode_state().unwrap();
                    let mut current = entry.clone();
                    if target < 0x300000 && !current.is_empty() {
                        current.push(patch(target + 1, 21));
                    }
                    actual.install_rom_patches(&current).unwrap();
                    assert_eq!(actual.cpu.queued_instruction_inputs(), queued);
                    assert_eq!(actual.encode_state().unwrap(), raw);
                    if target < 0x300000 && !current.is_empty() {
                        assert_eq!(actual.peek(target + 1), Some(21));
                    }
                    actual.step_instruction().unwrap();
                    assert_eq!(actual.registers().d[0], u32::from(sampled & 0xff));
                    actual.step_instruction().unwrap();
                    assert_eq!(actual.registers().d[1], 2);
                    actual.step_instruction().unwrap();
                    assert_eq!(
                        actual.registers().d[2],
                        if target == 0x60000 && transition == 1 {
                            11
                        } else if target == 0x60000 && transition == 2 {
                            13
                        } else {
                            3
                        }
                    );
                    actual.install_rom_patches(&entry).unwrap();
                    assert_eq!(
                        actual.bus.cartridge.rom_overlay,
                        before.bus.cartridge.rom_overlay
                    );
                    for _ in 0..3 {
                        actual.step_instruction().unwrap();
                    }
                    assert!(actual.debug_call_returned(&context));
                    let returned = match transition {
                        1 => 0x7007,
                        2 => 0x7009,
                        _ => 0x7005,
                    };
                    assert_eq!(actual.queued_words(), Some([returned, 0x4ef9]));
                    let queue = actual.cpu.queued_instruction_inputs();
                    let bus = actual.bus.clone();
                    actual.finish_debug_call(context).unwrap();
                    assert_eq!(actual.bus, bus);
                    assert_eq!(actual.cpu.queued_instruction_inputs(), queue);
                    unknown_queue(&actual);
                    assert_eq!(actual.rom_offset_for_cpu_address(target), None);
                    let mut exposed = actual.clone();
                    exposed.clear_rom_patches();
                    assert_eq!(exposed.peek(caller), Some(0x70));
                    assert_eq!(exposed.peek(caller + 1), Some(5));
                    actual.step_instruction().unwrap();
                    assert_eq!(actual.registers().d[0], u32::from(returned & 0xff));
                    let before = actual.clone();
                    assert!(actual.install_rom_patches(&[patch(0x300000, 7)]).is_err());
                    same(&actual, &before);
                    assert!(!actual.supports_rom_breakpoints());
                }
            }
            let (mut last, caller) =
                written_target(0x3ffffc, &[0x4e75, 0x4e71], region, concurrent);
            for _ in 0..4 {
                last.step_instruction().unwrap();
            }
            assert_eq!(last.registers().pc, caller);
            let context = last.begin_debug_call(0x3ffffc).unwrap();
            assert_eq!(last.queued_words(), Some([0x4e75, 0x4e71]));
            last.step_instruction().unwrap();
            assert!(last.debug_call_returned(&context));
            last.finish_debug_call(context).unwrap();
            assert_eq!(last.registers().pc, caller);
            unknown_queue(&last);
        }
    }
}
