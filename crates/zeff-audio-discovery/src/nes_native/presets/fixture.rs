use super::{CNROM_HEADER, NROM_HEADER, Profile, SOURCE_LEN};

pub(super) const PROFILE_NROM: &str = "nes-native-register-presets-fixture-nrom";
pub(super) const PROFILE_CNROM: &str = "nes-native-register-presets-fixture-cnrom";

pub fn fixture_rom() -> Vec<u8> {
    fixture(false)
}

pub fn fixture_rom_cnrom() -> Vec<u8> {
    fixture(true)
}

pub(super) fn source_hash(cnrom: bool) -> String {
    zeff_firmware::sha256_hex(&fixture(cnrom))
}

pub(super) fn profile(bytes: &[u8], hash: &str) -> Option<Profile> {
    let cnrom = bytes.get(..16) == Some(CNROM_HEADER);
    let nrom = bytes.get(..16) == Some(NROM_HEADER);
    if !(cnrom || nrom) || hash != source_hash(cnrom) {
        return None;
    }
    Some(Profile {
        id: if cnrom { PROFILE_CNROM } else { PROFILE_NROM },
        mapper: if cnrom { 3 } else { 0 },
        header: if cnrom { CNROM_HEADER } else { NROM_HEADER },
        hash: if cnrom {
            "fixture-cnrom"
        } else {
            "fixture-nrom"
        },
    })
}

fn fixture(cnrom: bool) -> Vec<u8> {
    let mut bytes = vec![0; SOURCE_LEN];
    bytes[..16].copy_from_slice(if cnrom { CNROM_HEADER } else { NROM_HEADER });
    put(
        &mut bytes,
        0xe9cb,
        &[
            0xa9, 0x0e, 0x8d, 0x15, 0x40, 0xa9, 0x0e, 0x85, 0xb0, 0xa9, 0, 0x85, 0xbc, 0x60,
        ],
    );
    put(
        &mut bytes,
        0xe9ff,
        &[
            0xa5, 0xad, 0xc9, 0xff, 0xf0, 0x3e, 0x0a, 0x0a, 0xa8, 0xc9, 0x20, 0xf0, 0x20, 0xa9,
            0x0f, 0x8d, 0x15, 0x40, 0xb9, 0x56, 0xd9, 0x8d, 0, 0x40, 0xb9, 0x57, 0xd9, 0x8d, 1,
            0x40, 0xb9, 0x58, 0xd9, 0x8d, 2, 0x40, 0xb9, 0x59, 0xd9, 0x8d, 3, 0x40, 0x4c, 0x43,
            0xea, 0xa9, 0x0e, 0x8d, 0x15, 0x40, 0xb9, 0x56, 0xd9, 0x8d, 0x0c, 0x40, 0xb9, 0x58,
            0xd9, 0x8d, 0x0e, 0x40, 0xb9, 0x59, 0xd9, 0x8d, 0x0f, 0x40, 0xa9, 0xff, 0x85, 0xad,
            0x60,
        ],
    );
    put(&mut bytes, 0xd956, &[0x9f, 0, 0x30, 9]);
    put(&mut bytes, 0xd95a, &[0x9f, 0, 0x40, 9]);
    put(&mut bytes, 0xd95e, &[0x9f, 0, 0x50, 9]);
    put(&mut bytes, 0xd962, &[0x9f, 0, 0x60, 9]);
    put(&mut bytes, 0xd96a, &[0x9f, 0, 0x80, 9]);
    put(&mut bytes, 0xd96e, &[0x9f, 0, 0x90, 9]);
    put(&mut bytes, 0xd976, &[0x1f, 0, 5, 8]);
    put(&mut bytes, 0xf800, &[0x4c, 0, 0xf8]);
    put(&mut bytes, 0xfffa, &[0, 0xf8, 0, 0xf8, 0, 0xf8]);
    bytes
}

fn put(bytes: &mut [u8], address: u16, code: &[u8]) {
    let offset = usize::from(address - 0x8000) + 16;
    bytes[offset..offset + code.len()].copy_from_slice(code);
}
