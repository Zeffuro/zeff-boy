use zeff_emu_common::save_state::{StateReader, StateWriter};

use super::{ARCADE_CARD_RAM_LEN, ArcadeCard};

const IO: u32 = 0x1F_FA00;

fn write_port(card: &mut ArcadeCard, port: u32, register: u32, value: u8) {
    assert!(card.write_physical(IO + port * 0x10 + register, value));
}

fn read_port(card: &mut ArcadeCard, port: u32, register: u32) -> u8 {
    card.read_physical(IO + port * 0x10 + register).unwrap()
}

#[test]
fn four_ports_address_independently_and_bank_windows_mirror_data() {
    let mut card = ArcadeCard::new();
    for port in 0..4 {
        write_port(&mut card, port, 2, (0x20 + port) as u8);
        write_port(&mut card, port, 3, 0x10);
        write_port(&mut card, port, 4, 0x01);
        assert!(card.write_physical(0x08_0000 + port * 0x2000 + 0x1555, 0xA0 + port as u8));
    }

    for port in 0..4 {
        assert_eq!(read_port(&mut card, port, 0), 0xA0 + port as u8);
        assert_eq!(
            card.peek_physical(IO + 0x40 + port * 0x10),
            Some(0xA0 + port as u8)
        );
    }
}

#[test]
fn offset_modes_triggers_and_increment_targets_wrap_at_their_native_widths() {
    let mut card = ArcadeCard::new();
    write_port(&mut card, 0, 2, 1);
    write_port(&mut card, 0, 5, 0xFF);
    write_port(&mut card, 0, 6, 0xFF);
    write_port(&mut card, 0, 7, 2);
    write_port(&mut card, 0, 9, 0x0B);
    card.ram_mut()[0] = 0x42;
    assert_eq!(read_port(&mut card, 0, 0), 0x42);
    assert_eq!(card.debug_snapshot().ports[0].offset, 1);

    write_port(&mut card, 0, 2, 0xFE);
    write_port(&mut card, 0, 3, 0xFF);
    write_port(&mut card, 0, 4, 0xFF);
    write_port(&mut card, 0, 7, 4);
    write_port(&mut card, 0, 9, 0x11);
    write_port(&mut card, 0, 0, 0x77);
    assert_eq!(card.debug_snapshot().ports[0].base, 2);
    assert_eq!(card.ram()[ARCADE_CARD_RAM_LEN - 2], 0x77);

    write_port(&mut card, 1, 2, 5);
    write_port(&mut card, 1, 9, 0x28);
    write_port(&mut card, 1, 6, 0xFF);
    write_port(&mut card, 1, 5, 0xFE);
    assert_eq!(card.debug_snapshot().ports[1].base, 3);
    write_port(&mut card, 1, 9, 0x68);
    write_port(&mut card, 1, 0x0A, 0);
    assert_eq!(card.debug_snapshot().ports[1].base, 1);

    write_port(&mut card, 3, 7, 1);
    write_port(&mut card, 3, 9, 0x05);
    write_port(&mut card, 3, 0, 0xCC);
    assert_eq!(card.debug_snapshot().ports[3].offset, 1);
    assert_eq!(read_port(&mut card, 3, 9), 0x05);
}

#[test]
fn shift_rotate_and_identification_registers_follow_arcade_card_protocol() {
    let mut card = ArcadeCard::new();
    for (register, value) in [0x78, 0x56, 0x34, 0x12].into_iter().enumerate() {
        assert!(card.write_physical(IO + 0xE0 + register as u32, value));
    }
    assert!(card.write_physical(IO + 0xE4, 0xA4));
    assert_eq!(card.debug_snapshot().value, 0x2345_6780);
    assert_eq!(card.read_physical(IO + 0xE4), Some(0xA4));
    assert!(card.write_physical(IO + 0xE5, 0xB8));
    assert_eq!(card.debug_snapshot().value, 0x8023_4567);
    assert_eq!(card.read_physical(IO + 0xFC), Some(0));
    assert_eq!(card.read_physical(IO + 0xFD), Some(0));
    assert_eq!(card.read_physical(IO + 0xFE), Some(0x10));
    assert_eq!(card.read_physical(IO + 0xFF), Some(0x51));
    assert_eq!(card.read_physical(IO + 0xEC), Some(0xFF));
    assert_eq!(card.read_physical(IO + 0xED), Some(0xFF));
}

#[test]
fn state_roundtrips_live_protocol_and_rejects_malformed_control_transactionally() {
    let mut saved = ArcadeCard::new();
    saved.ram_mut()[0x1F_FFFE] = 0x91;
    write_port(&mut saved, 2, 2, 0xFE);
    write_port(&mut saved, 2, 3, 0xFF);
    write_port(&mut saved, 2, 4, 0x1F);
    write_port(&mut saved, 2, 7, 3);
    write_port(&mut saved, 2, 9, 0x13);
    let mut writer = StateWriter::new();
    saved.write_state(&mut writer);
    let bytes = writer.into_bytes();

    let mut restored = ArcadeCard::new();
    restored.read_state(&mut StateReader::new(&bytes)).unwrap();
    assert_eq!(restored, saved);
    assert_eq!(read_port(&mut restored, 2, 0), 0x91);
    assert_eq!(read_port(&mut saved, 2, 0), 0x91);
    assert_eq!(restored, saved);

    let before = restored.clone();
    let mut malformed = bytes;
    malformed[ARCADE_CARD_RAM_LEN + 8] = 0x80;
    assert!(
        restored
            .read_state(&mut StateReader::new(&malformed))
            .is_err()
    );
    assert_eq!(restored, before);
}
