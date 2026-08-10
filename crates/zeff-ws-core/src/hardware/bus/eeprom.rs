use crate::hardware::cartridge::SaveKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EepromCommand {
    Read { address: usize },
    Write { address: usize },
    Erase { address: usize },
    WriteDisable,
    WriteAll,
    EraseAll,
    WriteEnable,
    Invalid,
}

pub(super) fn decode_eeprom_command(save_kind: SaveKind, command: u16) -> EepromCommand {
    let Some(address_bits) = eeprom_address_bits(save_kind) else {
        return EepromCommand::Invalid;
    };
    let address_mask = (1usize << address_bits) - 1;
    if address_bits <= 6 {
        if command & 0xFF00 != 0x0100 {
            return EepromCommand::Invalid;
        }
        let op = ((command >> 6) & 0x03) as u8;
        if op != 0 {
            return decode_eeprom_address_command(op, usize::from(command & 0x003F));
        }
        return decode_eeprom_short_command(((command >> 4) & 0x03) as u8);
    }

    if command & 0xF000 != 0x1000 {
        return EepromCommand::Invalid;
    }
    let op = ((command >> 10) & 0x03) as u8;
    if op != 0 {
        return decode_eeprom_address_command(op, usize::from(command) & address_mask);
    }
    decode_eeprom_short_command(((command >> 8) & 0x03) as u8)
}

fn eeprom_address_bits(save_kind: SaveKind) -> Option<usize> {
    match save_kind {
        SaveKind::Eeprom128 => Some(6),
        SaveKind::Eeprom1K => Some(9),
        SaveKind::Eeprom2K => Some(10),
        _ => None,
    }
}

fn decode_eeprom_address_command(op: u8, address: usize) -> EepromCommand {
    match op {
        0x01 => EepromCommand::Write { address },
        0x02 => EepromCommand::Read { address },
        0x03 => EepromCommand::Erase { address },
        _ => EepromCommand::Invalid,
    }
}

fn decode_eeprom_short_command(sub_op: u8) -> EepromCommand {
    match sub_op {
        0x00 => EepromCommand::WriteDisable,
        0x01 => EepromCommand::WriteAll,
        0x02 => EepromCommand::EraseAll,
        0x03 => EepromCommand::WriteEnable,
        _ => EepromCommand::Invalid,
    }
}
