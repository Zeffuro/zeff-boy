use super::*;
use crate::emulator::InstructionObserver;
use zeff_m68k::timing::{InstructionByteOrigin, InstructionInput};

mod pending;
mod state;

const SIZES: [usize; 2] = [0x80000, 0x100000];

fn machine(size: usize, concurrent_sound: bool, program: &[u16]) -> Emulator {
    let image = image(program);
    let words: Vec<_> = image[0x200..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();
    let cartridge = crate::cartridge::linear_rom::fixture_with_len(&words, size);
    assert!(cartridge.linear_rom_compatible());
    Emulator::new_with_cartridge(
        cartridge,
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
fn linear_rom_overlays_drive_literal_main_lanes_tail_and_keep_finite_admission() {
    for size in SIZES {
        let tail = size as u32 - 2;
        let program = [
            0x1039,
            0x0101,
            0x8000,
            0x1239,
            0x0101,
            0x8001,
            0x3439,
            1,
            0x8000,
            0x3639,
            (tail >> 16) as u16,
            tail as u16,
            0x1839,
            ((tail + 1) >> 16) as u16,
            (tail + 1) as u16,
            0x2a39,
            ((tail - 2) >> 16) as u16,
            (tail - 2) as u16,
            0x60fe,
            0x4e71,
        ];
        for concurrent in [false, true] {
            for (text, expected) in [
                (
                    Some(format!("018000:A55A + {tail:06X}:C73D")),
                    [0xa5, 0x5a, 0xa55a, 0xc73d, 0x3d, 0x1234c73d],
                ),
                (
                    Some(format!("018000:96E1 + {tail:06X}:4826")),
                    [0x96, 0xe1, 0x96e1, 0x4826, 0x26, 0x12344826],
                ),
                (None, [0x12, 0x34, 0x1234, 0x5678, 0x78, 0x12345678]),
            ] {
                let mut machine = machine(size, concurrent, &program);
                let source = machine.bus.cartridge.bytes.clone();
                let digest = machine.bus.cartridge.rom_digest();
                if let Some(text) = text {
                    machine.install_rom_patches(&patches(&text)).unwrap();
                }
                assert!(machine.supports_rom_cheats());
                assert_eq!(
                    machine.rom_offset_for_cpu_address(0x01018000),
                    Some(DATA as usize)
                );
                let mut capture = Capture::default();
                machine.step_instruction_traced(&mut capture).unwrap();
                assert_eq!(capture.0[0].0, 0x200);
                assert_eq!(capture.0[0].1.value, 0x1039);
                assert_eq!(offsets(capture.0[0].1), [Some(0x200), Some(0x201)]);
                for _ in 0..5 {
                    machine.step_instruction().unwrap();
                }
                assert_eq!(machine.registers().d[..6], expected);
                assert!(Arc::ptr_eq(&source, &machine.bus.cartridge.bytes));
                assert_eq!(machine.bus.cartridge.rom_digest(), digest);
                assert!(machine.bus.cartridge.read_sampled(size as u32).is_none());
                assert_eq!(machine.peek(size as u32), None);
                assert_eq!(machine.rom_offset_for_cpu_address(size as u32), None);
                assert_eq!(machine.peek(0xa13000), None);
                assert!(machine.dump_save_ram().is_none());
                assert!(machine.check_warm_reset().is_err());
                assert!(
                    crate::hardware::bus::Bus::new_owned(
                        machine.bus.cartridge.clone(),
                        Region::NtscUs
                    )
                    .is_err()
                );
                crate::hardware::vdp::VdpDmaSource::from_cartridge(
                    &machine.bus.cartridge,
                    &machine.bus.ram,
                )
                .assert_word(DATA, expected[2] as u16);
                let before = machine.clone();
                let owned = machine.encode_debugger_checkpoint().unwrap();
                for bad in [size as u32, 0xa13000, 0xff0000] {
                    assert!(
                        machine
                            .install_rom_patches(&[patch(DATA, 0xee), patch(bad, 1)])
                            .is_err()
                    );
                    assert_eq!(machine, before);
                    assert_eq!(machine.encode_debugger_checkpoint().unwrap(), owned);
                }
            }
            let mut invalid = machine(size, concurrent, &[0x60fe, 0x4e71]);
            invalid.install_rom_patches(&set(7, 0xa5)).unwrap();
            invalid.bus.cartridge.save = Some(crate::cartridge::ram::headerless_odd8_fixture());
            assert!(!invalid.supports_rom_cheats());
            let before = invalid.clone();
            assert!(invalid.install_rom_patches(&[patch(DATA, 0xee)]).is_err());
            assert_eq!(invalid, before);
            let mut invalid = machine(size, concurrent, &[0x60fe, 0x4e71]);
            Arc::make_mut(&mut invalid.bus.cartridge.bytes)[DATA as usize] ^= 1;
            assert!(!invalid.supports_rom_cheats());
            let before = invalid.clone();
            assert!(invalid.install_rom_patches(&[patch(DATA, 0xee)]).is_err());
            assert_eq!(invalid, before);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn linear_rom_guest_callee_consumes_sampled_a_and_owned_undo_keeps_current_b() {
    for size in SIZES {
        for concurrent in [false, true] {
            let mut machine = machine(size, concurrent, &[0x60fe, 0x4e71]);
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
