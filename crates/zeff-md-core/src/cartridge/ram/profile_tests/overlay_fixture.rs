use super::*;
use crate::cartridge::Cartridge;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Profile {
    Expansion,
    Bytes,
    Odd,
    Switched,
    Reserved,
    Split,
}

pub(crate) const ALL: [Profile; 6] = [
    Profile::Expansion,
    Profile::Bytes,
    Profile::Odd,
    Profile::Switched,
    Profile::Reserved,
    Profile::Split,
];

impl Profile {
    pub(crate) fn size(self) -> usize {
        match self {
            Self::Bytes => 0x340000,
            Self::Odd => 0x100000,
            Self::Switched => 0x280000,
            _ => 0x400000,
        }
    }

    pub(crate) fn factory_fixture(self) -> bool {
        matches!(self, Self::Expansion | Self::Split)
    }
}

fn words(bytes: &mut [u8], at: usize, program: &[u16]) {
    for (index, word) in program.iter().enumerate() {
        bytes[at + index * 2..at + index * 2 + 2].copy_from_slice(&word.to_be_bytes());
    }
}

fn image(profile: Profile, program: &[u16]) -> Vec<u8> {
    let mut bytes = vec![0; profile.size()];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    if profile == Profile::Split {
        bytes[0x180..0x18e].copy_from_slice(b"GM T-172116-00");
        bytes[0x1b0..0x1bc].copy_from_slice(b"RA\xf8\x20\x00\x20\x00\x01\x00\x20\xff\xff");
    }
    words(&mut bytes, 0x200, program);
    words(&mut bytes, 0x10100, &[0x7001, 0x0640, 3, 0x4e75, 0x4e71]);
    words(&mut bytes, 0x18000, &[0x1234, 0x5678]);
    words(&mut bytes, profile.size() - 2, &[0x9abc]);
    bytes
}

fn finish(profile: Profile, bytes: Vec<u8>) -> Cartridge {
    let mut cart = Cartridge::new(bytes).unwrap();
    if profile != Profile::Split {
        assert!(cart.save.is_none());
        cart.save = Some(match profile {
            Profile::Expansion => SaveMemory::new_expansion_bytes(),
            Profile::Bytes => SaveMemory::bounded_bytes_for_test(),
            Profile::Odd => SaveMemory::bounded_odd_for_test(),
            Profile::Switched => SaveMemory::bounded_switched_for_test(),
            Profile::Reserved => SaveMemory::reserved_switched_for_test(),
            Profile::Split => unreachable!(),
        });
    }
    cart.save.as_ref().unwrap().validate_state().unwrap();
    assert_eq!(cart.supports_rom_overlay(), profile == Profile::Split);
    cart
}

pub(crate) fn cartridge(profile: Profile, program: &[u16]) -> Cartridge {
    if profile == Profile::Expansion {
        return Cartridge::new(crate::cartridge::ram::expansion_save_fixture()).unwrap();
    }
    finish(profile, image(profile, program))
}

pub(crate) fn matrix_cartridge(profile: Profile, source: usize) -> Cartridge {
    let program = [0x4e70, 0x4e71, 0x4e71, 0x4e71];
    let mut bytes = image(profile, &program);
    words(&mut bytes, source, &program);
    finish(profile, bytes)
}
