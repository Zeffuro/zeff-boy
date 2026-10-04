use super::*;
use crate::hardware::cartridge::Cartridge;

pub(super) fn rom(mapper: u8, chr_rom: bool) -> Vec<u8> {
    let mut bytes = vec![0; 16 + 0x10000 + usize::from(chr_rom) * 0x10000];
    bytes[..4].copy_from_slice(b"NES\x1a");
    bytes[4] = 4;
    bytes[5] = if chr_rom { 8 } else { 0 };
    bytes[6] = (mapper << 4) | 2;
    bytes[7] = mapper & 0xf0;
    let reset = [
        0x78, 0xd8, 0xa2, 0xff, 0x9a, 0xa9, 1, 0x8d, 0x15, 0x40, 0xa9, 0xbf, 0x8d, 0, 0x40, 0xa9,
        0x60, 0x8d, 2, 0x40, 0xa9, 8, 0x8d, 3, 0x40, 0xa9, 0x80, 0x8d, 0, 0x20, 0xa9, 0x1e, 0x8d,
        1, 0x20, 0x4c, 0x22, 0xe0,
    ];
    let nmi = [
        0x48, 0xa9, 1, 0x8d, 0x16, 0x40, 0xa9, 0, 0x8d, 0x16, 0x40, 0xad, 0x16, 0x40, 0x85, 0,
        0x8d, 2, 0x40, 0x68, 0x40,
    ];
    for bank in bytes[16..16 + 0x10000].as_chunks_mut::<0x2000>().0 {
        bank[..reset.len()].copy_from_slice(&reset);
        bank[0x100..0x100 + nmi.len()].copy_from_slice(&nmi);
        bank[0x200] = 0x40;
        bank[0x1ffa..].copy_from_slice(&[0, 0xe1, 0, 0xe0, 0, 0xe2]);
    }
    bytes
}

fn mutate_mapper(core: &mut Emulator, seed: u8) {
    let cartridge = &mut core.bus.cartridge;
    for (index, address) in [
        0x4100, 0x4800, 0x5000, 0x5100, 0x5101, 0x5104, 0x5105, 0x5114, 0x5120, 0x5203, 0x5204,
        0x5c00, 0x6000, 0x6001, 0x6004, 0x6006, 0x7000, 0x7ef0, 0x7ef8, 0x7efd, 0x7ffe, 0x8000,
        0x8001, 0x8002, 0x8010, 0x9000, 0x9010, 0x9030, 0xa000, 0xa001, 0xb000, 0xb001, 0xc000,
        0xc001, 0xd000, 0xd001, 0xe000, 0xe001, 0xf000, 0xf001, 0xf800,
    ]
    .into_iter()
    .enumerate()
    {
        cartridge.cpu_write(address, (index as u8).wrapping_add(seed) & 0x7f);
        cartridge.clock_cpu();
        cartridge.clock_cpu();
    }
    for address in [0, 0xfd8, 0xfe8, 0x1000, 0x1fd8, 0x1fe8] {
        cartridge.chr_write(address, seed);
        cartridge.chr_read(address);
    }
    let mut ciram = [0; 0x1000];
    for address in [0x2000, 0x23c0, 0x2400, 0x2800, 0x2c00] {
        cartridge.ppu_nametable_write(address, seed, &mut ciram);
        cartridge.ppu_nametable_read(address, &ciram);
    }
    for _ in 0..300 {
        cartridge.clock_cpu();
    }
}

#[test]
fn every_loaded_fixed_mapper_replays_corrected_frames() {
    let mut admitted = 0;
    let mut variants = std::collections::HashSet::new();
    let mut corrected_variants = std::collections::HashSet::new();
    for mapper in 0..=255 {
        for chr_rom in [false, true] {
            let bytes = rom(mapper, chr_rom);
            let Ok(cartridge) = Cartridge::load(&bytes) else {
                continue;
            };
            let variant = cartridge.rollback_mapper_variant();
            variants.insert(variant);
            let mut subject = Emulator::new(&bytes, 48_000.0).unwrap();
            if mapper == 20 || mapper == 99 {
                assert!(subject.begin_rollback_session().is_err());
                assert!(!subject.has_portable_rollback_hardware());
                continue;
            }
            let mut control = Emulator::new(&bytes, 48_000.0).unwrap();
            let Ok(session) = subject.begin_rollback_session() else {
                assert!(!subject.has_portable_rollback_hardware());
                continue;
            };
            assert_eq!(subject.has_portable_rollback_hardware(), mapper != 85);
            admitted += 1;
            corrected_variants.insert(variant);
            mutate_mapper(&mut subject, 3);
            mutate_mapper(&mut control, 3);
            let snapshot = session.capture(&subject).unwrap();
            let before = observe(&subject, Vec::new());
            mutate_mapper(&mut subject, 17);
            for _ in 0..8 {
                session.advance_frame(&mut subject, [0xff, 0]).unwrap();
            }
            let expected: Vec<_> = (0..4)
                .map(|frame| {
                    let audio = ordinary_frame(&mut control, input(frame));
                    observe(&control, audio)
                })
                .collect();
            for _ in 0..2 {
                session.restore(&mut subject, &snapshot).unwrap();
                assert!(
                    observe(&subject, Vec::new()) == before,
                    "restore mapper {mapper}"
                );
                for (frame, reference) in expected.iter().enumerate() {
                    let audio = session.advance_frame(&mut subject, input(frame)).unwrap();
                    assert!(
                        &observe(&subject, audio) == reference,
                        "mapper {mapper}, CHR ROM {chr_rom}, frame {frame}"
                    );
                }
            }
        }
    }
    assert!(
        admitted >= 120,
        "mapper coverage unexpectedly shrank: {admitted}"
    );
    assert_eq!(variants.len(), 64);
    assert_eq!(corrected_variants.len(), 62);
    println!(
        "covered {} variants, {} corrected variants, {admitted} cartridge layouts",
        variants.len(),
        corrected_variants.len()
    );
}

fn with_mapper(mapper: u8) -> (Emulator, NesRollbackSession) {
    let mut core = Emulator::new(&rom(mapper, true), 48_000.0).unwrap();
    let session = core.begin_rollback_session().unwrap();
    (core, session)
}

fn failed_load_preserves_runtime(core: &mut Emulator) {
    let before = observe(core, Vec::new());
    let mut state = core.encode_state().unwrap();
    let mut payload = lz4_flex::decompress_size_prepended(&state[12..]).unwrap();
    payload.push(0);
    state.truncate(12);
    state.extend(lz4_flex::compress_prepend_size(&payload));
    assert!(core.load_state(&state).is_err());
    assert_eq!(observe(core, Vec::new()), before);
}

#[test]
fn interrupted_mmc1_serial_write_keeps_suppression() {
    let (mut core, session) = with_mapper(1);
    core.bus.cartridge.cpu_write(0xe000, 1);
    let snapshot = session.capture(&core).unwrap();
    failed_load_preserves_runtime(&mut core);
    core.bus.cartridge.clock_cpu();
    core.bus.cartridge.clock_cpu();
    core.bus.cartridge.cpu_write(0xe000, 0);
    session.restore(&mut core, &snapshot).unwrap();
    let native = core.encode_state().unwrap();
    core.bus.cartridge.cpu_write(0xe000, 0);
    assert_eq!(core.encode_state().unwrap(), native);
    core.bus.cartridge.clock_cpu();
    core.bus.cartridge.clock_cpu();
    core.bus.cartridge.cpu_write(0xe000, 0);
    assert_ne!(core.encode_state().unwrap(), native);
}

#[test]
fn mmc5_restores_nametable_threshold_before_irq() {
    let (mut core, session) = with_mapper(5);
    core.bus.cartridge.cpu_write(0x5203, 1);
    core.bus.cartridge.cpu_write(0x5204, 0x80);
    let ciram = [0; 0x1000];
    for _ in 0..2 {
        core.bus.cartridge.ppu_nametable_read(0x2000, &ciram);
    }
    let snapshot = session.capture(&core).unwrap();
    failed_load_preserves_runtime(&mut core);
    core.bus.cartridge.chr_read(0);
    session.restore(&mut core, &snapshot).unwrap();
    assert!(!core.bus.cartridge.irq_pending());
    core.bus.cartridge.ppu_nametable_read(0x2000, &ciram);
    assert!(core.bus.cartridge.irq_pending());
}

fn eeprom_write(core: &mut Emulator, value: u8) {
    core.bus.cartridge.cpu_write(0x800d, value | 0x80);
}

fn eeprom_byte(core: &mut Emulator, value: u8) {
    for bit in (0..8).rev() {
        let sda = ((value >> bit) & 1) << 6;
        eeprom_write(core, sda);
        eeprom_write(core, sda | 0x20);
    }
}

#[test]
fn eeprom_restores_pending_ack_and_clocked_read_bit() {
    let (mut core, session) = with_mapper(16);
    eeprom_write(&mut core, 0x60);
    eeprom_write(&mut core, 0x20);
    eeprom_byte(&mut core, 0xa1);
    let pending = session.capture(&core).unwrap();
    failed_load_preserves_runtime(&mut core);
    assert_eq!(core.cpu_peek(0x6000), 0);
    eeprom_write(&mut core, 0);
    eeprom_write(&mut core, 0x20);
    eeprom_write(&mut core, 0);
    assert_eq!(core.cpu_peek(0x6000), 0x10);
    session.restore(&mut core, &pending).unwrap();
    assert_eq!(core.cpu_peek(0x6000), 0);
    eeprom_write(&mut core, 0);
    eeprom_write(&mut core, 0x20);
    eeprom_write(&mut core, 0);
    eeprom_write(&mut core, 0x20);
    let clocked = session.capture(&core).unwrap();
    failed_load_preserves_runtime(&mut core);
    eeprom_write(&mut core, 0);
    let expected = observe(&core, Vec::new());
    session.restore(&mut core, &clocked).unwrap();
    eeprom_write(&mut core, 0);
    assert_eq!(observe(&core, Vec::new()), expected);
}

fn expansion_audio_write(core: &mut Emulator, mapper: u8, register: u8, value: u8) {
    let (address, data) = if mapper == 19 {
        (0xf800, 0x4800)
    } else {
        (0x9010, 0x9030)
    };
    core.bus.cartridge.cpu_write(address, register);
    core.bus.cartridge.cpu_write(data, value);
}

#[test]
fn expansion_audio_restores_held_output_and_next_pcm() {
    for mapper in [19, 85] {
        let (mut subject, session) = with_mapper(mapper);
        let mut control = Emulator::new(&rom(mapper, true), 48_000.0).unwrap();
        for core in [&mut subject, &mut control] {
            if mapper == 19 {
                for (register, value) in [
                    (0, 0xff),
                    (0x78, 0xff),
                    (0x7a, 0xff),
                    (0x7c, 1),
                    (0x7f, 0x0f),
                ] {
                    expansion_audio_write(core, mapper, register, value);
                }
            } else {
                for (register, value) in [(0x10, 0xff), (0x30, 0x10), (0x20, 0x1f)] {
                    expansion_audio_write(core, mapper, register, value);
                }
            }
            for _ in 0..10_001 {
                core.bus.cartridge.clock_cpu();
            }
        }
        assert_ne!(
            subject.bus.cartridge.audio_output().to_bits(),
            0,
            "mapper {mapper}"
        );
        let snapshot = session.capture(&subject).unwrap();
        let before = observe(&subject, Vec::new());
        failed_load_preserves_runtime(&mut subject);
        for _ in 0..91 {
            subject.bus.cartridge.clock_cpu();
        }
        session.restore(&mut subject, &snapshot).unwrap();
        assert_eq!(observe(&subject, Vec::new()), before);
        assert_eq!(
            subject.bus.cartridge.audio_output().to_bits(),
            control.bus.cartridge.audio_output().to_bits()
        );
        for frame in 0..3 {
            let expected = ordinary_frame(&mut control, input(frame));
            let actual = session.advance_frame(&mut subject, input(frame)).unwrap();
            assert_eq!(
                observe(&subject, actual),
                observe(&control, expected),
                "mapper {mapper}"
            );
        }
    }
}
