use super::*;
use crate::emulator::InstructionObserver;
use zeff_m68k::timing::{InstructionByteOrigin, InstructionInput};

mod state;

const PROFILES: [bool; 2] = [false, true];
const ROUTES: [(TimingModel, bool); 3] = [
    (TimingModel::Standard, false),
    (TimingModel::Standard, true),
    (TimingModel::Precise, true),
];

fn machine(time: bool, timing: TimingModel, concurrent_sound: bool, program: &[u16]) -> Emulator {
    let mut bytes = image(program);
    bytes.resize(if time { 0x200000 } else { 0x100000 }, 0);
    bytes[0x80..0x90].copy_from_slice(if time {
        b"UNPROTECTED_TIME"
    } else {
        b"UNPROTECTED_TEST"
    });
    let machine = Emulator::new_with_timing(
        bytes,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound,
        },
        timing,
    )
    .unwrap();
    assert!(machine.bus.cartridge.unprotected_rom.is_some());
    assert!(machine.bus.cartridge.unprotected_rom_compatible());
    assert_eq!(machine.timing_model(), timing);
    machine
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
fn unprotected_profiles_execute_literal_main_lanes_and_preserve_time_read_policy() {
    for time in PROFILES {
        let tail = if time { 0x1ffffe_u32 } else { 0x0ffffe };
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
            0x60e0,
            0x4e71,
        ];
        for (timing, concurrent) in ROUTES {
            let mut machine = machine(time, timing, concurrent, &program);
            let source = machine.bus.cartridge.bytes.clone();
            let identity = machine.bus.cartridge.rom_digest();
            for (text, expected) in [
                (
                    Some(format!("018000:A55A + {tail:06X}:C73D")),
                    [0xa5, 0x5a, 0xa55a, 0xc73d, 0x3d],
                ),
                (
                    Some(format!("018000:96E1 + {tail:06X}:4826")),
                    [0x96, 0xe1, 0x96e1, 0x4826, 0x26],
                ),
                (None, [0x12, 0x34, 0x1234, 0, 0]),
            ] {
                if let Some(text) = text {
                    machine.install_rom_patches(&patches(&text)).unwrap();
                } else {
                    machine.clear_rom_patches();
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
                for _ in 0..4 {
                    machine.step_instruction().unwrap();
                }
                assert_eq!(machine.registers().d[..5], expected);
                machine.step_instruction().unwrap();
                assert_eq!(machine.registers().pc, 0x200);
                assert!(Arc::ptr_eq(&source, &machine.bus.cartridge.bytes));
                assert_eq!(machine.bus.cartridge.rom_digest(), identity);
                let unpopulated = machine.bus.cartridge.read_sampled(tail + 2).unwrap();
                assert_eq!(unpopulated.value, 0xff);
                assert_eq!(unpopulated.origin, InstructionByteOrigin::UNKNOWN);
                assert_eq!(machine.rom_offset_for_cpu_address(tail + 2), None);
                assert!(machine.dump_save_ram().is_none());
                crate::hardware::vdp::VdpDmaSource::from_cartridge(
                    &machine.bus.cartridge,
                    &machine.bus.ram,
                )
                .assert_word(DATA, expected[2] as u16);
            }
        }
        for concurrent in [false, true] {
            let program = [
                0x3039, 0x00a1, 0x3000, 0x3039, 0x00a1, 0x3000, 0x0200, 0, 0x60fe,
            ];
            let mut machine = machine(time, TimingModel::Standard, concurrent, &program);
            machine.install_rom_patches(&[patch(DATA, 0xa5)]).unwrap();
            assert_eq!(machine.peek(0xa13000), None);
            if time {
                machine.step_instruction().unwrap();
                assert_eq!(machine.registers().d[0] & 0xffff, 0x3039);
                machine.step_instruction().unwrap();
                assert_eq!(machine.registers().d[0] & 0xffff, 0x0200);
            } else {
                assert!(machine.step_instruction().is_err());
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn unprotected_effective_vectors_and_old_main_queue_survive_replacement_and_disable() {
    for time in PROFILES {
        for (timing, concurrent) in ROUTES {
            for next in [Some(9), None] {
                let mut machine = machine(time, timing, concurrent, &[0x4ef9, 1, 0x0100, 0x4e71]);
                machine.install_rom_patches(&set(7, 0xa5)).unwrap();
                machine.step_instruction().unwrap();
                let queued = machine.cpu.queued_instruction_inputs().unwrap();
                assert_eq!(queued[0].value, 0x7007);
                assert_eq!(
                    offsets(queued[0]),
                    [Some(u64::from(CODE)), Some(u64::from(CODE + 1))]
                );
                let saved = machine.encode_frontend_state().unwrap();
                if let Some(value) = next {
                    machine.install_rom_patches(&set(value, 0x5a)).unwrap();
                } else {
                    machine.clear_rom_patches();
                }
                assert_eq!(machine.encode_frontend_state().unwrap(), saved);
                assert_eq!(machine.cpu.queued_instruction_inputs().unwrap(), queued);
                let mut capture = Capture::default();
                machine.step_instruction_traced(&mut capture).unwrap();
                assert_eq!(capture.0, [(CODE, queued[0])]);
                assert_eq!(machine.registers().d[0], 7);
                for _ in 0..3 {
                    machine.step_instruction().unwrap();
                }
                assert_eq!(machine.registers().d[0], u32::from(next.unwrap_or(1)));
                machine
                    .install_rom_patches(&patches(
                        "000000:1234 + 000002:5679 + 000004:8001 + 000006:0100 + 010101:0B",
                    ))
                    .unwrap();
                let initial = machine.bus.cartridge.initial;
                machine.reset();
                assert_eq!(machine.bus.cartridge.initial, initial);
                assert_eq!(machine.registers().pc, 0x80010100);
                assert_eq!(machine.registers().ssp, 0x12345679);
                assert_eq!(machine.queued_words(), Some([0x700b, 0x0640]));
                let mut capture = Capture::default();
                machine.step_instruction_traced(&mut capture).unwrap();
                assert_eq!(capture.0[0].0, 0x80010100);
                assert_eq!(offsets(capture.0[0].1), [Some(0x10100), Some(0x10101)]);
                assert_eq!(machine.registers().d[0], 11);
                machine.clear_rom_patches();
                machine.reset();
                assert_eq!(machine.registers().pc, 0x200);
                assert_eq!(machine.queued_words(), Some([0x4ef9, 1]));
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn unprotected_admission_keeps_original_identity_boundaries_and_neighbor_rejections_atomic() {
    crate::cartridge::unprotected_rom::check_identity();
    for time in PROFILES {
        for (timing, concurrent) in ROUTES {
            let mut machine = machine(time, timing, concurrent, &[0x60fe, 0x4e71]);
            machine.install_rom_patches(&set(7, 0xa5)).unwrap();
            let before = machine.clone();
            let inputs = machine.cpu.queued_instruction_inputs();
            let len = machine.rom_bytes().len() as u32;
            for bad in [len, 0x3fffff, 0x400000, 0xa13000, 0xff0000, u32::MAX] {
                assert!(
                    machine
                        .install_rom_patches(&[patch(DATA, 0xee), patch(bad, 1)])
                        .is_err()
                );
                assert_eq!(machine, before);
                assert_eq!(machine.cpu.queued_instruction_inputs(), inputs);
            }
            for text in ["018000:EE + 000007:01", "018000:EE + 000004:0040"] {
                assert!(machine.install_rom_patches(&patches(text)).is_err());
                assert_eq!(machine, before);
            }
            let mut conflict = machine.clone();
            conflict.bus.cartridge.ssf2 = Some(crate::cartridge::ssf2::Ssf2::new());
            assert!(!conflict.supports_rom_cheats());
            let before = conflict.clone();
            assert!(conflict.install_rom_patches(&[patch(DATA, 0xee)]).is_err());
            assert_eq!(conflict, before);
        }
    }
    let mut linear = Emulator::new_with_cartridge(
        crate::cartridge::linear_rom::fixture(&[0x60fe]),
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: true,
        },
    )
    .unwrap();
    linear.bus.cartridge.ssf2 = Some(crate::cartridge::ssf2::Ssf2::new());
    let before = linear.clone();
    assert!(!linear.supports_rom_cheats());
    assert!(linear.install_rom_patches(&[patch(DATA, 0xa5)]).is_err());
    assert_eq!(linear, before);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn unprotected_rom_guest_call_consumes_sampled_opcode_and_owned_restore_keeps_current_set() {
    for time in PROFILES {
        for (timing, concurrent) in ROUTES {
            let mut machine = machine(time, timing, concurrent, &[0x60fe, 0x4e71]);
            let mut a = set(7, 0xa5).to_vec();
            a.extend(patches("010106:4E75"));
            machine.install_rom_patches(&a).unwrap();
            let saved = machine.encode_debugger_checkpoint().unwrap();
            let queued = machine.cpu.queued_instruction_inputs();
            let mut context = machine.begin_debug_call(CODE).unwrap();
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
            assert_eq!(machine.queued_words(), Some([0x60fe, 0x4e71]));
            assert_eq!(
                capture
                    .0
                    .iter()
                    .map(|(address, _)| *address)
                    .collect::<Vec<_>>(),
                [CODE, CODE + 2, CODE + 4, CODE + 6]
            );
            assert_eq!(capture.0[0], (CODE, admitted));
            machine.decode_debugger_checkpoint(&saved).unwrap();
            assert_eq!(machine.cpu.queued_instruction_inputs(), queued);
            assert_eq!(machine.peek(DATA), Some(0x5a));
            assert_eq!(machine.peek(CODE + 1), Some(9));
            assert_eq!(machine.registers().d[0], 0);
            context = machine.begin_debug_call(CODE).unwrap();
            for _ in 0..3 {
                machine.step_instruction().unwrap();
            }
            assert!(machine.debug_call_returned(&context));
            machine.finish_debug_call(context).unwrap();
            assert_eq!(machine.registers().d[0], 12);
            assert_eq!(machine.peek(DATA), Some(0x5a));
        }
    }
}
