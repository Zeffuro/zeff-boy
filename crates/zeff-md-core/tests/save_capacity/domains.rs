use super::fixture::{Rom, config, expected, run_to_phase};
use zeff_emu_common::time::Reset;
use zeff_m68k::bus::Direction;
use zeff_md_core::{Emulator, LoadError, MachineFault, Region, SoundFault};

#[test]
fn new_layouts_allow_real_guest_reads_and_writes_without_enable_before_and_after_reset() {
    for size in [8192, 32768] {
        let last = 0x200000 + size as u32 * 2 - 1;
        let mut rom = Rom::new(size);
        for (index, (address, mask)) in [(0x200001, 0x5a), (last, 0xa7)].into_iter().enumerate() {
            rom.read_byte(address, 0xff0000 + index as u32);
            rom.word(0x0a00);
            rom.word(mask);
            rom.word(0x13c0);
            rom.long(address);
            rom.read_byte(address, 0xff0002 + index as u32);
        }
        let bytes = rom.finish();
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            for concurrent in [false, true] {
                let mut machine = Emulator::new(bytes.clone(), config(region, concurrent)).unwrap();
                run_to_phase(&mut machine, 3);
                for (offset, value) in [0xff, 0xff, 0xa5, 0x58].into_iter().enumerate() {
                    assert_eq!(machine.peek(0xff0000 + offset as u32), Some(value));
                }
                let mut expected = vec![0xff; size];
                expected[0] = 0xa5;
                expected[size - 1] = 0x58;
                assert_eq!(machine.dump_save_ram(), Some(expected));
                machine.reset();
                run_to_phase(&mut machine, 3);
                for (offset, value) in [0xa5, 0x58, 0xff, 0xff].into_iter().enumerate() {
                    assert_eq!(machine.peek(0xff0000 + offset as u32), Some(value));
                }
                assert_eq!(machine.dump_save_ram(), Some(vec![0xff; size]));
            }
        }
    }
}

#[test]
fn new_layout_admission_requires_the_complete_exact_nonoverlap_declaration() {
    for size in [8192, 32768] {
        let bytes = Rom::new(size).finish();
        for offset in 0x1b2..0x1bc {
            let mut malformed = bytes.clone();
            malformed[offset] ^= 1;
            assert!(
                matches!(
                    Emulator::new(malformed, config(Region::NtscUs, false)),
                    Err(LoadError::UnsupportedSaveHardware)
                ),
                "size={size} header offset={offset:x}"
            );
        }
        let mut overlap = bytes.clone();
        overlap.resize(0x200002, 0);
        assert!(matches!(
            Emulator::new(overlap, config(Region::NtscUs, false)),
            Err(LoadError::UnsupportedSaveHardware)
        ));
        let mut short = bytes;
        short[4..8].copy_from_slice(&0x100_u32.to_be_bytes());
        short[0x100..0x104].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
        for length in 0x1b2..0x1bc {
            assert!(
                matches!(
                    Emulator::new(short[..length].to_vec(), config(Region::NtscUs, false)),
                    Err(LoadError::UnsupportedSaveHardware)
                ),
                "size={size} truncated length={length:x}"
            );
        }
    }
}

#[test]
fn constructor_frontend_reset_cpu_restart_and_reset_output_keep_their_mapping_domains() {
    for size in [512, 8192, 32768] {
        for concurrent in [false, true] {
            let mut rom = Rom::new(size);
            rom.enable();
            rom.enable();
            rom.word(0x4e70);
            rom.read_byte(0x200001, 0xff0000);
            rom.read_byte(0x200000 + size as u32 * 2 - 1, 0xff0001);
            let mut machine =
                Emulator::new(rom.finish(), config(Region::NtscUs, concurrent)).unwrap();
            assert_eq!(machine.peek(0x200001), (size != 512).then_some(0xff));
            let cells = expected(size, 0xa1);
            machine.load_save_ram(&cells).unwrap();
            assert_eq!(machine.peek(0x200001), (size != 512).then_some(cells[0]));
            run_to_phase(&mut machine, 3);
            assert_eq!(machine.reset_pulses(), 1);
            assert_eq!(machine.peek(0xff0000), Some(cells[0]));
            assert_eq!(machine.peek(0xff0001), Some(cells[size - 1]));
            machine.reset_cpu().unwrap();
            assert_eq!(machine.peek(0x200001), Some(cells[0]));
            machine.reset();
            assert_eq!(machine.dump_save_ram(), Some(cells.clone()));
            assert_eq!(machine.peek(0x200001), (size != 512).then_some(cells[0]));
            assert_eq!(
                machine.peek(0x200000 + size as u32 * 2 - 1),
                (size != 512).then_some(cells[size - 1])
            );
            run_to_phase(&mut machine, 3);
            assert_eq!(machine.dump_save_ram(), Some(cells));
        }
    }
}

#[test]
fn unsupported_controls_and_old_pre_enable_accesses_fault_without_save_mutation() {
    for size in [512, 8192, 32768] {
        for (address, value) in [
            (0xa130f1, 0),
            (0xa130f1, 2),
            (0xa130f1, 3),
            (0xa130f1, 0x81),
            (0xa130f0, 1),
        ] {
            let mut rom = Rom::new(size);
            rom.byte(address, value);
            let mut machine = Emulator::new(rom.finish(), config(Region::NtscUs, true)).unwrap();
            assert!(
                machine.step_instruction().is_err(),
                "size={size} {address:x}={value:x}"
            );
            assert_eq!(machine.dump_save_ram(), Some(vec![0xff; size]));
            assert_eq!(machine.peek(0x200001), (size != 512).then_some(0xff));
        }
        let mut rom = Rom::new(size);
        rom.write_word(0xa130f0, 1);
        let mut machine = Emulator::new(rom.finish(), config(Region::Pal, false)).unwrap();
        assert!(machine.step_instruction().is_err());
        assert_eq!(machine.dump_save_ram(), Some(vec![0xff; size]));
        for address in [0xa130f0, 0xa130f1] {
            let mut rom = Rom::new(size);
            rom.read_byte(address, 0xff0000);
            let mut machine = Emulator::new(rom.finish(), config(Region::Pal, false)).unwrap();
            assert!(machine.step_instruction().is_err());
            assert_eq!(machine.peek(0xff0000), Some(0));
            assert_eq!(machine.dump_save_ram(), Some(vec![0xff; size]));
        }
    }
    for address in [0x200000, 0x200001, 0x2003fe, 0x2003ff] {
        for write in [false, true] {
            let mut rom = Rom::new(512);
            if write {
                rom.byte(address, 0x42);
            } else {
                rom.read_byte(address, 0xff0000);
            }
            let mut machine = Emulator::new(rom.finish(), config(Region::NtscJapan, true)).unwrap();
            assert!(machine.step_instruction().is_err());
            assert_eq!(machine.dump_save_ram(), Some(vec![0xff; 512]));
            assert_eq!(machine.peek(0xff0000), Some(0));
        }
    }
}

#[test]
fn restored_pending_vdp_dma_reads_first_and_last_new_cells() {
    for size in [8192, 32768] {
        for index in [0, size - 1] {
            let source_word = (0x200000 + index as u32 * 2) / 2;
            let mut rom = Rom::new(size);
            for (register, value) in [
                (1, 0x14),
                (15, 2),
                (19, 1),
                (20, 0),
                (21, source_word as u8),
                (22, (source_word >> 8) as u8),
                (23, (source_word >> 16) as u8),
            ] {
                rom.write_word(0xc00004, 0x8000 | register << 8 | u16::from(value));
            }
            rom.write_word(0xc00004, 0x5000);
            let cut = rom.pc as u32;
            rom.write_word(0xc00004, 0x0080);
            let mut machine = Emulator::new(rom.finish(), config(Region::Pal, true)).unwrap();
            let cells = expected(size, 0x37);
            machine.load_save_ram(&cells).unwrap();
            while machine.registers().pc != cut {
                machine.step_instruction().unwrap();
            }
            let saved = machine.encode_state().unwrap();
            let mut restored = machine.clone();
            machine.decode_state(&saved).unwrap();
            restored.decode_state(&saved).unwrap();
            run_to_phase(&mut machine, 3);
            run_to_phase(&mut restored, 3);
            assert_eq!(machine, restored);
            assert_eq!(
                &machine.vdp().memory().vram()[0x1000..0x1002],
                &[0xff, cells[index]]
            );
        }
    }
}

#[test]
fn actual_uploaded_z80_banked_reads_and_writes_guard_both_new_aperture_endpoints() {
    for size in [8192, 32768] {
        for address in [0x200001, 0x200000 + size as u32 * 2 - 1] {
            for direction in [Direction::Read, Direction::Write] {
                let bank = address & !0x7fff;
                let sound_address = (0x8000 | (address & 0x7fff)) as u16;
                let mut sound = Vec::new();
                for bit in 0..9 {
                    sound.extend([0x3e, ((bank >> (15 + bit)) & 1) as u8, 0x32, 0, 0x60]);
                }
                if direction == Direction::Write {
                    sound.extend([0x3e, 0x42]);
                }
                sound.extend([
                    if direction == Direction::Read {
                        0x3a
                    } else {
                        0x32
                    },
                    sound_address as u8,
                    (sound_address >> 8) as u8,
                    0x76,
                ]);
                let mut rom = Rom::new(size);
                rom.write_word(0xa11100, 0x0100);
                rom.write_word(0xa11200, 0x0100);
                for (offset, value) in sound.into_iter().enumerate() {
                    rom.byte(0xa00000 + offset as u32, value);
                }
                rom.write_word(0xa11200, 0x0100);
                rom.write_word(0xa11100, 0);
                let mut machine =
                    Emulator::new(rom.finish(), config(Region::NtscUs, true)).unwrap();
                let mut fault = None;
                for _ in 0..1000 {
                    if let Err(error) = machine.step_instruction() {
                        fault = Some(error);
                        break;
                    }
                }
                assert_eq!(
                    fault,
                    Some(MachineFault::Sound(SoundFault::UnsupportedAccess {
                        address: sound_address,
                        direction,
                    }))
                );
                assert_eq!(machine.dump_save_ram(), Some(vec![0xff; size]));
            }
        }
    }
}
