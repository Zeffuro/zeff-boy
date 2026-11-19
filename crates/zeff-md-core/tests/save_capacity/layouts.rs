use super::fixture::Rom;

impl Rom {
    pub fn write_long(&mut self, address: u32, value: u32) {
        self.word(0x23fc);
        self.long(value);
        self.long(address);
    }
}

pub fn mapped(mut bytes: Vec<u8>, base: u32) -> Vec<u8> {
    if base != 0x200000 {
        let end = u32::from_be_bytes(bytes[0x1b8..0x1bc].try_into().unwrap()) - 0x200000;
        bytes.resize(base as usize, 0);
        bytes[0x1b4..0x1b8].copy_from_slice(&(base + 1).to_be_bytes());
        bytes[0x1b8..0x1bc].copy_from_slice(&(base + end).to_be_bytes());
        bytes[base as usize - 1] = 0x69;
    }
    bytes
}

pub fn header_profile(mut bytes: Vec<u8>, profile: usize) -> Vec<u8> {
    let (size, product, declaration) = [
        (
            0x10_0000,
            b"GM    55508-00",
            b"RA\xf8\x20\x00\x20\x00\x00\x00\x20\x3f\xff",
        ),
        (
            0xa_0000,
            b"GM 00005502-00",
            b"RA\xf8\x20\x00\x20\x00\x00\x00\x20\x3f\xff",
        ),
        (
            0x10_0000,
            b"GM T-106113-00",
            b"RA\xe0\x20\x00\x20\x00\x01\x00\x20\x3f\xff",
        ),
    ][profile];
    bytes.resize(size, 0);
    bytes[0x180..0x18e].copy_from_slice(product);
    bytes[0x1b0..0x1bc].copy_from_slice(declaration);
    bytes
}
