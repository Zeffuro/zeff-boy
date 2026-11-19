use super::super::sound_tests::{advance_sound, pending_rom_read};
use super::*;
use crate::hardware::sound::SoundFault;
use crate::hardware::vdp::VdpDmaSource;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_changed_consumed_sound_opcode_faults_without_speculative_publication() {
    let bytes = download_image(&[0x60fe, 0x4e71]);
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut source = Emulator::new(
            bytes.clone(),
            LoadConfig {
                region,
                concurrent_sound: true,
            },
        )
        .unwrap();
        source.bus.sound.set_reset(false);
        source.bus.sound.bank = 3;
        source.bus.sound.ram[..3].copy_from_slice(&[0xc3, 0, 0x80]);
        source.install_rom_patches(&[patch(DATA, 0)]).unwrap();
        pending_rom_read(&mut source);
        assert!(source.sound_schedule.state.next_segment_reads_opcode());
        let pending = source.sound_schedule.pending.as_ref().unwrap().clone();
        assert_eq!(pending.instruction.pc, 0x8000);
        let raw = source.encode_state().unwrap();
        let frontend = source.encode_frontend_state().unwrap();
        for front in [false, true] {
            let mut receiver = source.clone();
            receiver.install_rom_patches(&[patch(DATA, 4)]).unwrap();
            assert_eq!(receiver.sound_schedule.pending.as_ref(), Some(&pending));
            if front {
                receiver.decode_frontend_state(&frontend).unwrap();
            } else {
                receiver.decode_state(&raw).unwrap();
            }
            assert_eq!(receiver.encode_state().unwrap(), raw);
            let cpu = receiver.sound_cpu.clone();
            let memory = receiver.bus.sound.clone();
            let cartridge = receiver.bus.cartridge.clone();
            let state = receiver.sound_schedule.state;
            let deadline = receiver.sound_deadline().unwrap().get();
            let remaining = deadline - receiver.bus.now.get();
            assert!(remaining > 1);
            advance_sound(&mut receiver, remaining - 1);
            assert_eq!(receiver.sound_cpu, cpu);
            assert_eq!(receiver.sound_fault(), None);
            advance_sound(&mut receiver, 1);
            assert_eq!(
                receiver.sound_fault(),
                Some(SoundFault::UnstableCartridgeRead { pc: 0x8000 })
            );
            assert!(receiver.sound_schedule.pending.is_none());
            assert_eq!(receiver.sound_cpu, cpu);
            assert_eq!(receiver.bus.sound, memory);
            assert_eq!(receiver.bus.cartridge, cartridge);
            assert_eq!(receiver.sound_schedule.state, state);
            assert_eq!(receiver.bus.now.get(), deadline);
            advance_sound(&mut receiver, 60);
            assert_eq!(
                receiver.sound_fault(),
                Some(SoundFault::UnstableCartridgeRead { pc: 0x8000 })
            );
            assert_eq!(receiver.sound_cpu, cpu);
            assert_eq!(receiver.bus.now.get(), deadline + 60);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_pending_sound_data_refreshes_at_saved_deadline_with_receiver_mask() {
    let mut bytes = download_image(&[0x60fe, 0x4e71]);
    bytes[DATA as usize] = 0x12;
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut source = Emulator::new(
            bytes.clone(),
            LoadConfig {
                region,
                concurrent_sound: true,
            },
        )
        .unwrap();
        source.bus.sound.set_reset(false);
        source.bus.sound.bank = 3;
        source.bus.sound.ram[..7].copy_from_slice(&[0x3a, 0, 0x80, 0x3a, 0, 0x80, 0x76]);
        source.install_rom_patches(&[patch(DATA, 0xa5)]).unwrap();
        pending_rom_read(&mut source);
        let pending = source.sound_schedule.pending.as_ref().unwrap().clone();
        assert_eq!(pending.cpu.regs().a, 0xa5);
        assert_ne!(source.sound_cpu.regs().a, 0xa5);
        let deadline = source.sound_deadline().unwrap().get();
        assert!(source.bus.now.get() + 1 < deadline);
        let raw = source.encode_state().unwrap();
        let frontend = source.encode_frontend_state().unwrap();
        for current in [Some(0xa5), Some(0x5a), None] {
            for front in [false, true] {
                let mut receiver = source.clone();
                let patches = current.map_or_else(Vec::new, |value| vec![patch(DATA, value)]);
                receiver.install_rom_patches(&patches).unwrap();
                assert_eq!(receiver.encode_state().unwrap(), raw);
                assert_eq!(receiver.sound_schedule.pending.as_ref(), Some(&pending));
                if front {
                    receiver.decode_frontend_state(&frontend).unwrap();
                    assert_eq!(receiver.encode_frontend_state().unwrap(), frontend);
                } else {
                    receiver.decode_state(&raw).unwrap();
                    assert_eq!(receiver.encode_state().unwrap(), raw);
                }
                assert_eq!(receiver.sound_schedule.pending.as_ref(), Some(&pending));
                let cpu = receiver.sound_cpu.clone();
                let ram = receiver.bus.sound.ram.clone();
                let remaining = deadline - receiver.bus.now.get();
                advance_sound(&mut receiver, remaining - 1);
                assert_eq!(receiver.sound_cpu, cpu);
                assert_eq!(receiver.bus.sound.ram, ram);
                advance_sound(&mut receiver, 1);
                assert_eq!(receiver.sound_cpu.regs().a, current.unwrap_or(0x12));
                assert_eq!(receiver.sound_fault(), None);
                assert_eq!(receiver.bus.now.get(), deadline);
                let mut oracle = source.clone();
                oracle.install_rom_patches(&patches).unwrap();
                if !front {
                    oracle.decode_state(&raw).unwrap();
                }
                advance_sound(&mut oracle, remaining);
                assert_eq!(
                    receiver.encode_state().unwrap(),
                    oracle.encode_state().unwrap()
                );
                assert_eq!(
                    receiver.encode_frontend_state().unwrap(),
                    oracle.encode_frontend_state().unwrap()
                );
                assert_eq!(receiver.bus.cartridge.rom_source_offset(DATA), None);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_dma_keeps_sampled_word_and_restores_with_current_mask() {
    let mut program = Vec::new();
    for value in [
        0x8114, 0x8f02, 0x9302, 0x9400, 0x9500, 0x96c0, 0x9700, 0x4400,
    ] {
        program.extend([0x33fc, value, 0x00c0, 4]);
    }
    program.extend([0x60fe, 0x4e71]);
    let mut bytes = download_image(&program);
    write_words(&mut bytes, DATA as usize, &[0x1234, 0x5678]);
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent_sound in [false, true] {
            let mut source = Emulator::new(
                bytes.clone(),
                LoadConfig {
                    region,
                    concurrent_sound,
                },
            )
            .unwrap();
            source
                .install_rom_patches(&patches("018000:A55A+018002:B66B"))
                .unwrap();
            for _ in 0..8 {
                source.step_instruction().unwrap();
            }
            source
                .bus
                .vdp
                .write_control_from(
                    0x0080,
                    region,
                    VdpDmaSource::from_cartridge(&source.bus.cartridge, &source.bus.ram),
                )
                .unwrap();
            for _ in 0..2000 {
                if source
                    .vdp()
                    .fifo()
                    .front()
                    .is_some_and(|entry| entry.data == 0xa55a)
                {
                    break;
                }
                advance_sound(&mut source, 1);
            }
            assert_eq!(source.vdp().fifo().front().unwrap().data, 0xa55a);
            let queue = source.queued_words();
            let fifo = source.vdp().fifo().clone();
            let raw = source.encode_state().unwrap();
            let frontend = source.encode_frontend_state().unwrap();
            for (code, future) in [
                ("018000:A55A+018002:B66B", [0xb6, 0x6b]),
                ("018000:C77C+018002:D88D", [0xd8, 0x8d]),
                ("", [0x56, 0x78]),
            ] {
                for front in [false, true] {
                    let active = patches(code);
                    let mut receiver = source.clone();
                    receiver.install_rom_patches(&active).unwrap();
                    assert_eq!(receiver.encode_state().unwrap(), raw);
                    assert_eq!(receiver.queued_words(), queue);
                    assert_eq!(receiver.vdp().fifo(), &fifo);
                    if front {
                        receiver.decode_frontend_state(&frontend).unwrap();
                        assert_eq!(receiver.encode_frontend_state().unwrap(), frontend);
                    } else {
                        receiver.decode_state(&raw).unwrap();
                        assert_eq!(receiver.encode_state().unwrap(), raw);
                    }
                    assert_eq!(receiver.vdp().fifo(), &fifo);
                    assert_eq!(receiver.queued_words(), queue);
                    let mut oracle = source.clone();
                    oracle.install_rom_patches(&active).unwrap();
                    if !front {
                        oracle.decode_state(&raw).unwrap();
                    }
                    let expected = [0xa5, 0x5a, future[0], future[1]];
                    for _ in 0..4000 {
                        if receiver.vdp().memory().vram()[0x400..0x404] == expected {
                            break;
                        }
                        advance_sound(&mut receiver, 1);
                        advance_sound(&mut oracle, 1);
                    }
                    assert_eq!(&receiver.vdp().memory().vram()[0x400..0x404], &expected);
                    assert_eq!(
                        receiver.encode_state().unwrap(),
                        oracle.encode_state().unwrap()
                    );
                    assert_eq!(
                        receiver.encode_frontend_state().unwrap(),
                        oracle.encode_frontend_state().unwrap()
                    );
                    assert_eq!(receiver.sound_fault(), None);
                }
            }
        }
    }
}
