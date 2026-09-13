pub fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

pub fn fixture(gd3: bool) -> Vec<u8> {
    let mut bytes = vec![0; 0x43];
    bytes[..4].copy_from_slice(b"Vgm ");
    put32(&mut bytes, 8, 0x171);
    put32(&mut bytes, 0x34, 0x0c);
    put32(&mut bytes, 0x18, 1617);
    put32(&mut bytes, 0x1c, 0x24);
    put32(&mut bytes, 0x20, 1617);
    bytes[0x40..].copy_from_slice(&[0x62, 0x63, 0x66]);
    if gd3 {
        let at = bytes.len();
        let mut fields = Vec::new();
        for text in ["Title", "", "", "", "", "", "", "", "", "", ""] {
            fields.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
            fields.extend_from_slice(&0u16.to_le_bytes());
        }
        bytes.extend_from_slice(b"Gd3 ");
        bytes.extend_from_slice(&0x100u32.to_le_bytes());
        bytes.extend_from_slice(&(fields.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&fields);
        put32(&mut bytes, 0x14, (at - 0x14) as u32);
    }
    let eof = bytes.len() as u32 - 4;
    put32(&mut bytes, 4, eof);
    bytes
}
