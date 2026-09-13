use crate::nes_music::NesSong;

fn put(bytes: &mut [u8], address: u16, data: &[u8]) {
    let offset = usize::from(address) - 0x8000 + 16;
    bytes[offset..offset + data.len()].copy_from_slice(data);
}

pub fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; 40_976];
    bytes[..8].copy_from_slice(b"NES\x1a\x02\x01\x01\x00");
    put(&mut bytes, 0xf90d, &[0x40; 49]);
    put(&mut bytes, 0xf94d, &[0, 0, 0x82, 0x40, 0x20, 0x60]);
    for index in 0..48 {
        put(&mut bytes, 0xff66 + index, &[(index % 8 + 2) as u8]);
    }
    put(&mut bytes, 0xff18, &[0, 254, 0, 226]);
    put(&mut bytes, 0x8200, &[0x80, 0x18, 0x81, 0x1a, 0]);
    put(&mut bytes, 0x8220, &[0x18, 0x9a]);
    put(&mut bytes, 0x8240, &[0x80, 0x18, 0x81, 0x1a, 0]);
    put(&mut bytes, 0x8260, &[0x10, 0]);
    bytes
}

pub fn synthetic_song(bytes: &[u8], index: u8) -> NesSong {
    crate::nes_music::synthetic_song_for_test_support(bytes, index)
}
