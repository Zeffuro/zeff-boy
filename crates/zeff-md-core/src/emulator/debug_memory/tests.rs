use super::*;
use crate::emulator::InstructionObserver;
use crate::{ControllerKind, ControllerPort, LoadConfig, Region, TimingModel};
use zeff_emu_common::cheats::{CheatPatch, CheatValue};
use zeff_m68k::timing::InstructionInput;

mod serial;

fn machine(region: Region, timing: TimingModel, kind: ControllerKind) -> Emulator {
    program(
        region,
        timing,
        kind,
        &[0x7007, 0x4e71, 0x60fa, 0x4e71, 0x4e71, 0x4e71],
    )
}

fn program(region: Region, timing: TimingModel, kind: ControllerKind, words: &[u16]) -> Emulator {
    let mut rom = vec![0; 0x2000];
    rom[..4].copy_from_slice(&0xfffffc_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    for (index, word) in words.iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut emu = Emulator::new_with_timing(
        rom,
        LoadConfig {
            region,
            concurrent_sound: true,
        },
        timing,
    )
    .unwrap();
    for port in [ControllerPort::One, ControllerPort::Two] {
        emu.input_mut().set_controller_kind(port, kind);
        emu.input_mut().set_controller_raw(port, 0xbe);
        emu.input_mut().set_controller_extra_raw(port, 5);
    }
    emu
}

fn versions(emu: &Emulator) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    (
        emu.encode_state().unwrap(),
        emu.encode_frontend_state().unwrap(),
        emu.encode_debugger_checkpoint().unwrap(),
    )
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn external_parallel_bytes_keep_every_other_machine_owner_and_exact_latches() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            for kind in [ControllerKind::ThreeButton, ControllerKind::SixButton] {
                let mut emu = machine(region, timing, kind);
                let before = emu.clone();
                let writes = [
                    (0xa10009, 0xc0),
                    (0xa10003, 0x80),
                    (0xa1000b, 0x81),
                    (0xa10005, 0x80),
                    (0xa1000d, 0xff),
                    (0xa10007, 0xd5),
                ];
                emu.validate_debug_memory_writes(&writes).unwrap();
                assert_eq!(emu, before);
                emu.debug_write_memory(&writes).unwrap();
                assert_eq!(emu.peek(0xa10009), Some(0xc0));
                assert_eq!(emu.peek(0xa1000b), Some(0x81));
                assert_eq!(emu.peek(0xa1000d), Some(0xff));
                assert_eq!(emu.peek(0xa10003), Some(0xa2));
                assert_eq!(emu.peek(0xa10005), Some(0xfe));
                assert_eq!(emu.peek(0xa10007), Some(0xd5));
                let mut normalized = emu.clone();
                normalized.bus.input = before.bus.input.clone();
                assert_eq!(normalized, before);
                emu.debug_write_memory(&[(0xa10009, 0xff), (0xa1000b, 0xff)])
                    .unwrap();
                assert_eq!(emu.peek(0xa10003), Some(0x80));
                assert_eq!(emu.peek(0xa10005), Some(0x80));
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn external_edits_reject_every_noncanonical_domain_atomically_and_keep_static_ram_api() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut emu = machine(Region::NtscUs, timing, ControllerKind::SixButton);
        for bad in [
            0, 0x200000, 0xe00000, 0xfeffff, 0x1000000, 0x1ff0000, 0xa10000, 0xa10001, 0xa10002,
            0xa10004, 0xa10006, 0xa10008, 0xa1000a, 0xa1000c, 0xa1000e, 0xa1000f, 0xa10011,
            0xa10013, 0xa10015, 0xa10017, 0xa10019, 0xa1001b, 0x1a10003, 0x1a10009, 0xa00000,
            0xa04000, 0xa11100, 0xa11200, 0xa130f1, 0xa14000, 0xa14101, 0xc00000, 0xc00004,
            0xc00011,
        ] {
            let before = emu.clone();
            let states = versions(&emu);
            let writes = [
                (0xff0041, 0x55),
                (0xa10009, 0x40),
                (0xa10003, 0),
                (bad, 0xa5),
            ];
            assert_eq!(emu.validate_debug_memory_writes(&writes), Err(bad));
            assert_eq!(emu.debug_write_memory(&writes), Err(bad));
            assert_eq!(emu, before);
            assert_eq!(versions(&emu), states);
        }
        let before = emu.clone();
        emu.debug_write_memory(&[]).unwrap();
        assert_eq!(emu, before);
        let writes = [(0xff0000, 1), (0xffffff, 2), (0xff0000, 3)];
        emu.debug_write_memory(&writes).unwrap();
        assert_eq!(emu.peek(0xff0000), Some(3));
        assert_eq!(emu.peek(0xffffff), Some(2));
        assert_eq!(
            Emulator::validate_debug_ram_writes(&[(0xa10003, 0)]),
            Err(0xa10003)
        );
        assert_eq!(
            emu.debug_write_ram(&[(0xff0041, 0x77), (0xa10009, 1)]),
            Err(0xa10009)
        );
        assert_eq!(emu.peek(0xff0041), Some(0));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn ordered_same_frontier_th_edges_restore_and_expire_at_each_region_boundary() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            for kind in [ControllerKind::ThreeButton, ControllerKind::SixButton] {
                let mut emu = machine(region, timing, kind);
                emu.step_instruction().unwrap();
                assert_eq!(emu.registers().d[0], 7);
                let now = emu.bus.input_state_tick().unwrap();
                if timing == TimingModel::Precise {
                    assert_eq!(emu.bus.vdp.owned_half().unwrap() & 1, 1);
                    assert_eq!(now, emu.bus.now.get() + 1);
                }
                let clock = emu.cpu_clock();
                for (data, ctrl) in [(0xa10003, 0xa10009), (0xa10005, 0xa1000b)] {
                    emu.debug_write_memory(&[
                        (ctrl, 0x40),
                        (data, 0),
                        (data, 0x40),
                        (data, 0),
                        (data, 0x40),
                        (data, 0),
                    ])
                    .unwrap();
                    assert_eq!(
                        emu.peek(data),
                        Some(if kind == ControllerKind::SixButton {
                            0x20
                        } else {
                            0x22
                        })
                    );
                    emu.debug_write_memory(&[(data, 0x40)]).unwrap();
                    assert_eq!(
                        emu.peek(data),
                        Some(if kind == ControllerKind::SixButton {
                            0x75
                        } else {
                            0x7e
                        })
                    );
                }
                assert_eq!(emu.bus.input_state_tick().unwrap(), now);
                assert_eq!(emu.cpu_clock(), clock);
                let state = emu.encode_debugger_checkpoint().unwrap();
                let mut restored = machine(region, timing, kind);
                restored.decode_debugger_checkpoint(&state).unwrap();
                assert_eq!(restored.bus.input, emu.bus.input);
                for delta in [region.pad_timeout_ticks() - 1, region.pad_timeout_ticks()] {
                    for port in 0..2 {
                        let expected = if kind == ControllerKind::SixButton
                            && delta < region.pad_timeout_ticks()
                        {
                            0x75
                        } else {
                            0x7e
                        };
                        assert_eq!(emu.bus.input.read_data(port, now + delta, region), expected);
                        assert_eq!(
                            restored.bus.input.read_data(port, now + delta, region),
                            expected
                        );
                    }
                }
                for _ in 0..32 {
                    assert_eq!(emu.step_instruction(), restored.step_instruction());
                    assert_eq!(versions(&emu), versions(&restored));
                }
            }
        }
    }
}

#[derive(Default)]
struct Inputs(Vec<(u32, InstructionInput)>);
impl InstructionObserver for Inputs {
    fn instruction_input(&mut self, address: u32, input: InstructionInput) {
        self.0.push((address, input));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn external_input_edit_preserves_queued_physical_opcode_under_current_overlay() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut emu = machine(Region::NtscUs, timing, ControllerKind::SixButton);
        emu.step_instruction().unwrap();
        let old = emu.cpu.queued_instruction_inputs().unwrap();
        assert_eq!(old[0].value, 0x4e71);
        assert_eq!(
            old[0].origin.map(|origin| origin.storage_offset()),
            [Some(0x202), Some(0x203)]
        );
        emu.install_rom_patches(&[CheatPatch::WideRomWrite {
            address: 0x203,
            value: CheatValue::Constant(0x75),
        }])
        .unwrap();
        assert_eq!(emu.peek(0x203), Some(0x75));
        let before = emu.clone();
        emu.debug_write_memory(&[(0xa10009, 0x40), (0xa10003, 0)])
            .unwrap();
        assert_eq!(emu.cpu.queued_instruction_inputs(), Some(old));
        let mut normalized = emu.clone();
        normalized.bus.input = before.bus.input.clone();
        assert_eq!(normalized, before);
        let mut seen = Inputs::default();
        let step = emu.step_instruction_traced(&mut seen).unwrap();
        assert!(
            matches!(step.cpu, zeff_m68k::timing::TimedOutcome::Instruction(step) if step.opcode == 0x4e71)
        );
        assert_eq!(seen.0[0], (0x202, old[0]));
        assert_eq!(emu.peek(0x203), Some(0x75));
    }
}
