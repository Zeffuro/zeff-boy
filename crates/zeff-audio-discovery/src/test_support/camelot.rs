use super::{fixture, put_word};

pub fn synth_fixture(kind: u8) -> Vec<u8> {
    let mut bytes = fixture();
    bytes.resize(0x3000, 0xFF);
    for (offset, signature) in [
        (0x1800, "0220b0e1400080030100c4051c4084e2"),
        (
            0x1900,
            "0260d3e5062c82e00460d3e5066c92e00660e04126a4a0e10310d3e50100d3e5000ca0e19a0126e0",
        ),
        (
            0x1A00,
            "01c05ce22000001a036ca0e3abb0a0e1ffbccbe370c0a0e3034495e8847197e0279c6ce08760a0e1a69d49e0c22099e09b022010847197e0279c6ce08760a0e1a69d49e0c22099e09b122110847197e0279c6ce08760a0e1a69d49e0c22099e09ba22a10847197e0279c6ce08760a0e1a69d49e0c22099e09be22e100344a5e8048058e2e3ffffca",
        ),
        (
            0x1B00,
            "8060a0e306cda0e3034495e8847197e0c79b6650a79b4c409b0920e0847197e0c79b6650a79b4c409b1921e0847197e0c79b6650a79b4c409ba92ae0847197e0c79b6650a79b4c409be92ee00344a5e8048058e2ebffffca",
        ),
    ] {
        let signature = const_hex::decode(signature).expect("valid Camelot fixture signature");
        bytes[offset..offset + signature.len()].copy_from_slice(&signature);
    }
    put_word(&mut bytes, 0x300, 0x4000_0000);
    put_word(&mut bytes, 0x304, 17_140_000);
    put_word(&mut bytes, 0x308, 0);
    put_word(&mut bytes, 0x30C, 0);
    bytes[0x310..0x316].copy_from_slice(&[0x80, kind, 0x10, 0xF0, 0xE0, 0x80]);
    bytes
}
