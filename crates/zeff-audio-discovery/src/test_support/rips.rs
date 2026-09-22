use crate::rips::RipFormat;

pub fn fixture(format: RipFormat) -> Vec<u8> {
    if format == RipFormat::Nsfe {
        return nsfe_fixture();
    }
    let (header, at, text) = match format {
        RipFormat::Gbs => (0x70, 6, 0x10),
        RipFormat::Nsf => (0x80, 8, 0x0e),
        RipFormat::Nsfe => unreachable!("handled above"),
    };
    let mut bytes = vec![0; header + 0x50];
    match format {
        RipFormat::Gbs => {
            bytes[..6].copy_from_slice(b"GBS\x01\x03\x02");
            bytes[0x0c..0x0e].copy_from_slice(&0xfffeu16.to_le_bytes());
        }
        RipFormat::Nsf => {
            bytes[..8].copy_from_slice(b"NESM\x1a\x01\x03\x02");
            bytes[0x6e..0x70].copy_from_slice(&16639u16.to_le_bytes());
        }
        RipFormat::Nsfe => unreachable!("handled above"),
    }
    let load: u16 = if format == RipFormat::Gbs {
        0x400
    } else {
        0x8000
    };
    for (offset, address) in [(at, load), (at + 2, load), (at + 4, load + 1)] {
        bytes[offset..offset + 2].copy_from_slice(&address.to_le_bytes());
    }
    bytes[text..text + 11].copy_from_slice(b"Test source");
    bytes[header..].fill(if format == RipFormat::Gbs { 0xc9 } else { 0x60 });
    bytes
}

fn nsfe_fixture() -> Vec<u8> {
    let mut bytes = b"NSFE".to_vec();
    chunk(
        &mut bytes,
        b"INFO",
        &[0, 0x80, 0, 0x80, 1, 0x80, 0, 0, 3, 1],
    );
    chunk(&mut bytes, b"RATE", &[0xff, 0x40, 0x1d, 0x4e]);
    chunk(&mut bytes, b"BANK", &[]);
    chunk(&mut bytes, b"DATA", &[0x60; 0x50]);
    chunk(&mut bytes, b"tlbl", b"Test source\0");
    chunk(&mut bytes, b"NEND", &[]);
    bytes
}

fn chunk(bytes: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend(id);
    bytes.extend(payload);
}
