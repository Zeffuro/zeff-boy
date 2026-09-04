use super::*;

fn eeprom_cartridge() -> Cartridge {
    let mut rom = vec![0; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xAC..0xB0].copy_from_slice(b"ABCD");
    rom[0xB2] = 0x96;
    rom.extend_from_slice(b"EEPROM_V122");
    Cartridge::load(&rom).unwrap()
}

#[test]
fn execution_state_roundtrips_complete_eeprom_protocol() {
    let mut source = eeprom_cartridge();
    *source.eeprom.get_mut() = EepromState {
        command_bits: vec![1, 0, 1, 1],
        read_bits: vec![0, 1, 1, 0, 1],
        read_index: 3,
        busy_cycles_remaining: 71_000,
    };
    let mut writer = StateWriter::new();
    source.write_backup_execution_state(&mut writer);
    let bytes = writer.into_bytes();
    assert_eq!(bytes.len(), BACKUP_EXECUTION_STATE_SIZE);

    let mut target = eeprom_cartridge();
    let mut reader = StateReader::new(&bytes);
    target.read_backup_execution_state(&mut reader).unwrap();

    let restored = target.eeprom.into_inner();
    assert_eq!(restored.command_bits, vec![1, 0, 1, 1]);
    assert_eq!(restored.read_bits, vec![0, 1, 1, 0, 1]);
    assert_eq!(restored.read_index, 3);
    assert_eq!(restored.busy_cycles_remaining, 71_000);
    assert!(reader.is_exhausted());
}

#[test]
fn execution_state_rejects_wrong_kind_and_noncanonical_padding() {
    let source = eeprom_cartridge();
    let mut writer = StateWriter::new();
    source.write_backup_execution_state(&mut writer);
    let bytes = writer.into_bytes();

    let mut wrong_kind = bytes.clone();
    wrong_kind[0] = backup_kind_tag(BackupKind::Flash1M);
    assert!(
        eeprom_cartridge()
            .read_backup_execution_state(&mut StateReader::new(&wrong_kind))
            .is_err()
    );

    let mut padded = bytes;
    padded[5] = 1;
    assert!(
        eeprom_cartridge()
            .read_backup_execution_state(&mut StateReader::new(&padded))
            .is_err()
    );
}
