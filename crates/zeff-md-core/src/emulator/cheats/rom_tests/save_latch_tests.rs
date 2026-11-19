use super::*;
use crate::emulator::InstructionObserver;
use zeff_m68k::timing::{InstructionByteOrigin, InstructionInput};

mod pending;
mod state;

fn machine(concurrent_sound: bool, program: &[u16]) -> Emulator {
    let mut bytes = crate::cartridge::save_latch::fixture_image(program);
    write_words(
        &mut bytes,
        CODE as usize,
        &[0x7001, 0x0640, 3, 0x60f8, 0x4e71],
    );
    bytes[DATA as usize..DATA as usize + 4].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
    let cart = crate::cartridge::Cartridge::new(bytes).unwrap();
    assert!(cart.save_latch.is_some());
    Emulator::new_with_cartridge(
        cart,
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
fn save_latch_main_visibility_lanes_and_atomic_rom_sets_preserve_save_priority() {
    for concurrent in [false, true] {
        for (mode, opcode, address) in [
            (0, 0x13fc, 0x30f0),
            (1, 0x13fc, 0x30f1),
            (3, 0x33fc, 0x30f0),
        ] {
            let program = [
                0x13fc, 1, 0x00a1, 0x30f0, 0x13fc, 0x77, 0x0020, 1, opcode, mode, 0x00a1, address,
                0x1039, 1, 0x8000, 0x1239, 1, 0x8001, 0x3439, 1, 0x8000, 0x3639, 0x21, 0x8000,
                0x1839, 0x20, 0, 0x1a39, 0x20, 1, 0x3c39, 0x20, 0, 0x3e39, 0x3f, 0xfffe, 0x60fe,
                0x4e71,
            ];
            let mut m = machine(concurrent, &program);
            let bytes = m.bus.cartridge.bytes.clone();
            let digest = m.bus.cartridge.rom_digest();
            for (text, data, tail) in [
                (None, [0x12, 0x34, 0x1234], 0x0089),
                (
                    Some("018000:A55A + 1FFFFE:C73D"),
                    [0xa5, 0x5a, 0xa55a],
                    0xc73d,
                ),
                (
                    Some("018000:96E1 + 1FFFFE:4826"),
                    [0x96, 0xe1, 0x96e1],
                    0x4826,
                ),
                (None, [0x12, 0x34, 0x1234], 0x0089),
            ] {
                let state = m.encode_frontend_state().unwrap();
                if let Some(text) = text {
                    m.install_rom_patches(&patches(text)).unwrap();
                } else {
                    m.clear_rom_patches();
                }
                assert_eq!(m.encode_frontend_state().unwrap(), state);
                let battery = m.bus.cartridge.save.as_ref().unwrap().bytes.clone();
                m.reset();
                let reset_save = m.bus.cartridge.save.as_ref().unwrap();
                assert_eq!(reset_save.bytes, battery);
                assert!(reset_save.enabled && !reset_save.write_protected);
                let mut capture = Capture::default();
                m.step_instruction_traced(&mut capture).unwrap();
                assert_eq!(capture.0[0].0, 0x200);
                assert_eq!(capture.0[0].1.value, 0x13fc);
                assert_eq!(offsets(capture.0[0].1), [Some(0x200), Some(0x201)]);
                for _ in 0..10 {
                    m.step_instruction().unwrap();
                }
                let save = if mode == 0 {
                    [0, 0xff, 0x00ff]
                } else {
                    [0xff, 0x77, 0xff77]
                };
                assert_eq!(
                    m.registers().d,
                    [
                        data[0], data[1], data[2], data[2], save[0], save[1], save[2], tail
                    ]
                );
                assert_eq!(m.bus.cartridge.save.as_ref().unwrap().bytes[0], 0x77);
                assert_eq!(
                    m.bus.cartridge.save.as_ref().unwrap().write_protected,
                    mode == 3
                );
                assert_eq!(
                    m.rom_offset_for_cpu_address(0x200001),
                    (mode == 0).then_some(1)
                );
                assert_eq!(m.rom_offset_for_cpu_address(0x218000), Some(DATA as usize));
                assert_eq!(m.rom_offset_for_cpu_address(0xa130f0), None);
                assert_eq!(m.peek(0xa130f0), None);
                let sampled = m.bus.cartridge.read_sampled(0x200001).unwrap();
                assert_eq!(sampled.value, save[1] as u8);
                assert_eq!(sampled.origin.storage_offset(), (mode == 0).then_some(1));
                let upper = m.bus.cartridge.read_sampled(0x200000).unwrap();
                assert_eq!(upper.value, save[0] as u8);
                assert_eq!(upper.origin.storage_offset(), (mode == 0).then_some(0));
                crate::hardware::vdp::VdpDmaSource::from_cartridge(&m.bus.cartridge, &m.bus.ram)
                    .assert_word(0x200000, save[2] as u16);
                assert!(Arc::ptr_eq(&bytes, &m.bus.cartridge.bytes));
                assert_eq!(m.bus.cartridge.rom_digest(), digest);
                assert!(m.supports_rom_cheats());
                assert!(m.check_warm_reset().is_err());
                assert!(
                    crate::hardware::bus::Bus::new_owned(m.bus.cartridge.clone(), Region::NtscUs)
                        .is_err()
                );
                let before = m.clone();
                let owned = m.encode_debugger_checkpoint().unwrap();
                for bad in [0x200000, 0xa130f0, 0x1000000] {
                    assert!(
                        m.install_rom_patches(&[patch(DATA, 0xee), patch(bad, 1)])
                            .is_err()
                    );
                    assert_eq!(m, before);
                    assert_eq!(m.encode_debugger_checkpoint().unwrap(), owned);
                }
                assert!(
                    m.install_rom_patches(&patches("018000:EE + 000007:01"))
                        .is_err()
                );
                assert_eq!(m, before);
                if mode != 0 {
                    assert!(m.begin_debug_call(0x200000).is_err());
                    assert_eq!(m, before);
                }
            }
            let mut changed = m.clone();
            Arc::make_mut(&mut changed.bus.cartridge.bytes)[DATA as usize] ^= 1;
            assert!(!changed.supports_rom_cheats());
            let before = changed.clone();
            assert!(changed.install_rom_patches(&[patch(DATA, 0xee)]).is_err());
            assert_eq!(changed, before);
            let mut conflict = m.clone();
            conflict.bus.cartridge.save_latch = None;
            assert!(conflict.bus.cartridge.validate_save_latch().is_err());
            assert!(!conflict.supports_rom_cheats());
            let before = conflict.clone();
            assert!(conflict.install_rom_patches(&[patch(DATA, 0xee)]).is_err());
            assert_eq!(conflict, before);
            let mut invalid_save = m.clone();
            invalid_save.bus.cartridge.save.as_mut().unwrap().bytes =
                vec![0; 8192].into_boxed_slice();
            assert!(!invalid_save.supports_rom_cheats());
            let before = invalid_save.clone();
            assert!(
                invalid_save
                    .install_rom_patches(&[patch(DATA, 0xee)])
                    .is_err()
            );
            assert_eq!(invalid_save, before);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn save_latch_rom_guest_callee_consumes_sampled_a_and_owned_undo_keeps_current_b() {
    for concurrent in [false, true] {
        let mut machine = machine(concurrent, &[0x60fe, 0x4e71]);
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
