use super::*;
use crate::emulator::InstructionObserver;
use zeff_m68k::timing::{InstructionByteOrigin, InstructionInput};

mod consumer;
mod ownership;
mod pending;
mod state;

const PROFILES: std::ops::Range<usize> = 0..10;

fn machine(profile: usize, concurrent_sound: bool, program: &[u16]) -> Emulator {
    let template = image(program);
    let words: Vec<_> = template[0x200..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();
    Emulator::new_with_cartridge(
        crate::cartridge::fixed_register_fixture::cartridge(profile, &words),
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound,
        },
    )
    .unwrap()
}

fn set(opcode: u8, data: u8) -> [CheatPatch; 2] {
    [patch(CODE + 1, opcode), patch(DATA, data)]
}

fn offsets(input: InstructionInput) -> [Option<u64>; 2] {
    input.origin.map(InstructionByteOrigin::storage_offset)
}

#[derive(Default)]
struct Capture(Vec<(u32, InstructionInput)>);

impl InstructionObserver for Capture {
    fn instruction_input(&mut self, address: u32, input: InstructionInput) {
        self.0.push((address, input));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn fixed_register_rom_main_lanes_identity_and_atomic_admission() {
    let program = [
        0x1039, 0x0101, 0x8000, 0x1239, 0x0101, 0x8001, 0x3439, 1, 0x8000, 0x60fe,
    ];
    for profile in PROFILES {
        for concurrent in [false, true] {
            for (text, expected) in [
                (Some("018000:A55A"), [0xa5, 0x5a, 0xa55a]),
                (Some("018000:96E1"), [0x96, 0xe1, 0x96e1]),
                (None, [0x12, 0x34, 0x1234]),
            ] {
                let mut machine = machine(profile, concurrent, &program);
                let source = machine.bus.cartridge.bytes.clone();
                let digest = machine.bus.cartridge.rom_digest();
                let header = machine.bus.cartridge.header.clone();
                let initial = machine.bus.cartridge.initial;
                if let Some(text) = text {
                    if profile == 9 {
                        consumer::overlay(&mut machine, &patches(text));
                    } else {
                        machine.install_rom_patches(&patches(text)).unwrap();
                    }
                }
                assert_eq!(machine.supports_rom_cheats(), profile != 9);
                let mut capture = Capture::default();
                machine.step_instruction_traced(&mut capture).unwrap();
                assert_eq!(capture.0[0].0, 0x200);
                assert_eq!(capture.0[0].1.value, 0x1039);
                assert_eq!(offsets(capture.0[0].1), [Some(0x200), Some(0x201)]);
                for _ in 0..2 {
                    machine.step_instruction().unwrap();
                }
                assert_eq!(machine.registers().d[..3], expected);
                assert!(Arc::ptr_eq(&source, &machine.bus.cartridge.bytes));
                assert_eq!(machine.bus.cartridge.rom_digest(), digest);
                assert_eq!(machine.bus.cartridge.header, header);
                assert_eq!(machine.bus.cartridge.initial, initial);
                assert_eq!(
                    machine.rom_offset_for_cpu_address(0x01018000),
                    (profile != 9).then_some(0x18000)
                );
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
                assert_eq!(machine.rom_offset_for_cpu_address(0x400000), None);
                assert_eq!(machine.rom_offset_for_cpu_address(0x700001), None);
                assert!(machine.dump_save_ram().is_none());
                assert!(machine.check_warm_reset().is_err());
                assert!(
                    crate::hardware::bus::Bus::new_owned(
                        machine.bus.cartridge.clone(),
                        Region::NtscUs
                    )
                    .is_err()
                );
                let before = machine.clone();
                let owned = if profile == 9 {
                    consumer::refusals(&mut machine);
                    None
                } else {
                    Some(machine.encode_debugger_checkpoint().unwrap())
                };
                for bad in [
                    source.len() as u32,
                    0x400000,
                    0x700001,
                    0xa13000,
                    0xff0000,
                    u32::MAX,
                ] {
                    assert!(
                        machine
                            .install_rom_patches(&[patch(DATA, 0xee), patch(bad, 1)])
                            .is_err()
                    );
                    assert_eq!(machine, before);
                    if let Some(owned) = &owned {
                        assert_eq!(&machine.encode_debugger_checkpoint().unwrap(), owned);
                    } else {
                        consumer::refusals(&mut machine);
                    }
                }
            }
            if profile != 9 {
                let mut stripped = machine(profile, concurrent, &[0x60fe, 0x4e71]);
                stripped.bus.cartridge.registers = None;
                stripped.bus.cartridge.protection = None;
                let before = stripped.clone();
                assert!(!stripped.supports_rom_cheats());
                assert!(stripped.install_rom_patches(&set(7, 0xa5)).is_err());
                assert_eq!(stripped, before);
                let mut wrong = machine(profile, concurrent, &[0x60fe, 0x4e71]);
                if profile < 5 {
                    wrong.bus.cartridge.protection =
                        crate::cartridge::fixed_register_fixture::cartridge(
                            (profile + 1) % 5,
                            &[0x60fe, 0x4e71],
                        )
                        .protection;
                } else {
                    wrong.bus.cartridge.registers =
                        crate::cartridge::registers::bank_tests::fixture().registers;
                }
                let before = wrong.clone();
                assert!(!wrong.supports_rom_cheats());
                assert!(wrong.install_rom_patches(&set(7, 0xa5)).is_err());
                assert_eq!(wrong, before);
            }
            let mut conflict = machine(profile, concurrent, &[0x60fe, 0x4e71]);
            conflict.bus.cartridge.save = Some(crate::cartridge::ram::headerless_odd8_fixture());
            let before = conflict.clone();
            assert!(!conflict.supports_rom_cheats());
            assert!(conflict.install_rom_patches(&set(7, 0xa5)).is_err());
            assert_eq!(conflict, before);
            let mut detached = machine(profile, concurrent, &[0x60fe, 0x4e71]);
            detached.bus.cartridge.bytes = detached.bus.cartridge.bytes.to_vec().into();
            let before = detached.clone();
            assert!(!detached.supports_rom_cheats());
            assert!(detached.install_rom_patches(&set(7, 0xa5)).is_err());
            assert_eq!(detached, before);
            let mut modified = machine(profile, concurrent, &[0x60fe, 0x4e71]);
            Arc::make_mut(&mut modified.bus.cartridge.bytes)[DATA as usize] ^= 1;
            assert!(!modified.supports_rom_cheats());
            let before = modified.clone();
            assert!(modified.install_rom_patches(&set(7, 0xa5)).is_err());
            assert_eq!(modified, before);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn fixed_register_rom_guest_callee_consumes_sampled_a_and_owned_undo_keeps_current_b() {
    for profile in PROFILES {
        for concurrent in [false, true] {
            if profile == 9 {
                consumer::callee(concurrent, false);
                continue;
            }
            let mut machine = machine(profile, concurrent, &[0x60fe, 0x4e71]);
            let mut a = set(7, 0xa5).to_vec();
            a.extend(patches("010106:4E75"));
            machine.install_rom_patches(&a).unwrap();
            let saved = machine.encode_debugger_checkpoint().unwrap();
            let queued = machine.cpu.queued_instruction_inputs();
            let context = machine.begin_debug_call(CODE).unwrap();
            let admitted = machine.cpu.queued_instruction_inputs().unwrap()[0];
            assert_eq!(admitted.value, 0x7007);
            assert_eq!(offsets(admitted), [Some(0x10100), Some(0x10101)]);
            let mut b = set(9, 0x5a).to_vec();
            b.extend(patches("010106:4E75"));
            machine.install_rom_patches(&b).unwrap();
            let mut capture = Capture::default();
            for _ in 0..3 {
                machine.step_instruction_traced(&mut capture).unwrap();
            }
            assert!(machine.debug_call_returned(&context));
            machine.finish_debug_call(context).unwrap();
            assert_eq!(machine.registers().d[0], 10);
            assert_eq!(machine.registers().pc, 0x200);
            assert_eq!(capture.0[0], (CODE, admitted));
            assert_eq!(
                capture
                    .0
                    .iter()
                    .map(|(address, _)| *address)
                    .collect::<Vec<_>>(),
                [CODE, CODE + 2, CODE + 4, CODE + 6]
            );
            assert_eq!(offsets(capture.0[3].1), [Some(0x10106), Some(0x10107)]);
            machine.decode_debugger_checkpoint(&saved).unwrap();
            assert_eq!(machine.cpu.queued_instruction_inputs(), queued);
            assert_eq!(machine.peek(CODE + 1), Some(9));
            assert_eq!(machine.peek(DATA), Some(0x5a));
            assert_eq!(machine.registers().d[0], 0);
            let context = machine.begin_debug_call(CODE).unwrap();
            for _ in 0..3 {
                machine.step_instruction().unwrap();
            }
            assert!(machine.debug_call_returned(&context));
            machine.finish_debug_call(context).unwrap();
            assert_eq!(machine.registers().d[0], 12);
        }
    }
}
