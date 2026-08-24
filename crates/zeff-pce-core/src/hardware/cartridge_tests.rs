use super::cpu::Cpu;
use super::{
    BaseBus, HUCARD_ROM_REGION_LEN, POPULOUS_CANONICAL_SHA256, POPULOUS_HUCARD_IMAGE_LEN,
    POPULOUS_HUCARD_RAM_LEN, PceCartridgeDescriptor, PceHuCardBoard, SF2_CE_CANONICAL_SHA256,
    SF2_CE_HUCARD_IMAGE_LEN,
};

fn sf2_image() -> Vec<u8> {
    let mut image = vec![0x10; SF2_CE_HUCARD_IMAGE_LEN];
    for bank in 0..4 {
        image[0x08_0000 + bank * 0x08_0000..0x10_0000 + bank * 0x08_0000].fill(0x20 + bank as u8);
    }
    image[0x001FF0..=0x001FF3].copy_from_slice(&[0xA0, 0xA1, 0xA2, 0xA3]);
    image
}

#[test]
fn canonical_hashes_and_structural_sf2_select_the_bounded_boards() {
    assert_eq!(
        PceCartridgeDescriptor::from_sha256(SF2_CE_CANONICAL_SHA256)
            .hucard_board(SF2_CE_HUCARD_IMAGE_LEN),
        PceHuCardBoard::Sf2Ce
    );
    assert_eq!(
        PceCartridgeDescriptor::from_sha256(POPULOUS_CANONICAL_SHA256)
            .hucard_board(POPULOUS_HUCARD_IMAGE_LEN),
        PceHuCardBoard::Populous
    );
    assert_eq!(
        PceCartridgeDescriptor::from_sha256([0; 32]).hucard_board(SF2_CE_HUCARD_IMAGE_LEN),
        PceHuCardBoard::Sf2Ce
    );
    assert_eq!(
        PceCartridgeDescriptor::from_sha256(SF2_CE_CANONICAL_SHA256)
            .with_hucard_board(PceHuCardBoard::Plain)
            .hucard_board(SF2_CE_HUCARD_IMAGE_LEN),
        PceHuCardBoard::Plain
    );
}

#[test]
fn board_image_sizes_are_exact_and_plain_keeps_its_existing_limit() {
    for (board, len) in [
        (PceHuCardBoard::Plain, HUCARD_ROM_REGION_LEN + 1),
        (PceHuCardBoard::Sf2Ce, SF2_CE_HUCARD_IMAGE_LEN - 1),
        (PceHuCardBoard::Sf2Ce, SF2_CE_HUCARD_IMAGE_LEN + 1),
        (PceHuCardBoard::Populous, POPULOUS_HUCARD_IMAGE_LEN - 1),
        (PceHuCardBoard::Populous, POPULOUS_HUCARD_IMAGE_LEN + 1),
    ] {
        let error = BaseBus::with_hucard(vec![0; len], board, ()).unwrap_err();
        assert_eq!(error.rom_len(), len);
        assert_eq!(error.board(), board);
    }
}

#[test]
fn sf2_fixed_and_banked_windows_use_only_the_four_exact_selectors() {
    let image = sf2_image();
    let mut bus = BaseBus::with_hucard(image.clone(), PceHuCardBoard::Sf2Ce, ()).unwrap();
    assert_eq!(bus.hucard_rom(), image);
    assert_eq!(bus.read(0), 0x10);
    assert_eq!(bus.read(0x07_FFFF), 0x10);
    assert_eq!(bus.read(0x08_0000), 0x20);
    assert_eq!(bus.read(0x0F_FFFF), 0x20);
    assert_eq!(bus.hucard_rom_offset(0x08_0000), Some(0x08_0000));
    assert_eq!(bus.hucard_mapping_token(), 0);

    for bank in 0..4_u32 {
        bus.write(0x001FF0 + bank, 0xFF - bank as u8);
        assert_eq!(bus.read(0x08_0000), 0x20 + bank as u8);
        assert_eq!(bus.read(0x0F_FFFF), 0x20 + bank as u8);
        assert_eq!(bus.read(0x001FF0 + bank), 0xA0 + bank as u8);
        assert_eq!(
            bus.hucard_rom_offset(0x08_0000),
            Some(0x08_0000 + bank * 0x08_0000)
        );
        assert_eq!(bus.hucard_mapping_token(), bank as u8);
    }

    bus.write(0x001FF2, 0);
    bus.write(0x001FEF, 0);
    assert_eq!(bus.read(0x08_0000), 0x22);
    bus.write(0x001FF4, 0);
    assert_eq!(bus.read(0x08_0000), 0x22);
    bus.reset_hucard();
    assert_eq!(bus.read(0x08_0000), 0x20);
}

#[test]
fn cpu_mpr_write_selects_an_sf2_page_before_the_following_read() {
    let mut image = sf2_image();
    image[..8].copy_from_slice(&[0xA9, 0x77, 0x8D, 0xF2, 0x1F, 0xAD, 0x00, 0x40]);
    let mut bus = BaseBus::with_hucard(image, PceHuCardBoard::Sf2Ce, ()).unwrap();
    let mut cpu = Cpu::new();
    cpu.set_mapping_register(2, 0x40);

    cpu.step(&mut bus).unwrap();
    cpu.step(&mut bus).unwrap();
    cpu.step(&mut bus).unwrap();

    assert_eq!(cpu.registers().a, 0x22);
}

#[test]
fn populous_ram_is_exactly_bounded_zeroed_and_preserved_by_reset() {
    let image = vec![0xCC; POPULOUS_HUCARD_IMAGE_LEN];
    let mut bus = BaseBus::with_hucard(image.clone(), PceHuCardBoard::Populous, ()).unwrap();
    assert_eq!(bus.hucard_rom(), image);
    assert_eq!(bus.hucard_ram(), Some(&[0; POPULOUS_HUCARD_RAM_LEN]));
    assert_eq!(bus.read(0x07_FFFF), 0xCC);
    assert_eq!(bus.read(0x08_0000), 0);
    assert_eq!(bus.read(0x08_7FFF), 0);
    assert_eq!(bus.read(0x08_8000), 0xFF);
    assert_eq!(bus.read(0x0F_FFFF), 0xFF);
    assert_eq!(bus.hucard_rom_offset(0x07_FFFF), Some(0x07_FFFF));
    assert_eq!(bus.hucard_rom_offset(0x08_0000), None);

    bus.write(0x07_FFFF, 0x11);
    bus.write(0x08_0000, 0x22);
    bus.write(0x08_7FFF, 0x33);
    bus.write(0x08_8000, 0x44);
    assert_eq!(bus.read(0x07_FFFF), 0xCC);
    assert_eq!(bus.read(0x08_0000), 0x22);
    assert_eq!(bus.read(0x08_7FFF), 0x33);
    assert_eq!(bus.read(0x08_8000), 0xFF);
    bus.reset_hucard();
    assert_eq!(bus.read(0x08_0000), 0x22);
    assert_eq!(bus.read(0x08_7FFF), 0x33);

    let fresh = BaseBus::with_hucard(
        vec![0; POPULOUS_HUCARD_IMAGE_LEN],
        PceHuCardBoard::Populous,
        (),
    )
    .unwrap();
    assert_eq!(fresh.hucard_ram(), Some(&[0; POPULOUS_HUCARD_RAM_LEN]));
}

#[test]
fn system_card_rom_offsets_follow_the_physical_mirror() {
    let bus = BaseBus::with_hucard(
        vec![0; super::SYSTEM_CARD_V1_V2_IMAGE_LEN],
        PceHuCardBoard::SystemCardV3,
        (),
    )
    .unwrap();

    assert_eq!(bus.hucard_rom_offset(0x03_FFFF), Some(0x03_FFFF));
    assert_eq!(bus.hucard_rom_offset(0x04_0000), Some(0));
    assert_eq!(bus.hucard_rom_offset(0x07_FFFF), Some(0x03_FFFF));
    assert_eq!(bus.hucard_rom_offset(0x0D_0000), None);
}
