pub fn half(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
pub fn integer(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
pub fn pointer(bytes: &mut [u8], at: usize, value: usize) {
    integer(bytes, at, 0x0800_0000 + value as u32);
}

pub fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; 0x1200];
    let version = b"GAX Sound Engine v3.05A (Aug 13 2003) \xa9 Shin'en\0";
    bytes[0x40..0x40 + version.len()].copy_from_slice(version);
    pointer(&mut bytes, 0x204, 0x300);
    pointer(&mut bytes, 0x230, 0x1000);
    integer(&mut bytes, 0x234, 16);
    bytes[0x301] = 2;
    pointer(&mut bytes, 0x30C, 0x400);
    bytes[0x310] = 1;
    bytes[0x311] = 1;
    pointer(&mut bytes, 0x314, 0x480);
    half(&mut bytes, 0x318, 32);
    integer(&mut bytes, 0x320, 2);
    integer(&mut bytes, 0x324, 16);
    bytes[0x400..0x404].copy_from_slice(&[2, 0, 255, 255]);
    bytes[0x408] = 255;
    half(&mut bytes, 0x40C, 8);
    half(&mut bytes, 0x40E, (-8160i16) as u16);
    bytes[0x480..0x488].copy_from_slice(&[2, 0, 1, 0, 0, 0, 0, 0]);
    let pattern = [0, 0xB1, 1, 0xFA, 12, 128, 0xB5, 1, 0x81, 0];
    bytes[0x500..0x50A].copy_from_slice(&pattern);
    let title = b"\"Fixture\" \xa9 Tests";
    bytes[0x50A..0x50A + title.len()].copy_from_slice(title);
    let table = (0x50A + title.len() + 3) & !3;
    half(&mut bytes, 0x800, 2);
    half(&mut bytes, 0x802, 4);
    half(&mut bytes, 0x804, 2);
    half(&mut bytes, 0x806, 1);
    half(&mut bytes, 0x808, 256);
    pointer(&mut bytes, 0x80C, 0x500);
    pointer(&mut bytes, 0x810, 0x200);
    pointer(&mut bytes, 0x814, 0x220);
    half(&mut bytes, 0x818, 15769);
    pointer(&mut bytes, 0x820, table);
    pointer(&mut bytes, 0x824, table + 8);
    bytes[table + 10] = 12;
    bytes[table + 14] = 12;
    bytes[0x1000..0x1010].copy_from_slice(&[
        0, 16, 32, 64, 96, 112, 128, 144, 160, 176, 192, 208, 224, 240, 255, 128,
    ]);
    bytes
}
