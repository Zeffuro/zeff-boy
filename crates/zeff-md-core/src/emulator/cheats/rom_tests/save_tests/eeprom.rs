use super::*;

fn line(code: &mut Vec<u16>, previous: &mut u8, value: u8) {
    if *previous & 2 != 0 && value & 2 == 0 {
        write_byte(code, 0x200000, 0xa4);
    }
    if *previous & 1 != value & 1 {
        write_byte(code, PORT, 0xa4 | (value & 1));
    }
    if *previous & 2 == 0 && value & 2 != 0 {
        write_byte(code, 0x200000, 0xa5);
    }
    *previous = value;
}

fn send(code: &mut Vec<u16>, previous: &mut u8, value: u8) {
    for bit in (0..8).rev() {
        let data = (value >> bit) & 1;
        for levels in [data, data | 2, data] {
            line(code, previous, levels);
        }
    }
    for levels in [1, 3, 1] {
        line(code, previous, levels);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn split_eeprom_guest_transaction_restore_preserves_protocol_and_hidden_source() {
    let mut code = vec![0x33fc, 0xa400, 0x0020, 0];
    let mut previous = 3;
    for levels in [1, 3, 2, 0] {
        line(&mut code, &mut previous, levels);
    }
    send(&mut code, &mut previous, 0xa0);
    send(&mut code, &mut previous, 3);
    let cut = 0xff1000 + code.len() as u32 * 2;
    send(&mut code, &mut previous, 0x5a);
    for levels in [0, 2, 3] {
        line(&mut code, &mut previous, levels);
    }
    code.extend([
        0x1039, 0x0020, 1, 0x33fc, 0xa401, 0x0020, 0, 0x1239, 0x0020, 1,
    ]);
    let done = 0xff1000 + code.len() as u32 * 2;
    code.extend([0x60fe, 0x4e71]);
    for region in REGIONS {
        for concurrent in [false, true] {
            let mut m = machine(Profile::Split256, &code, region, concurrent);
            m.install_rom_patches(&[patch(PORT, 0xa5)]).unwrap();
            for _ in 0..1000 {
                if m.registers().pc == cut {
                    break;
                }
                m.step_instruction().unwrap();
            }
            assert_eq!(m.registers().pc, cut);
            assert_eq!(m.dump_save_ram().unwrap(), vec![0xff; 256]);
            assert_ne!(m.peek(PORT), Some(0xa5));
            let raw = m.encode_state().unwrap();
            let front = m.encode_frontend_state().unwrap();
            for current in [Some(0xa5), Some(0xc7), None] {
                for frontend in [false, true] {
                    let mut receiver = m.clone();
                    receiver.clear_rom_patches();
                    if let Some(value) = current {
                        receiver.install_rom_patches(&[patch(PORT, value)]).unwrap();
                    }
                    restore(&mut receiver, &raw, &front, frontend);
                    assert_ne!(receiver.peek(PORT), current);
                    for _ in 0..1000 {
                        if receiver.registers().pc == done {
                            break;
                        }
                        receiver.step_instruction().unwrap();
                    }
                    assert_eq!(receiver.registers().pc, done);
                    let mut cells = vec![0xff; 256];
                    cells[3] = 0x5a;
                    assert_eq!(receiver.dump_save_ram(), Some(cells.clone()));
                    assert_eq!(receiver.registers().d[0] & 0xff, 1);
                    assert_eq!(
                        receiver.registers().d[1] & 0xff,
                        u32::from(current.unwrap_or(0x11))
                    );
                    receiver.reset();
                    assert_eq!(receiver.dump_save_ram(), Some(cells));
                    assert_eq!(receiver.peek(PORT), Some(current.unwrap_or(0x11)));
                }
            }
        }
    }
}
