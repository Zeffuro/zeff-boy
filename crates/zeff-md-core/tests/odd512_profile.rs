#[path = "support/state_layout.rs"]
mod state_layout;

use zeff_emu_common::time::Reset;
use zeff_m68k::bus::Direction;
use zeff_md_core::{Emulator, LoadConfig, LoadError, MachineFault, Region, SoundFault};

#[path = "odd512_profile/fixture.rs"]
mod fixture;

use fixture::{PRODUCTS, Rom, TABLE, checksum, config, initialization_program, pattern, until};

#[test]
fn tiny_cmpi_and_memory_read_use_actual_seeded_cells_before_enable() {
    for product in PRODUCTS {
        for marker in [0x12, 0x71] {
            let mut rom = Rom::new(product);
            rom.word(0x0c39);
            rom.word(0x12);
            rom.long(0x200003);
            rom.word(0x1039);
            rom.long(0x200003);
            let mut machine = Emulator::new(rom.finish(), LoadConfig::default()).unwrap();
            let mut cells = pattern();
            cells[1] = marker;
            machine.load_save_ram(&cells).unwrap();
            assert_eq!(machine.peek(0x200003), Some(marker));
            machine.step_instruction().unwrap();
            assert_eq!(machine.registers().pc, 0x208);
            assert_eq!(
                machine.registers().status_register() & 4 != 0,
                marker == 0x12
            );
            machine.step_instruction().unwrap();
            assert_eq!(machine.registers().pc, 0x20e);
            assert_eq!(machine.registers().d[0] as u8, marker);
            assert_eq!(machine.dump_save_ram(), Some(cells));
        }
    }
}

#[test]
fn literal_odd_stride_clear_team_copy_checksum_reuse_and_repair() {
    for (product, region) in
        PRODUCTS
            .into_iter()
            .zip([Region::NtscJapan, Region::Pal, Region::NtscJapan])
    {
        for concurrent in [false, true] {
            let program = initialization_program(product);
            for mode in 0..4 {
                let mut cells = pattern();
                cells[1] = if mode == 0 { 0x73 } else { 0x12 };
                cells[2] = checksum(&cells).wrapping_add(u8::from(mode == 2));
                if mode == 3 {
                    cells.fill(0xff);
                }
                let mut machine =
                    Emulator::new(program.bytes.clone(), config(region, concurrent)).unwrap();
                machine.load_save_ram(&cells).unwrap();
                if mode != 1 {
                    until(&mut machine, program.clear_end);
                    assert_eq!(machine.dump_save_ram(), Some(vec![0; 512]));
                    assert_eq!(machine.registers().a[2], 0x200401);
                    until(&mut machine, program.copy_end);
                    let mut copied = program.initialized.clone();
                    copied[2] = 0;
                    assert_eq!(machine.dump_save_ram(), Some(copied));
                    assert_eq!(machine.registers().a[5], TABLE + 512);
                    assert_eq!(machine.registers().a[6], 0x20000b + 416 * 2);
                }
                until(&mut machine, program.done);
                machine.step_instruction().unwrap();
                let expected = if mode == 1 {
                    cells
                } else {
                    program.initialized.clone()
                };
                assert_eq!(machine.dump_save_ram(), Some(expected.clone()));
                assert_eq!(machine.peek(0xff1000), Some(if mode == 1 { 2 } else { 1 }));
                assert_eq!(machine.peek(0x200005), Some(checksum(&expected)));
                assert_eq!(machine.registers().a[2], 0x2003ff);
            }
        }
    }
}

#[test]
fn every_product_byte_size_declaration_and_other_board_boundary_is_guarded() {
    for product in PRODUCTS {
        let bytes = Rom::new(product).finish();
        for offset in 0x180..0x18e {
            let mut bad = bytes.clone();
            bad[offset] ^= 0x80;
            let machine = Emulator::new(bad, LoadConfig::default()).unwrap();
            assert_eq!(machine.peek(0x200001), None, "offset={offset:x}");
        }
        for size in [0x8000, 0xffffe, 0x100002, 0x200000] {
            let mut bad = bytes.clone();
            bad.resize(size, 0);
            assert_eq!(
                Emulator::new(bad, LoadConfig::default())
                    .unwrap()
                    .peek(0x200001),
                None
            );
        }
        for offset in 0x1b0..0x1bc {
            let mut bad = bytes.clone();
            bad[offset] ^= 1;
            if offset < 0x1b2 {
                assert_eq!(
                    Emulator::new(bad, LoadConfig::default())
                        .unwrap()
                        .dump_save_ram(),
                    None
                );
            } else {
                assert_eq!(
                    Emulator::new(bad, LoadConfig::default()).unwrap_err(),
                    LoadError::UnsupportedSaveHardware
                );
            }
        }
        for size in [0x200002, 0x300000, 0x400000] {
            let mut bad = bytes.clone();
            bad.resize(size, 0);
            assert_eq!(
                Emulator::new(bad, LoadConfig::default()).unwrap_err(),
                LoadError::UnsupportedSaveHardware
            );
        }
        for size in [8192, 32768] {
            let mut other = bytes.clone();
            other[0x1ba..0x1bc].copy_from_slice(&((size * 2 - 1) as u16).to_be_bytes());
            let machine = Emulator::new(other, LoadConfig::default()).unwrap();
            assert_eq!(machine.dump_save_ram(), Some(vec![0xff; size]));
            assert_eq!(machine.peek(0x200001), Some(0xff));
        }
    }
    for product in [b"GM MK-1079 -00", b"GM G-00352-00 ", b"GM 00005518-01"] {
        let mut rom = Rom::new(product);
        rom.bytes.resize(0x200000, 0);
        rom.word(0x1039);
        rom.long(0x200003);
        let mut machine = Emulator::new(rom.finish(), LoadConfig::default()).unwrap();
        assert_eq!(machine.peek(0x200003), None);
        assert!(machine.step_instruction().is_err());
        assert_eq!(machine.dump_save_ram(), Some(vec![0xff; 512]));
    }
}

#[test]
fn mapped_profile_preserves_control_lanes_and_unmirrored_aperture() {
    for (address, value, word) in [
        (0xa130f1, 0, false),
        (0xa130f1, 2, false),
        (0xa130f1, 3, false),
        (0xa130f1, 0x81, false),
        (0xa130f0, 1, false),
        (0xa130f0, 1, true),
    ] {
        let mut rom = Rom::new(PRODUCTS[0]);
        if word {
            rom.write_word(address, value);
        } else {
            rom.byte(address, value as u8);
        }
        let mut machine = Emulator::new(rom.finish(), LoadConfig::default()).unwrap();
        assert!(machine.step_instruction().is_err());
        assert_eq!(machine.dump_save_ram(), Some(vec![0xff; 512]));
        assert_eq!(machine.peek(0x200003), Some(0xff));
    }
    let mut rom = Rom::new(PRODUCTS[0]);
    rom.byte(0xa130f1, 1);
    rom.word(0x4e70);
    rom.byte(0x200000, 0x13);
    rom.write_word(0x200000, 0x1234);
    rom.write_word(0x2003fe, 0x5678);
    rom.write_word(0x200400, 0x9abc);
    let done = rom.pc;
    let mut machine = Emulator::new(rom.finish(), LoadConfig::default()).unwrap();
    until(&mut machine, done);
    let mut cells = vec![0xff; 512];
    cells[0] = 0x34;
    cells[511] = 0x78;
    assert_eq!(machine.dump_save_ram(), Some(cells));
    for (address, value) in [
        (0x200000, 0xff),
        (0x200001, 0x34),
        (0x2003ff, 0x78),
        (0x200401, 0xff),
    ] {
        assert_eq!(machine.peek(address), Some(value));
    }
    assert_eq!(machine.reset_pulses(), 1);
}

#[test]
fn reset_clone_exact_import_and_raw20_frontend1_preserve_legacy_false_mapping() {
    for product in PRODUCTS {
        for concurrent in [false, true] {
            let mut rom = Rom::new(product);
            rom.word(0x4e70);
            rom.byte(0xa130f1, 1);
            rom.byte(0x200003, 0x44);
            let done = rom.pc;
            let mut machine = Emulator::new(rom.finish(), config(Region::Pal, concurrent)).unwrap();
            machine.load_save_ram(&pattern()).unwrap();
            until(&mut machine, done);
            let cells = machine.dump_save_ram().unwrap();
            let raw = machine.encode_state().unwrap();
            let front = machine.encode_frontend_state().unwrap();
            assert_eq!(&raw[8..12], &1_u32.to_le_bytes());
            assert_eq!(&front[8..12], &1u32.to_le_bytes());
            let save = state_layout::save_range(&raw);
            let enabled = save.start + 1;
            assert_eq!(&raw[enabled - 5..enabled - 1], &515u32.to_le_bytes());
            assert_eq!(&raw[enabled - 1..enabled + 1], &[1, 1]);
            assert_eq!(&raw[enabled + 1..save.end - 1], cells.as_slice());
            assert_eq!(&front[16..16 + raw.len()], raw.as_slice());
            for frontend in [false, true] {
                let mut restored = machine.clone();
                let mut reference = machine.clone();
                if frontend {
                    restored.decode_frontend_state(&front).unwrap();
                    reference.decode_frontend_state(&front).unwrap();
                    assert_eq!(restored.encode_frontend_state().unwrap(), front);
                } else {
                    restored.decode_state(&raw).unwrap();
                    reference.decode_state(&raw).unwrap();
                    assert_eq!(restored.encode_state().unwrap(), raw);
                }
                for _ in 0..100 {
                    assert_eq!(restored.step_instruction(), reference.step_instruction());
                }
                assert_eq!(restored, reference);
                assert_eq!(
                    restored.drain_audio_samples(),
                    reference.drain_audio_samples()
                );
                restored.load_save_ram(&[0x91; 512]).unwrap();
                assert_eq!(reference.dump_save_ram(), Some(cells.clone()));
                assert_ne!(restored.dump_save_ram(), reference.dump_save_ram());
            }
            for frontend in [false, true] {
                let mut legacy = if frontend { front.clone() } else { raw.clone() };
                legacy[enabled + if frontend { 16 } else { 0 }] = 0;
                if frontend {
                    machine.decode_frontend_state(&legacy).unwrap();
                    assert_eq!(machine.encode_frontend_state().unwrap(), legacy);
                } else {
                    machine.decode_state(&legacy).unwrap();
                    assert_eq!(machine.encode_state().unwrap(), legacy);
                }
                assert_eq!(machine.peek(0x200001), None);
                assert_eq!(machine.peek(0x2003ff), None);
                assert_eq!(machine.dump_save_ram(), Some(cells.clone()));
                machine.reset_cpu().unwrap();
                assert_eq!(machine.peek(0x200003), None);
                machine.step_instruction().unwrap();
                assert_eq!(machine.peek(0x200003), None);
                machine.step_instruction().unwrap();
                assert_eq!(machine.peek(0x200003), Some(cells[1]));
                machine
                    .decode_state(&{
                        let mut disabled = raw.clone();
                        disabled[enabled] = 0;
                        disabled
                    })
                    .unwrap();
                machine.reset();
                assert_eq!(machine.peek(0x200003), Some(cells[1]));
                assert_eq!(machine.dump_save_ram(), Some(cells.clone()));
            }
            machine.decode_frontend_state(&front).unwrap();
            let before = machine.clone();
            for length in [0, 511, 513, 8192] {
                assert!(machine.load_save_ram(&vec![0; length]).is_err());
                assert_eq!(machine, before);
            }
            for frontend in [false, true] {
                let saved = if frontend { &front } else { &raw };
                for cut in [0, 12, saved.len() - 1] {
                    if frontend {
                        assert!(machine.decode_frontend_state(&saved[..cut]).is_err());
                    } else {
                        assert!(machine.decode_state(&saved[..cut]).is_err());
                    }
                    assert_eq!(machine, before);
                }
                let mut bad = saved.clone();
                bad[enabled + if frontend { 16 } else { 0 }] = 2;
                if frontend {
                    assert!(machine.decode_frontend_state(&bad).is_err());
                } else {
                    assert!(machine.decode_state(&bad).is_err());
                }
                assert_eq!(machine, before);
            }
        }
    }
}

#[test]
fn initially_mapped_cells_feed_every_dma_word_after_raw_and_frontend_restore() {
    for (product, region) in
        PRODUCTS
            .into_iter()
            .zip([Region::NtscJapan, Region::Pal, Region::NtscJapan])
    {
        let mut rom = Rom::new(product);
        for (register, value) in [
            (1, 0x14),
            (15, 2),
            (19, 0),
            (20, 2),
            (21, 0),
            (22, 0),
            (23, 0x10),
        ] {
            rom.write_word(0xc00004, 0x8000 | register << 8 | value);
        }
        rom.write_word(0xc00004, 0x5000);
        let cut = rom.pc;
        rom.write_word(0xc00004, 0x0080);
        let done = rom.pc;
        let bytes = rom.finish();
        for concurrent in [false, true] {
            let cells = pattern();
            let mut machine = Emulator::new(bytes.clone(), config(region, concurrent)).unwrap();
            machine.load_save_ram(&cells).unwrap();
            until(&mut machine, cut);
            let raw = machine.encode_state().unwrap();
            let front = machine.encode_frontend_state().unwrap();
            for frontend in [false, true] {
                let mut restored = machine.clone();
                if frontend {
                    restored.decode_frontend_state(&front).unwrap();
                } else {
                    restored.decode_state(&raw).unwrap();
                }
                until(&mut restored, done);
                for _ in 0..16 {
                    restored.step_instruction().unwrap();
                }
                assert_eq!(restored.dump_save_ram(), Some(cells.clone()));
                for (index, value) in cells.iter().enumerate() {
                    assert_eq!(
                        &restored.vdp().memory().vram()[0x1000 + index * 2..0x1002 + index * 2],
                        &[0xff, *value]
                    );
                }
            }
        }
    }
}

#[test]
fn banked_z80_accesses_keep_existing_mutable_aperture_guards() {
    for product in PRODUCTS {
        for address in [0x200001u32, 0x2003ff] {
            for direction in [Direction::Read, Direction::Write] {
                let sound_address = (0x8000 | (address & 0x7fff)) as u16;
                let mut sound = Vec::new();
                for bit in 0..9 {
                    sound.extend([0x3e, ((address >> (15 + bit)) & 1) as u8, 0x32, 0, 0x60]);
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
                let mut rom = Rom::new(product);
                rom.write_word(0xa11100, 0x0100);
                rom.write_word(0xa11200, 0x0100);
                for (offset, value) in sound.into_iter().enumerate() {
                    rom.byte(0xa00000 + offset as u32, value);
                }
                rom.write_word(0xa11200, 0x0100);
                rom.write_word(0xa11100, 0);
                let mut machine =
                    Emulator::new(rom.finish(), config(Region::NtscJapan, true)).unwrap();
                let cells = pattern();
                machine.load_save_ram(&cells).unwrap();
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
                        direction
                    }))
                );
                assert_eq!(machine.dump_save_ram(), Some(cells.clone()));
                assert_eq!(
                    machine.peek(address),
                    Some(cells[((address - 0x200001) / 2) as usize])
                );
            }
        }
    }
}
