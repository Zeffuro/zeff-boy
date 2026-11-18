use super::fixture::{Rom, all_cells_rom, config, expected, run_to_phase};
use zeff_emu_common::time::Reset;
use zeff_m68k::bus::Direction;
use zeff_md_core::{Emulator, LoadError, MachineFault, Region, SoundFault};

const PROFILES: [(usize, &[u8]); 8] = [
    (8192, b"GM G-5524  -00"),
    (8192, b"GM MK -1307   "),
    (8192, b"GM MK-1307 -00"),
    (8192, b"GM MK-1354 -00"),
    (8192, b"GM G-5543  -00"),
    (32768, b"GM MK-1239 -00"),
    (32768, b"GM MK-1242 -00"),
    (32768, b"GM MK-1244 -00"),
];

fn switched(mut bytes: Vec<u8>, product: &[u8]) -> Vec<u8> {
    bytes.resize(0x300000, 0x69);
    bytes[0x180..0x18e].copy_from_slice(product);
    bytes
}

fn product(size: usize) -> &'static [u8] {
    PROFILES.iter().find(|(n, _)| *n == size).unwrap().1
}

#[test]
fn switching_admission_requires_matching_board_capacity_size_and_declaration() {
    for (size, product) in PROFILES {
        let bytes = switched(Rom::new(size).finish(), product);
        let machine = Emulator::new(bytes.clone(), config(Region::NtscUs, false)).unwrap();
        assert_eq!(machine.peek(0x200001), Some(0x69));
        assert_eq!(machine.dump_save_ram(), Some(vec![0xff; size]));
        for offset in (0x180..0x18e).chain(0x1b2..0x1bc) {
            let mut invalid = bytes.clone();
            invalid[offset] ^= 1;
            assert!(
                matches!(
                    Emulator::new(invalid, config(Region::NtscUs, false)),
                    Err(LoadError::UnsupportedSaveHardware)
                ),
                "product={product:?} offset={offset:x}"
            );
        }
        for length in [0x200002, 0x280000, 0x300002, 0x400000] {
            let mut invalid = bytes.clone();
            invalid.resize(length, 0);
            assert!(matches!(
                Emulator::new(invalid, config(Region::NtscUs, false)),
                Err(LoadError::UnsupportedSaveHardware)
            ));
        }
    }
}

#[test]
fn guest_switches_word_and_byte_controls_and_mirrors_both_lanes() {
    for size in [8192, 32768] {
        let mut rom = Rom::new(size);
        rom.byte(0x200001, 0x12);
        rom.read_byte(0x200001, 0xff0000);
        rom.enable();
        rom.byte(0x200000, 0x12);
        rom.write_word(0x200000, 0x1234);
        rom.write_long(0x300002, 0x56789abc);
        rom.word(0x207c);
        rom.long(0x3f0001);
        rom.word(0x203c);
        rom.long(0x5c6d7e8f);
        rom.word(0x01c8);
        rom.word(0);
        rom.word(0x0148);
        rom.word(0);
        rom.word(0x23c0);
        rom.long(0xff0010);
        for (index, address) in [0x200000, 0x200001, 0x204001, 0x300003, 0x3fffff]
            .into_iter()
            .enumerate()
        {
            rom.read_byte(address, 0xff0001 + index as u32);
        }
        rom.phase(1);
        rom.byte(0xa130f1, 2);
        rom.byte(0x200001, 0x99);
        rom.read_byte(0x200001, 0xff0006);
        rom.read_byte(0x300001, 0xff0007);
        rom.phase(2);
        rom.write_word(0xa130f0, 0xab01);
        rom.phase(4);
        rom.write_word(0xa130f0, 0xcd00);
        let bytes = switched(rom.finish(), product(size));
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            for concurrent in [false, true] {
                let mut machine = Emulator::new(bytes.clone(), config(region, concurrent)).unwrap();
                for phase in [1, 2, 4, 3] {
                    run_to_phase(&mut machine, phase);
                    let cells = machine.dump_save_ram().unwrap();
                    assert_eq!(&cells[..4], &[0x5c, 0x6d, 0x7e, 0x8f]);
                    assert_eq!(cells[size - 1], 0xff);
                    assert_eq!(machine.peek(0xff0000), Some(0x69));
                    assert_eq!(machine.peek(0xff0001), Some(0xff));
                    let alias = if size == 8192 { 0x5c } else { 0xff };
                    assert_eq!(machine.peek(0xff0003), Some(alias));
                    assert_eq!(machine.peek(0xff0004), Some(0x6d));
                    assert_eq!(machine.peek(0xff0005), Some(0xff));
                    for (offset, value) in [0x5c, 0x6d, 0x7e, 0x8f].into_iter().enumerate() {
                        assert_eq!(machine.peek(0xff0010 + offset as u32), Some(value));
                    }
                    if phase != 1 {
                        assert_eq!(machine.peek(0xff0006), Some(0x69));
                        assert_eq!(machine.peek(0xff0007), Some(0xff));
                    }
                    assert_eq!(
                        machine.peek(0x200001),
                        Some(if phase == 1 || phase == 4 { 0x5c } else { 0x69 })
                    );
                    let state = machine.encode_state().unwrap();
                    assert_eq!(&state[8..12], &1_u32.to_le_bytes());
                    let mut restored = machine.clone();
                    restored.load_save_ram(&vec![0; size]).unwrap();
                    restored.decode_state(&state).unwrap();
                    machine.decode_state(&state).unwrap();
                    assert_eq!(machine, restored);
                    let mut future = machine.clone();
                    for _ in 0..3 {
                        assert_eq!(future.step_instruction(), restored.step_instruction());
                    }
                    assert_eq!(
                        future.encode_state().unwrap(),
                        restored.encode_state().unwrap()
                    );
                }
                let cells = machine.dump_save_ram();
                machine.reset_cpu().unwrap();
                assert_eq!(machine.peek(0x200001), Some(0x69));
                machine.reset();
                assert_eq!(machine.peek(0x200001), Some(0x69));
                assert_eq!(machine.dump_save_ram(), cells);
            }
        }
    }
}

#[test]
fn switched_guest_fills_reads_and_mirrors_every_cell_twice() {
    for size in [8192, 32768] {
        let bytes = switched(all_cells_rom(size), product(size));
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            for concurrent in [false, true] {
                let mut machine = Emulator::new(bytes.clone(), config(region, concurrent)).unwrap();
                for (phase, seed) in [(1, 0x19), (2, 0xe6)] {
                    run_to_phase(&mut machine, phase);
                    let cells = expected(size, seed);
                    assert_eq!(machine.dump_save_ram(), Some(cells.clone()));
                    for (index, value) in cells.into_iter().enumerate() {
                        assert_eq!(machine.peek(0xff0000 + index as u32), Some(value));
                        assert_eq!(machine.peek(0x300001 + index as u32 * 2), Some(value));
                        assert_eq!(machine.peek(0x3f0001 + index as u32 * 2), Some(value));
                    }
                }
            }
        }
    }
}

#[test]
fn unqualified_control_values_lanes_aliases_and_reads_preserve_mapping_and_cells() {
    for size in [8192, 32768] {
        for enable in [false, true] {
            for (address, value, word, read) in [
                (0xa130f1, 3, false, false),
                (0xa130f1, 0x81, false, false),
                (0xa130f0, 1, false, false),
                (0xa130f3, 1, false, false),
                (0xa130f0, 3, true, false),
                (0xa130f1, 0, false, true),
            ] {
                let mut rom = Rom::new(size);
                if enable {
                    rom.enable();
                }
                rom.phase(1);
                if read {
                    rom.read_byte(address, 0xff0000);
                } else if word {
                    rom.write_word(address, value);
                } else {
                    rom.byte(address, value as u8);
                }
                let mut machine = Emulator::new(
                    switched(rom.finish(), product(size)),
                    config(Region::Pal, true),
                )
                .unwrap();
                run_to_phase(&mut machine, 1);
                assert!(machine.step_instruction().is_err());
                assert_eq!(machine.dump_save_ram(), Some(vec![0xff; size]));
                assert_eq!(
                    machine.peek(0x200001),
                    Some(if enable { 0xff } else { 0x69 })
                );
            }
        }
    }
}

#[test]
fn enabled_reset_domains_and_rejected_state_preserve_cells() {
    for size in [8192, 32768] {
        let mut rom = Rom::new(size);
        rom.enable();
        rom.word(0x4e70);
        let bytes = switched(rom.finish(), product(size));
        for concurrent in [false, true] {
            let mut machine =
                Emulator::new(bytes.clone(), config(Region::Pal, concurrent)).unwrap();
            let cells = expected(size, 0x37);
            machine.load_save_ram(&cells).unwrap();
            run_to_phase(&mut machine, 3);
            assert_eq!(machine.reset_pulses(), 1);
            assert_eq!(machine.peek(0x200001), Some(cells[0]));
            let mut frontend = machine.clone();
            frontend.reset();
            assert_eq!(frontend.peek(0x200001), Some(0x69));
            assert_eq!(frontend.dump_save_ram(), Some(cells.clone()));
            machine.reset_cpu().unwrap();
            assert_eq!(machine.peek(0x200001), Some(cells[0]));
            let saved = machine.encode_state().unwrap();
            let mut invalid = saved.clone();
            invalid[crate::state_layout::save_range(&saved).start + 1] = 2;
            let before = machine.clone();
            assert!(machine.decode_state(&invalid).is_err());
            assert_eq!(machine, before);
            let disabled = frontend.encode_state().unwrap();
            machine.decode_state(&disabled).unwrap();
            frontend.decode_state(&disabled).unwrap();
            assert_eq!(machine, frontend);
            assert_eq!(machine.peek(0x200001), Some(0x69));
            assert_eq!(machine.dump_save_ram(), Some(cells));
        }
    }
}

#[test]
fn pending_dma_restore_observes_rom_or_mirrored_save_mapping() {
    for size in [8192, 32768] {
        for enabled in [false, true] {
            for address in [0x200000, 0x204000, 0x300000, 0x3ffffe] {
                let mut rom = Rom::new(size);
                if enabled {
                    rom.enable();
                }
                let source_word = address / 2;
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
                let mut machine = Emulator::new(
                    switched(rom.finish(), product(size)),
                    config(Region::Pal, true),
                )
                .unwrap();
                let cells = expected(size, 0x37);
                machine.load_save_ram(&cells).unwrap();
                while machine.registers().pc != cut {
                    machine.step_instruction().unwrap();
                }
                let state = machine.encode_state().unwrap();
                let mut restored = machine.clone();
                machine.decode_state(&state).unwrap();
                restored.decode_state(&state).unwrap();
                run_to_phase(&mut machine, 3);
                run_to_phase(&mut restored, 3);
                assert_eq!(machine, restored);
                let expected = if enabled {
                    [
                        0xff,
                        cells[((address - 0x200000) as usize / 2) & (size - 1)],
                    ]
                } else if address < 0x300000 {
                    [0x69; 2]
                } else {
                    [0xff; 2]
                };
                assert_eq!(&machine.vdp().memory().vram()[0x1000..0x1002], &expected);
            }
        }
    }
}

#[test]
fn uploaded_z80_banked_reads_use_mapping_and_writes_remain_guarded() {
    for size in [8192, 32768] {
        for enable in [false, true] {
            for address in [0x200001, 0x3fffff] {
                for direction in [Direction::Read, Direction::Write] {
                    let bank = address & !0x7fff;
                    let sound_address = (0x8000 | (address & 0x7fff)) as u16;
                    let mut sound = Vec::new();
                    for bit in 0..9 {
                        sound.extend([0x3e, ((bank >> (15 + bit)) & 1) as u8, 0x32, 0, 0x60]);
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
                    if enable {
                        rom.enable();
                    }
                    rom.write_word(0xa11100, 0x0100);
                    rom.write_word(0xa11200, 0x0100);
                    for (offset, value) in sound.into_iter().enumerate() {
                        rom.byte(0xa00000 + offset as u32, value);
                    }
                    rom.write_word(0xa11100, 0);
                    let mut machine = Emulator::new(
                        switched(rom.finish(), product(size)),
                        config(Region::NtscUs, true),
                    )
                    .unwrap();
                    let mut fault = None;
                    for _ in 0..1000 {
                        if let Err(error) = machine.step_instruction() {
                            fault = Some(error);
                            break;
                        }
                    }
                    if direction == Direction::Write {
                        assert_eq!(
                            fault,
                            Some(MachineFault::Sound(SoundFault::UnsupportedAccess {
                                address: sound_address,
                                direction
                            }))
                        );
                    } else {
                        assert_eq!(fault, None);
                        assert_eq!(
                            machine.sound_cpu().regs().a,
                            if enable || address >= 0x300000 {
                                0xff
                            } else {
                                0x69
                            }
                        );
                    }
                    assert_eq!(machine.dump_save_ram(), Some(vec![0xff; size]));
                }
            }
        }
    }
}
