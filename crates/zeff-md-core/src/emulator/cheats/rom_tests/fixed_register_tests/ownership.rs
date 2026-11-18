use super::*;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::ClockedBus;

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & 0xfffffe,
        function_code: FunctionCode::SupervisorProgram,
        kind: AccessKind::Prefetch,
        lanes,
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn fixed_register_constants_and_latches_never_gain_equal_rom_origin_or_new_lanes() {
    for (profile, cells) in [
        (
            0,
            vec![
                (0x400000, 0x55),
                (0x400002, 0x0f),
                (0x400004, 0xc9),
                (0x400006, 0x18),
            ],
        ),
        (
            1,
            vec![
                (0x400000, 0x55),
                (0x400002, 0x0f),
                (0x400004, 0xaa),
                (0x400006, 0xf0),
            ],
        ),
        (2, vec![(0x400000, 0x55), (0x400002, 0x0f)]),
        (
            3,
            vec![
                (0x400000, 0x63),
                (0x400002, 0x98),
                (0x400004, 0xc9),
                (0x400006, 0x18),
            ],
        ),
        (4, vec![(0x400000, 0x90), (0x401000, 0xd3)]),
    ] {
        for (address, value) in cells {
            for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
                let mut machine = machine(profile, true, &[0x60fe, 0x4e71]);
                machine.install_rom_patches(&[patch(DATA, value)]).unwrap();
                let rom = machine.bus.cartridge.read_sampled(DATA).unwrap();
                assert_eq!(rom.value, value);
                assert_eq!(rom.origin.storage_offset(), Some(0x18000));
                let target = address + u32::from(lanes == ByteLanes::Lower);
                let clock = machine.bus.cpu_clock;
                let result = machine
                    .bus
                    .read_instruction_cycle(access(target, lanes), clock)
                    .result;
                if lanes == ByteLanes::Lower || profile == 4 && lanes == ByteLanes::Both {
                    assert!(result.is_err());
                } else {
                    let input = result.unwrap();
                    assert_eq!(
                        input.value,
                        u16::from(value) << if lanes == ByteLanes::Upper { 8 } else { 0 }
                    );
                    assert_eq!(input.origin, [InstructionByteOrigin::UNKNOWN; 2]);
                }
                assert_eq!(machine.peek(address), Some(value));
                assert_eq!(machine.peek(address + 1), None);
                assert_eq!(machine.rom_offset_for_cpu_address(address), None);
                let before = machine.bus.cartridge.clone();
                let clock = machine.bus.cpu_clock;
                let result = machine
                    .bus
                    .write_cycle(access(target, lanes), 0xffff, clock)
                    .result;
                assert_eq!(result.is_err(), profile == 4);
                assert_eq!(machine.bus.cartridge, before);
            }
        }
        for hole in if profile == 4 {
            [0x400002, 0x400006, 0x401002, 0x40fffe]
        } else if profile == 2 {
            [0x400004, 0x400006, 0x400008, 0x40fffe]
        } else {
            [0x400008, 0x400010, 0x400100, 0x40fffe]
        } {
            let mut machine = machine(profile, true, &[0x60fe, 0x4e71]);
            machine.install_rom_patches(&[patch(DATA, 0x55)]).unwrap();
            assert_eq!(machine.peek(hole), None);
            let clock = machine.bus.cpu_clock;
            assert!(
                machine
                    .bus
                    .read_instruction_cycle(access(hole, ByteLanes::Both), clock)
                    .result
                    .is_err()
            );
            assert_eq!(machine.rom_offset_for_cpu_address(hole), None);
        }
    }
    for profile in 5..9 {
        for (address, aliases) in [
            (0x400000, [0x400000, 0x400002]),
            (0x400004, [0x400004, 0x400006]),
        ] {
            for lanes in [ByteLanes::Upper, ByteLanes::Both] {
                let mut machine = machine(profile, true, &[0x60fe, 0x4e71]);
                machine.install_rom_patches(&[patch(DATA, 0x55)]).unwrap();
                let clock = machine.bus.cpu_clock;
                machine
                    .bus
                    .write_cycle(
                        access(address, lanes),
                        if lanes == ByteLanes::Upper {
                            0x5500
                        } else {
                            0x55
                        },
                        clock,
                    )
                    .result
                    .unwrap();
                assert!(machine.supports_rom_cheats());
                for alias in aliases {
                    let clock = machine.bus.cpu_clock;
                    let input = machine
                        .bus
                        .read_instruction_cycle(access(alias, lanes), clock)
                        .result
                        .unwrap();
                    assert_eq!(
                        input.value,
                        if lanes == ByteLanes::Upper {
                            0x5500
                        } else {
                            0x55
                        }
                    );
                    assert_eq!(input.origin, [InstructionByteOrigin::UNKNOWN; 2]);
                    assert_eq!(machine.peek(alias), Some(0x55));
                    assert_eq!(machine.peek(alias + 1), None);
                    assert_eq!(machine.rom_offset_for_cpu_address(alias), None);
                    let clock = machine.bus.cpu_clock;
                    assert!(
                        machine
                            .bus
                            .read_instruction_cycle(access(alias + 1, ByteLanes::Lower), clock)
                            .result
                            .is_err()
                    );
                }
                for hole in [0x400008, 0x40fffe, 0x410000] {
                    let clock = machine.bus.cpu_clock;
                    assert!(
                        machine
                            .bus
                            .read_instruction_cycle(access(hole, ByteLanes::Both), clock)
                            .result
                            .is_err()
                    );
                    assert_eq!(machine.peek(hole), None);
                }
                assert_eq!(
                    machine
                        .bus
                        .cartridge
                        .read_sampled(DATA)
                        .unwrap()
                        .origin
                        .storage_offset(),
                    Some(0x18000)
                );
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn or_bank_unsealed_consumer_selects_physical_samples_and_refuses_owned_admission() {
    for concurrent in [false, true] {
        consumer::callee(concurrent, true);
    }
}
