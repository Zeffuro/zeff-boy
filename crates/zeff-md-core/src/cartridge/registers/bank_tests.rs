use super::*;
use crate::cartridge::Cartridge;
use zeff_emu_common::save_state::{StateReader, StateWriter};

pub(crate) fn fixture() -> Cartridge {
    let mut bytes = vec![0; 0x100000];
    for (page, chunk) in bytes.chunks_mut(0x8000).enumerate() {
        chunk.fill(0x40 + page as u8);
    }
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xf0, 0, 0, 2, 0]);
    bytes[0x180..0x18e].fill(0);
    let mut cart = Cartridge::new(bytes).unwrap();
    cart.registers = Some(CartridgeRegisters {
        layout: RegisterLayout::OrBank32KiB,
        values: [0; 3],
    });
    cart
}

pub(crate) fn assert_identity_mapping_and_codec() {
    let cart = fixture();
    let mut header = crate::CartridgeHeader::parse(&cart.bytes).unwrap();
    header.checksum = 0;
    header.calculated_checksum = 0x6ff8;
    let expected = [
        0x49, 0x78, 0x5e, 0xaa, 0x4f, 0x8d, 0x31, 0x83, 0xed, 0x56, 0xb1, 0xaa, 0xa3, 0xbc, 0x4b,
        0xa8, 0xd7, 0xd7, 0x3c, 0xfb, 0x44, 0xa4, 0xf1, 0xfd, 0x79, 0xc0, 0x29, 0x84, 0xf4, 0x90,
        0x56, 0x66,
    ];
    assert!(
        CartridgeRegisters::detect_rom(&cart.bytes, Some(&header), || expected)
            .unwrap()
            .is_banked()
    );
    for index in 0..32 {
        let mut wrong = expected;
        wrong[index] ^= 1;
        assert!(CartridgeRegisters::detect_rom(&cart.bytes, Some(&header), || wrong).is_none());
    }
    for change in 0..4 {
        let mut bytes = cart.bytes.to_vec();
        let mut header = header.clone();
        match change {
            0 => bytes.push(0),
            1 => bytes[0x180] = 1,
            2 => header.checksum = 1,
            _ => header.calculated_checksum ^= 1,
        }
        assert!(
            CartridgeRegisters::detect_rom(&bytes, Some(&header), || panic!(
                "invalid metadata hashed"
            ))
            .is_none()
        );
    }
    let mut board = cart.registers.unwrap();
    for bank in [0, 1, 2, 3, 0x10, 0x1f, 0x20, 0x3f, 0x80, 0xff] {
        board.write_bank(bank);
        for page in 0..32 {
            let address = page * 0x8000 + 0x1234;
            assert_eq!(
                board.source_offset(address),
                Some((address | (u32::from(bank & 0x3f) << 15)) as usize)
            );
        }
        for address in [0x100000, 0x200000, 0x3fffff, 0xbfffff] {
            assert_eq!(board.source_offset(address), None);
        }
        let mut writer = StateWriter::new();
        board.write_state(&mut writer);
        let encoded = writer.into_bytes();
        assert_eq!(encoded, [2, 0, 0, bank]);
        let mut copy = CartridgeRegisters {
            layout: RegisterLayout::OrBank32KiB,
            values: [0; 3],
        };
        copy.read_state(&mut StateReader::new(&encoded)).unwrap();
        assert_eq!(copy, board);
    }
    board.reset_bank();
    assert_eq!(board.values, [0; 3]);
    let mut old = CartridgeRegisters::default();
    old.write(0, 0x12);
    old.write(1, 0x34);
    let mut writer = StateWriter::new();
    old.write_state(&mut writer);
    assert_eq!(writer.into_bytes(), [1, 0x12, 0x34]);
    for bytes in [
        &[1, 0x12, 0x34][..],
        &[2, 1, 0, 0][..],
        &[2, 0, 1, 0][..],
        &[2, 0, 0][..],
    ] {
        let before = board.clone();
        assert!(board.read_state(&mut StateReader::new(bytes)).is_err());
        assert_eq!(board, before);
    }
    let before = old.clone();
    assert!(
        old.read_state(&mut StateReader::new(&[2, 0, 0, 1]))
            .is_err()
    );
    assert_eq!(old, before);
    assert!(old.validate_profile(&board).is_err());
    assert!(board.validate_profile(&old).is_err());
    old.values[2] = 1;
    assert!(
        old.validate_profile(&CartridgeRegisters::default())
            .is_err()
    );
}
