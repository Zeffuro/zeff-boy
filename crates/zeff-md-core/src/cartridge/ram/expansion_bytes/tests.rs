use super::*;
use crate::hardware::sound::{CandidateBus, SoundFault, SoundMemory};
use zeff_m68k::bus::Direction;
use zeff_z80::Z80Bus;

const VECTORS: [u8; 8] = [0, 0xff, 0xff, 0xfc, 0, 0, 2, 0];
const MARKER: &[u8; 16] = b"ZEFF EXP64 TEST ";
const PROGRAM: [u16; 19] = [
    0x13fc, 0x0012, 0x0040, 0, 0x13fc, 0x0034, 0x0040, 1, 0x33fc, 0x5678, 0x0040, 4, 0x3039,
    0x0040, 4, 0x0240, 0x00ff, 0x4e71, 0x60fe,
];

pub(super) fn fixture_matches(rom: &[u8]) -> bool {
    rom.len() == 0x400000
        && rom[..8] == VECTORS
        && rom[0x100..0x110] == *MARKER
        && rom[8..0x100].iter().all(|&byte| byte == 0)
        && rom[0x110..0x200].iter().all(|&byte| byte == 0)
        && PROGRAM
            .iter()
            .enumerate()
            .all(|(index, word)| rom[0x200 + 2 * index..0x202 + 2 * index] == word.to_be_bytes())
        && rom[0x200 + 2 * PROGRAM.len()..]
            .iter()
            .all(|&byte| byte == 0)
}

pub(crate) fn fixture_rom() -> Vec<u8> {
    let mut rom = vec![0; 0x400000];
    rom[..8].copy_from_slice(&VECTORS);
    rom[0x100..0x110].copy_from_slice(MARKER);
    for (index, word) in PROGRAM.iter().enumerate() {
        rom[0x200 + 2 * index..0x202 + 2 * index].copy_from_slice(&word.to_be_bytes());
    }
    rom
}

pub(in crate::cartridge::ram) fn contract() {
    for digest in DIGESTS {
        assert!(identity(
            0x400000,
            Some(&HEADER_CHECKSUM),
            Some(&DECLARATION),
            || digest
        ));
    }
    assert!(!identity(
        0x3fffff,
        Some(&HEADER_CHECKSUM),
        Some(&DECLARATION),
        || panic!("wrong-sized save identity hashed")
    ));
    assert!(!identity(
        0x400000,
        Some(&HEADER_CHECKSUM),
        Some(&DECLARATION),
        || [0; 32]
    ));
    let cart = Cartridge::new(fixture_rom()).unwrap();
    cart.validate_expansion_save().unwrap();
    assert!(!cart.save.as_ref().unwrap().banked_readable());
    let mut stripped = cart.clone();
    stripped.save = None;
    assert!(stripped.validate_expansion_save().is_err());
    for target in [START + 4, START + 65535] {
        for direction in [Direction::Read, Direction::Write] {
            let mut memory = SoundMemory::default();
            memory.set_reset(false);
            memory.bank = (target >> 15) as u16;
            let before = memory.clone();
            let address = (0x8000 | (target & 0x7fff)) as u16;
            {
                let mut bus = CandidateBus::new(&mut memory, &cart);
                if direction == Direction::Read {
                    bus.cpu_read(address);
                } else {
                    bus.cpu_write(address, 0x5a);
                }
                assert_eq!(
                    bus.fault.get(),
                    Some(SoundFault::UnsupportedAccess { address, direction })
                );
            }
            assert_eq!(memory, before);
        }
    }
    crate::Emulator::expansion_save_contract();
}
