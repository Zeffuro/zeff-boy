use super::super::sound_tests::advance_sound;
use super::*;
use crate::hardware::sound::SoundFault;
use zeff_m68k::bus::Direction;

fn banked_source(region: Region, code: &[u8]) -> Emulator {
    let mut machine = Emulator::new(
        tone_image(&[0x60fe, 0x4e71]),
        LoadConfig {
            region,
            concurrent_sound: true,
        },
    )
    .unwrap();
    machine.set_sample_rate(48_000);
    for _ in 0..3 {
        machine.step_instruction().unwrap();
    }
    machine.bus.sound.set_reset(false);
    machine.bus.sound.bank = 3;
    machine.bus.sound.ram[..3].copy_from_slice(&[0xc3, 0, 0x80]);
    let masks = code
        .iter()
        .enumerate()
        .map(|(offset, &value)| patch(DATA + offset as u32, value))
        .collect::<Vec<_>>();
    machine.install_rom_patches(&masks).unwrap();
    machine
}

fn pending_at(machine: &mut Emulator, address: u32) {
    for _ in 0..1000 {
        advance_sound(machine, 1);
        assert_eq!(machine.sound_fault(), None);
        if machine
            .sound_schedule
            .pending
            .as_ref()
            .is_some_and(|pending| {
                pending.bank_access.is_some_and(|access| {
                    access.address == address && access.direction == Direction::Read
                })
            })
        {
            return;
        }
    }
    panic!("Download banked read not reached");
}

fn restore(machine: &mut Emulator, raw: &[u8], frontend: &[u8], mode: u8) {
    match mode {
        0 => {}
        1 => machine.decode_state(raw).unwrap(),
        2 => machine.decode_frontend_state(frontend).unwrap(),
        _ => unreachable!(),
    }
    assert!(machine.encode_state().unwrap() == raw);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_consumed_prefix_retains_exact_bytes_and_changed_terminal_faults_atomically() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut source = banked_source(region, &[0xdd, 0, 0x76]);
        pending_at(&mut source, DATA + 1);
        assert!(source.sound_schedule.state.prefix_pending());
        assert_eq!(source.sound_cpu.instruction_bytes(), &[0xdd]);
        let pending = source.sound_schedule.pending.as_ref().unwrap().clone();
        assert_eq!(pending.cpu.instruction_bytes(), &[0xdd, 0]);
        let raw = source.encode_state().unwrap();
        let frontend = source.encode_frontend_state().unwrap();
        for consumed in [Some(0xfd), Some(0), None] {
            for terminal in [0, 1] {
                for mode in 0..3 {
                    let mut receiver = source.clone();
                    let mut masks = vec![patch(DATA + 1, terminal), patch(DATA + 2, 0x76)];
                    if let Some(value) = consumed {
                        masks.push(patch(DATA, value));
                    }
                    receiver.install_rom_patches(&masks).unwrap();
                    restore(&mut receiver, &raw, &frontend, mode);
                    assert_eq!(receiver.sound_cpu.instruction_bytes(), &[0xdd]);
                    assert_eq!(receiver.sound_schedule.pending.as_ref(), Some(&pending));
                    let cpu = receiver.sound_cpu.clone();
                    let memory = receiver.bus.sound.clone();
                    let cartridge = receiver.bus.cartridge.clone();
                    let state = receiver.sound_schedule.state;
                    let deadline = receiver.sound_deadline().unwrap().get();
                    let remaining = deadline - receiver.bus.now.get();
                    advance_sound(&mut receiver, remaining - 1);
                    assert_eq!(receiver.sound_cpu, cpu);
                    advance_sound(&mut receiver, 1);
                    assert_eq!(receiver.bus.now.get(), deadline);
                    if terminal == 0 {
                        assert_eq!(receiver.sound_fault(), None);
                        assert!(!receiver.sound_schedule.state.pending());
                        assert_eq!(receiver.sound_cpu.regs().pc, 0x8002);
                        assert_eq!(receiver.sound_cpu.instruction_bytes(), &[0xdd, 0]);
                        let mut oracle = receiver_oracle(&source, &masks, mode != 1);
                        advance_sound(&mut oracle, remaining);
                        same_continuation(&receiver, &oracle);
                        future_field(&mut receiver, &mut oracle);
                    } else {
                        assert_eq!(
                            receiver.sound_fault(),
                            Some(SoundFault::UnstableCartridgeRead { pc: 0x8000 })
                        );
                        assert_eq!(receiver.sound_cpu, cpu);
                        assert_eq!(receiver.sound_cpu.instruction_bytes(), &[0xdd]);
                        assert_eq!(receiver.bus.sound, memory);
                        assert_eq!(receiver.bus.cartridge, cartridge);
                        assert_eq!(receiver.sound_schedule.state, state);
                        assert!(receiver.sound_schedule.pending.is_none());
                        advance_sound(&mut receiver, 60);
                        assert_eq!(receiver.sound_cpu, cpu);
                        assert_eq!(receiver.bus.now.get(), deadline + 60);
                    }
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_halt_refresh_observes_current_mask_without_manufacturing_instruction_bytes() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut source = banked_source(region, &[0x76, 0x5a]);
        pending_at(&mut source, DATA + 1);
        assert!(source.sound_cpu.is_halted());
        assert_eq!(source.sound_cpu.regs().pc, 0x8001);
        assert_eq!(source.sound_cpu.instruction_bytes(), &[0x76]);
        let pending = source.sound_schedule.pending.as_ref().unwrap().clone();
        assert_eq!(pending.instruction.opcode, 0x76);
        assert!(pending.cpu.instruction_bytes().is_empty());
        assert_eq!(pending.bank_access.unwrap().value, 0x5a);
        let raw = source.encode_state().unwrap();
        let frontend = source.encode_frontend_state().unwrap();
        for current in [Some(0x5a), Some(0), Some(0x3e), None] {
            for mode in 0..3 {
                let mut receiver = source.clone();
                let masks = current.map_or_else(Vec::new, |value| vec![patch(DATA + 1, value)]);
                receiver.install_rom_patches(&masks).unwrap();
                restore(&mut receiver, &raw, &frontend, mode);
                assert_eq!(receiver.sound_schedule.pending.as_ref(), Some(&pending));
                let cpu = receiver.sound_cpu.clone();
                let deadline = receiver.sound_deadline().unwrap().get();
                let remaining = deadline - receiver.bus.now.get();
                advance_sound(&mut receiver, remaining - 1);
                assert_eq!(receiver.sound_cpu, cpu);
                advance_sound(&mut receiver, 1);
                assert_eq!(receiver.sound_fault(), None);
                assert!(receiver.sound_cpu.is_halted());
                assert_eq!(receiver.sound_cpu.regs().pc, 0x8001);
                assert!(receiver.sound_cpu.instruction_bytes().is_empty());
                assert_eq!(receiver.sound_cpu.regs().r, cpu.regs().r.wrapping_add(1));
                let mut oracle = receiver_oracle(&source, &masks, mode != 1);
                advance_sound(&mut oracle, remaining);
                same_continuation(&receiver, &oracle);
                future_field(&mut receiver, &mut oracle);
            }
        }
    }
}
