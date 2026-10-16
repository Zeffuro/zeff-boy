use zeff_emu_common::time::Reset;
use zeff_md_core::{Emulator, LoadConfig, Region};

struct Rom {
    bytes: Vec<u8>,
    pc: usize,
}

impl Rom {
    fn new() -> Self {
        let mut bytes = vec![0; 0x8000];
        bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
        bytes[0x1b0..0x1bc]
            .copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20, 3, 0xff]);
        Self { bytes, pc: 0x200 }
    }

    fn word(&mut self, value: u16) {
        self.bytes[self.pc..self.pc + 2].copy_from_slice(&value.to_be_bytes());
        self.pc += 2;
    }

    fn long(&mut self, value: u32) {
        self.word((value >> 16) as u16);
        self.word(value as u16);
    }

    fn byte(&mut self, address: u32, value: u8) {
        self.word(0x13fc);
        self.word(u16::from(value));
        self.long(address);
    }

    fn read_byte(&mut self, address: u32, destination: u32) {
        self.word(0x1039);
        self.long(address);
        self.word(0x13c0);
        self.long(destination);
    }

    fn write_word(&mut self, address: u32, value: u16) {
        self.word(0x33fc);
        self.word(value);
        self.long(address);
    }

    fn finish(mut self) -> Vec<u8> {
        self.byte(0xff1000, 0x7a);
        self.word(0x60fe);
        self.bytes
    }
}

fn run(machine: &mut Emulator) {
    for _ in 0..100_000 {
        machine.step_instruction().unwrap();
        if machine.peek(0xff1000) == Some(0x7a) {
            return;
        }
    }
    panic!("save ROM did not finish");
}

#[test]
fn original_cpu_rom_writes_and_reads_sparse_odd_save_cells() {
    let mut rom = Rom::new();
    rom.byte(0xa130f1, 1);
    for (index, value) in [(0, 0x11), (255, 0x6d), (511, 0xa7)] {
        rom.byte(0x200001 + index * 2, value);
        rom.read_byte(0x200001 + index * 2, 0xff0000 + index);
    }
    rom.byte(0x200000, 0x22);
    rom.byte(0x200400, 0x33);
    rom.read_byte(0x200000, 0xff0200);
    rom.read_byte(0x200400, 0xff0201);
    let mut machine = Emulator::new(rom.finish(), LoadConfig::default()).unwrap();
    assert_eq!(machine.peek(0x200001), None);
    run(&mut machine);
    let mut expected = vec![0xff; 512];
    expected[0] = 0x11;
    expected[255] = 0x6d;
    expected[511] = 0xa7;
    assert_eq!(machine.dump_save_ram(), Some(expected));
    for (address, value) in [
        (0xff0000, 0x11),
        (0xff00ff, 0x6d),
        (0xff01ff, 0xa7),
        (0xff0200, 0xff),
        (0xff0201, 0xff),
    ] {
        assert_eq!(machine.peek(address), Some(value));
    }
}

fn full_rom(seed: u8) -> (Vec<u8>, Vec<u8>) {
    let mut rom = Rom::new();
    rom.byte(0xa130f1, 1);
    for value in [0x85, 0x06, 0x90] {
        rom.byte(0xc00011, value);
    }
    rom.word(0x207c);
    rom.long(0x200001);
    let expected: Vec<_> = (0..512)
        .map(|index| (index as u8).wrapping_mul(37).wrapping_add(seed) ^ (index >> 8) as u8)
        .collect();
    for (index, group) in expected.as_chunks::<4>().0.iter().enumerate() {
        rom.word(0x203c);
        rom.long(u32::from_be_bytes(*group));
        rom.word(0x01c8);
        rom.word(index as u16 * 8);
        rom.word(0x0148);
        rom.word(index as u16 * 8);
        rom.word(0x23c0);
        rom.long(0xff0000 + index as u32 * 4);
    }
    rom.byte(0xa130f1, 1);
    (rom.finish(), expected)
}

fn full_machine(seed: u8, region: Region, concurrent_sound: bool) -> Emulator {
    let (rom, expected) = full_rom(seed);
    let mut machine = Emulator::new(
        rom,
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap();
    run(&mut machine);
    assert_eq!(machine.dump_save_ram(), Some(expected.clone()));
    for (index, value) in expected.into_iter().enumerate() {
        assert_eq!(machine.peek(0xff0000 + index as u32), Some(value));
        assert_eq!(machine.peek(0x200001 + index as u32 * 2), Some(value));
        assert_eq!(machine.peek(0x200000 + index as u32 * 2), Some(0xff));
    }
    machine
}

#[test]
fn every_cell_roundtrips_through_real_movep_in_all_regions_and_schedulers() {
    for seed in [0x19, 0xe6] {
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            for concurrent in [false, true] {
                full_machine(seed, region, concurrent);
            }
        }
    }
}

#[test]
fn save_import_reset_and_state_are_transactional_and_media_bound() {
    let mut machine = full_machine(0x19, Region::NtscUs, true);
    let saved = machine.encode_state().unwrap();
    let expected = machine.dump_save_ram().unwrap();
    for len in [0, 511, 513, 1024] {
        assert!(machine.load_save_ram(&vec![0; len]).is_err());
        assert_eq!(machine.encode_state().unwrap(), saved);
    }
    for cut in [0, 12, saved.len() - 1] {
        assert!(machine.decode_state(&saved[..cut]).is_err());
        assert_eq!(machine.encode_state().unwrap(), saved);
    }
    for (offset, value) in [(saved.len() - 514, 0), (saved.len() - 513, 2)] {
        let mut bad = saved.clone();
        bad[offset] = value;
        assert!(machine.decode_state(&bad).is_err());
        assert_eq!(machine.encode_state().unwrap(), saved);
    }
    let mut old = saved[..saved.len() - 518].to_vec();
    old[8..12].copy_from_slice(&14_u32.to_le_bytes());
    assert!(
        machine
            .decode_state(&old)
            .unwrap_err()
            .to_string()
            .contains("no cartridge save RAM")
    );
    assert_eq!(machine.encode_state().unwrap(), saved);
    machine.reset();
    assert_eq!(machine.dump_save_ram(), Some(expected.clone()));
    assert_eq!(machine.peek(0x200001), None);
    machine.load_save_ram(&[0x42; 512]).unwrap();
    assert_eq!(machine.peek(0x200001), None);
    machine.decode_state(&saved).unwrap();
    assert_eq!(machine.dump_save_ram(), Some(expected));
    assert_eq!(machine.encode_state().unwrap(), saved);
    let mut clone = machine.clone();
    clone.decode_state(&saved).unwrap();
    machine.decode_state(&saved).unwrap();
    for _ in 0..1000 {
        assert_eq!(machine.step_instruction(), clone.step_instruction());
    }
    assert_eq!(
        machine.encode_state().unwrap(),
        clone.encode_state().unwrap()
    );
    assert_eq!(machine.framebuffer(), clone.framebuffer());
    assert_eq!(machine.drain_audio_samples(), clone.drain_audio_samples());
    clone.load_save_ram(&[0xa5; 512]).unwrap();
    assert_ne!(machine.dump_save_ram(), clone.dump_save_ram());
    assert_eq!(machine.rom_bytes(), clone.rom_bytes());
}

#[test]
fn unsupported_boards_controls_and_initial_mapping_remain_guarded() {
    let mut bytes = Rom::new().finish();
    for (index, value) in [(0x1b2, 0xf0), (0x1b3, 0x40), (0x1b7, 0), (0x1bb, 0xfd)] {
        let mut bad = bytes.clone();
        bad[index] = value;
        assert!(Emulator::new(bad, LoadConfig::default()).is_err());
    }
    let mut overlap = bytes.clone();
    overlap.resize(0x200002, 0);
    assert!(Emulator::new(overlap, LoadConfig::default()).is_err());
    for (address, value) in [
        (0x200001, 0x42),
        (0xa130f1, 0),
        (0xa130f1, 2),
        (0xa130f1, 3),
        (0xa130f1, 0x81),
        (0xa130f0, 1),
    ] {
        let mut rom = Rom::new();
        rom.byte(address, value);
        let mut machine = Emulator::new(rom.finish(), LoadConfig::default()).unwrap();
        assert!(
            machine.step_instruction().is_err(),
            "{address:06x}={value:02x}"
        );
        assert_eq!(machine.dump_save_ram(), Some(vec![0xff; 512]));
        assert_eq!(machine.peek(0x200001), None);
    }
    for address in [0x200000, 0x200001, 0xa130f1] {
        let mut rom = Rom::new();
        rom.read_byte(address, 0xff0000);
        let mut machine = Emulator::new(rom.finish(), LoadConfig::default()).unwrap();
        assert!(machine.step_instruction().is_err());
        assert_eq!(machine.peek(0xff0000), Some(0));
    }
    let mut rom = Rom::new();
    rom.write_word(0xa130f0, 1);
    let mut guarded = Emulator::new(rom.finish(), LoadConfig::default()).unwrap();
    assert!(guarded.step_instruction().is_err());
    assert_eq!(guarded.peek(0x200001), None);
    bytes[0x1b0..0x1bc].fill(0);
    let mut plain = Emulator::new(bytes, LoadConfig::default()).unwrap();
    assert!(plain.load_save_ram(&[0; 512]).is_err());
    let mut old = plain.encode_state().unwrap();
    old.truncate(old.len() - 5);
    old[8..12].copy_from_slice(&14_u32.to_le_bytes());
    plain.decode_state(&old).unwrap();
    assert_eq!(plain.dump_save_ram(), None);
}

#[test]
fn ordinary_word_long_and_edge_lanes_preserve_ordered_cells() {
    let mut rom = Rom::new();
    rom.byte(0xa130f1, 1);
    rom.word(0x4e70);
    rom.write_word(0x200000, 0x1234);
    rom.word(0x23fc);
    rom.long(0x56789abc);
    rom.long(0x200002);
    rom.word(0x23fc);
    rom.long(0x11223344);
    rom.long(0x2003fe);
    for (index, address) in [
        0x200000, 0x200001, 0x200003, 0x200005, 0x2003ff, 0x200400, 0x200401,
    ]
    .into_iter()
    .enumerate()
    {
        rom.read_byte(address, 0xff0000 + index as u32);
    }
    let mut machine = Emulator::new(rom.finish(), LoadConfig::default()).unwrap();
    run(&mut machine);
    let mut expected = vec![0xff; 512];
    expected[..3].copy_from_slice(&[0x34, 0x78, 0xbc]);
    expected[511] = 0x22;
    assert_eq!(machine.dump_save_ram(), Some(expected));
    for (index, value) in [0xff, 0x34, 0x78, 0xbc, 0x22, 0xff, 0xff]
        .into_iter()
        .enumerate()
    {
        assert_eq!(machine.peek(0xff0000 + index as u32), Some(value));
    }
}

#[test]
fn guest_vdp_dma_reads_every_odd_cell_and_restored_mapping() {
    let mut rom = Rom::new();
    rom.byte(0xa130f1, 1);
    for (index, value) in [
        (1, 0x14),
        (15, 2),
        (19, 0),
        (20, 2),
        (21, 0),
        (22, 0),
        (23, 0x10),
    ] {
        rom.write_word(0xc00004, 0x8000 | index << 8 | value);
    }
    rom.write_word(0xc00004, 0x5000);
    let cut_pc = rom.pc as u32;
    rom.write_word(0xc00004, 0x0080);
    let bytes = rom.finish();
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut machine = Emulator::new(
            bytes.clone(),
            LoadConfig {
                region,
                concurrent_sound: true,
            },
        )
        .unwrap();
        let expected: Vec<_> = (0..512)
            .map(|i| (i as u8).wrapping_mul(17) ^ (i >> 8) as u8)
            .collect();
        machine.load_save_ram(&expected).unwrap();
        while machine.registers().pc != cut_pc {
            machine.step_instruction().unwrap();
        }
        let state = machine.encode_state().unwrap();
        let mut restored = Emulator::new(
            bytes.clone(),
            LoadConfig {
                region,
                concurrent_sound: true,
            },
        )
        .unwrap();
        restored.decode_state(&state).unwrap();
        machine.decode_state(&state).unwrap();
        for _ in 0..8 {
            assert_eq!(machine.step_instruction(), restored.step_instruction());
        }
        assert_eq!(
            machine.encode_state().unwrap(),
            restored.encode_state().unwrap()
        );
        for (index, value) in expected.into_iter().enumerate() {
            assert_eq!(
                &machine.vdp().memory().vram()[0x1000 + index * 2..0x1002 + index * 2],
                &[0xff, value]
            );
        }
    }
}

#[test]
fn banked_z80_mutable_save_reads_are_explicitly_unsupported() {
    let mut rom = Rom::new();
    rom.byte(0xa130f1, 1);
    rom.write_word(0xa11100, 0x0100);
    rom.write_word(0xa11200, 0x0100);
    let mut sound = Vec::new();
    for bit in [0, 0, 0, 0, 0, 0, 1, 0, 0] {
        sound.extend([0x3e, bit, 0x32, 0x00, 0x60]);
    }
    sound.extend([0x3a, 0x01, 0x80, 0x76]);
    for (index, value) in sound.into_iter().enumerate() {
        rom.byte(0xa00000 + index as u32, value);
    }
    rom.write_word(0xa11200, 0x0100);
    rom.write_word(0xa11100, 0);
    let bytes = rom.finish();
    let mut machine = Emulator::new(
        bytes,
        LoadConfig {
            concurrent_sound: true,
            ..LoadConfig::default()
        },
    )
    .unwrap();
    let mut fault = None;
    for _ in 0..1000 {
        if let Err(error) = machine.step_instruction() {
            fault = Some(error);
            break;
        }
    }
    assert!(
        matches!(
            fault,
            Some(zeff_md_core::MachineFault::Sound(
                zeff_md_core::SoundFault::UnsupportedAccess {
                    address: 0x8001,
                    ..
                }
            ))
        ),
        "fault={fault:?}, bank={:x}",
        machine.sound_bank()
    );
    assert_eq!(machine.dump_save_ram(), Some(vec![0xff; 512]));
}

#[test]
fn save_declaration_validation_is_independent_of_optional_metadata() {
    let mut bytes = Rom::new().finish();
    bytes[4..8].copy_from_slice(&0x100_u32.to_be_bytes());
    bytes[0x100..0x104].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
    for length in [0x1b2, 0x1b3, 0x1b8, 0x1bb] {
        assert!(matches!(
            Emulator::new(bytes[..length].to_vec(), LoadConfig::default()),
            Err(zeff_md_core::LoadError::UnsupportedSaveHardware)
        ));
    }
    for length in [0x1bc, 0x1ff, 0x200] {
        let machine = Emulator::new(bytes[..length].to_vec(), LoadConfig::default()).unwrap();
        assert_eq!(machine.cartridge_header().is_some(), length >= 0x200);
        assert_eq!(machine.dump_save_ram(), Some(vec![0xff; 512]));
    }
    let plain = Emulator::new(bytes[..0x1b1].to_vec(), LoadConfig::default()).unwrap();
    assert_eq!(plain.dump_save_ram(), None);
}
