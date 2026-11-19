use super::Cartridge;

pub(crate) fn cartridge(profile: usize, words: &[u16]) -> Cartridge {
    let mut bytes = match profile {
        0..=4 => super::protection::tests::rom(profile, words),
        5..=8 => super::registers::tests::rom(profile - 5),
        9 => super::registers::bank_tests::fixture().bytes.to_vec(),
        _ => panic!("unknown finite owner"),
    };
    let expected = crate::CartridgeHeader::parse(&bytes)
        .unwrap()
        .calculated_checksum;
    for (index, word) in words.iter().enumerate() {
        bytes[0x200 + 2 * index..0x202 + 2 * index].copy_from_slice(&word.to_be_bytes());
    }
    if profile == 9 {
        let program = bytes[0x200..0x300].to_vec();
        bytes[0x18200..0x18300].copy_from_slice(&program);
        let callee = bytes[0x10100..0x10108].to_vec();
        bytes[0x18100..0x18108].copy_from_slice(&callee);
        bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    } else {
        let tail = bytes.len() - 2;
        bytes[tail..].fill(0);
        let sum = bytes[0x200..]
            .as_chunks::<2>()
            .0
            .iter()
            .fold(0_u16, |sum, pair| {
                sum.wrapping_add(u16::from_be_bytes(*pair))
            });
        bytes[tail..].copy_from_slice(&expected.wrapping_sub(sum).to_be_bytes());
    }
    let mut cart = Cartridge::new(bytes).unwrap();
    if profile == 9 {
        assert!(cart.registers.is_none());
        cart.registers = super::registers::bank_tests::fixture().registers;
    } else if profile < 5 {
        assert!(cart.protection.is_some());
    } else {
        assert!(cart.registers.is_some());
    }
    assert!(cart.save.is_none());
    cart
}
