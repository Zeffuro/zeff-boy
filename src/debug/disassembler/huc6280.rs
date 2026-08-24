use super::{DisassembledLine, InstructionBytes, Mnemonic};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AddressMode {
    Implied,
    Accumulator,
    Immediate,
    DirectPage,
    DirectPageX,
    DirectPageY,
    Absolute,
    AbsoluteX,
    AbsoluteY,
    IndexedIndirect,
    Indirect,
    IndirectIndexed,
    AbsoluteIndirect,
    AbsoluteIndexedIndirect,
    Relative,
    DirectPageRelative,
    TestDirectPage,
    TestDirectPageX,
    TestAbsolute,
    TestAbsoluteX,
    BlockTransfer,
    Break,
}

pub(super) fn instruction_len(bus_read: &impl Fn(u16) -> u8, addr: u16) -> usize {
    opcode_info(bus_read(addr)).map_or(1, |(_, mode)| mode_len(mode))
}

const fn mode_len(mode: AddressMode) -> usize {
    match mode {
        AddressMode::Implied | AddressMode::Accumulator => 1,
        AddressMode::Immediate
        | AddressMode::DirectPage
        | AddressMode::DirectPageX
        | AddressMode::DirectPageY
        | AddressMode::IndexedIndirect
        | AddressMode::Indirect
        | AddressMode::IndirectIndexed
        | AddressMode::Relative
        | AddressMode::Break => 2,
        AddressMode::Absolute
        | AddressMode::AbsoluteX
        | AddressMode::AbsoluteY
        | AddressMode::AbsoluteIndirect
        | AddressMode::AbsoluteIndexedIndirect
        | AddressMode::DirectPageRelative
        | AddressMode::TestDirectPage
        | AddressMode::TestDirectPageX => 3,
        AddressMode::TestAbsolute | AddressMode::TestAbsoluteX => 4,
        AddressMode::BlockTransfer => 7,
    }
}

pub(super) fn decode_instruction(bus_read: &impl Fn(u16) -> u8, addr: u16) -> DisassembledLine {
    let opcode = bus_read(addr);
    let Some((name, mode)) = opcode_info(opcode) else {
        return line(
            addr,
            [opcode].into_iter().collect(),
            mn!("DB ${opcode:02X}"),
            None,
        );
    };
    let bytes: InstructionBytes = (0..mode_len(mode))
        .map(|offset| bus_read(addr.wrapping_add(offset as u16)))
        .collect();
    let (operand, mut target) = render_operand(addr, mode, &bytes);
    if matches!(opcode, 0x20 | 0x4C) {
        target = Some(u16::from_le_bytes([bytes[1], bytes[2]]));
    }
    let mnemonic = if operand.is_empty() {
        mn!("{name}")
    } else {
        mn!("{name} {operand}")
    };
    line(addr, bytes, mnemonic, target)
}

fn line(
    address: u16,
    bytes: InstructionBytes,
    mnemonic: Mnemonic,
    control_target: Option<u16>,
) -> DisassembledLine {
    DisassembledLine {
        address: address.into(),
        storage_offset: None,
        bank: None,
        symbol: None,
        control_target: control_target.map(Into::into),
        control_target_storage: None,
        control_target_bank: None,
        control_target_symbol: None,
        source: None,
        bytes,
        mnemonic,
    }
}

fn render_operand(
    addr: u16,
    mode: AddressMode,
    bytes: &InstructionBytes,
) -> (Mnemonic, Option<u16>) {
    let byte = || bytes[1];
    let word = || u16::from_le_bytes([bytes[1], bytes[2]]);
    let relative_target = |instruction_len: u16, offset: u8| {
        addr.wrapping_add(instruction_len)
            .wrapping_add_signed(i16::from(offset as i8))
    };

    match mode {
        AddressMode::Implied | AddressMode::Break => (Mnemonic::new(), None),
        AddressMode::Accumulator => (mn!("A"), None),
        AddressMode::Immediate => (mn!("#${:02X}", byte()), None),
        AddressMode::DirectPage => (mn!("${:02X}", byte()), None),
        AddressMode::DirectPageX => (mn!("${:02X},X", byte()), None),
        AddressMode::DirectPageY => (mn!("${:02X},Y", byte()), None),
        AddressMode::Absolute => (mn!("${:04X}", word()), None),
        AddressMode::AbsoluteX => (mn!("${:04X},X", word()), None),
        AddressMode::AbsoluteY => (mn!("${:04X},Y", word()), None),
        AddressMode::IndexedIndirect => (mn!("(${:02X},X)", byte()), None),
        AddressMode::Indirect => (mn!("(${:02X})", byte()), None),
        AddressMode::IndirectIndexed => (mn!("(${:02X}),Y", byte()), None),
        AddressMode::AbsoluteIndirect => (mn!("(${:04X})", word()), None),
        AddressMode::AbsoluteIndexedIndirect => (mn!("(${:04X},X)", word()), None),
        AddressMode::Relative => {
            let target = relative_target(2, byte());
            (mn!("${target:04X}"), Some(target))
        }
        AddressMode::DirectPageRelative => {
            let target = relative_target(3, bytes[2]);
            (mn!("${:02X},${target:04X}", byte()), Some(target))
        }
        AddressMode::TestDirectPage => (mn!("#${:02X},${:02X}", bytes[1], bytes[2]), None),
        AddressMode::TestDirectPageX => (mn!("#${:02X},${:02X},X", bytes[1], bytes[2]), None),
        AddressMode::TestAbsolute => {
            let address = u16::from_le_bytes([bytes[2], bytes[3]]);
            (mn!("#${:02X},${address:04X}", bytes[1]), None)
        }
        AddressMode::TestAbsoluteX => {
            let address = u16::from_le_bytes([bytes[2], bytes[3]]);
            (mn!("#${:02X},${address:04X},X", bytes[1]), None)
        }
        AddressMode::BlockTransfer => {
            let source = u16::from_le_bytes([bytes[1], bytes[2]]);
            let destination = u16::from_le_bytes([bytes[3], bytes[4]]);
            let length = u16::from_le_bytes([bytes[5], bytes[6]]);
            (mn!("${source:04X},${destination:04X},${length:04X}"), None)
        }
    }
}

fn opcode_info(opcode: u8) -> Option<(&'static str, AddressMode)> {
    use AddressMode::*;

    Some(match opcode {
        0x00 => ("BRK", Break),
        0x01 => ("ORA", IndexedIndirect),
        0x02 => ("SXY", Implied),
        0x03 => ("ST0", Immediate),
        0x04 => ("TSB", DirectPage),
        0x05 => ("ORA", DirectPage),
        0x06 => ("ASL", DirectPage),
        0x07 => ("RMB0", DirectPage),
        0x08 => ("PHP", Implied),
        0x09 => ("ORA", Immediate),
        0x0A => ("ASL", Accumulator),
        0x0C => ("TSB", Absolute),
        0x0D => ("ORA", Absolute),
        0x0E => ("ASL", Absolute),
        0x0F => ("BBR0", DirectPageRelative),
        0x10 => ("BPL", Relative),
        0x11 => ("ORA", IndirectIndexed),
        0x12 => ("ORA", Indirect),
        0x13 => ("ST1", Immediate),
        0x14 => ("TRB", DirectPage),
        0x15 => ("ORA", DirectPageX),
        0x16 => ("ASL", DirectPageX),
        0x17 => ("RMB1", DirectPage),
        0x18 => ("CLC", Implied),
        0x19 => ("ORA", AbsoluteY),
        0x1A => ("INC", Accumulator),
        0x1C => ("TRB", Absolute),
        0x1D => ("ORA", AbsoluteX),
        0x1E => ("ASL", AbsoluteX),
        0x1F => ("BBR1", DirectPageRelative),
        0x20 => ("JSR", Absolute),
        0x21 => ("AND", IndexedIndirect),
        0x22 => ("SAX", Implied),
        0x23 => ("ST2", Immediate),
        0x24 => ("BIT", DirectPage),
        0x25 => ("AND", DirectPage),
        0x26 => ("ROL", DirectPage),
        0x27 => ("RMB2", DirectPage),
        0x28 => ("PLP", Implied),
        0x29 => ("AND", Immediate),
        0x2A => ("ROL", Accumulator),
        0x2C => ("BIT", Absolute),
        0x2D => ("AND", Absolute),
        0x2E => ("ROL", Absolute),
        0x2F => ("BBR2", DirectPageRelative),
        0x30 => ("BMI", Relative),
        0x31 => ("AND", IndirectIndexed),
        0x32 => ("AND", Indirect),
        0x34 => ("BIT", DirectPageX),
        0x35 => ("AND", DirectPageX),
        0x36 => ("ROL", DirectPageX),
        0x37 => ("RMB3", DirectPage),
        0x38 => ("SEC", Implied),
        0x39 => ("AND", AbsoluteY),
        0x3A => ("DEC", Accumulator),
        0x3C => ("BIT", AbsoluteX),
        0x3D => ("AND", AbsoluteX),
        0x3E => ("ROL", AbsoluteX),
        0x3F => ("BBR3", DirectPageRelative),
        0x40 => ("RTI", Implied),
        0x41 => ("EOR", IndexedIndirect),
        0x42 => ("SAY", Implied),
        0x43 => ("TMA", Immediate),
        0x44 => ("BSR", Relative),
        0x45 => ("EOR", DirectPage),
        0x46 => ("LSR", DirectPage),
        0x47 => ("RMB4", DirectPage),
        0x48 => ("PHA", Implied),
        0x49 => ("EOR", Immediate),
        0x4A => ("LSR", Accumulator),
        0x4C => ("JMP", Absolute),
        0x4D => ("EOR", Absolute),
        0x4E => ("LSR", Absolute),
        0x4F => ("BBR4", DirectPageRelative),
        0x50 => ("BVC", Relative),
        0x51 => ("EOR", IndirectIndexed),
        0x52 => ("EOR", Indirect),
        0x53 => ("TAM", Immediate),
        0x54 => ("CSL", Implied),
        0x55 => ("EOR", DirectPageX),
        0x56 => ("LSR", DirectPageX),
        0x57 => ("RMB5", DirectPage),
        0x58 => ("CLI", Implied),
        0x59 => ("EOR", AbsoluteY),
        0x5A => ("PHY", Implied),
        0x5D => ("EOR", AbsoluteX),
        0x5E => ("LSR", AbsoluteX),
        0x5F => ("BBR5", DirectPageRelative),
        0x60 => ("RTS", Implied),
        0x61 => ("ADC", IndexedIndirect),
        0x62 => ("CLA", Implied),
        0x64 => ("STZ", DirectPage),
        0x65 => ("ADC", DirectPage),
        0x66 => ("ROR", DirectPage),
        0x67 => ("RMB6", DirectPage),
        0x68 => ("PLA", Implied),
        0x69 => ("ADC", Immediate),
        0x6A => ("ROR", Accumulator),
        0x6C => ("JMP", AbsoluteIndirect),
        0x6D => ("ADC", Absolute),
        0x6E => ("ROR", Absolute),
        0x6F => ("BBR6", DirectPageRelative),
        0x70 => ("BVS", Relative),
        0x71 => ("ADC", IndirectIndexed),
        0x72 => ("ADC", Indirect),
        0x73 => ("TII", BlockTransfer),
        0x74 => ("STZ", DirectPageX),
        0x75 => ("ADC", DirectPageX),
        0x76 => ("ROR", DirectPageX),
        0x77 => ("RMB7", DirectPage),
        0x78 => ("SEI", Implied),
        0x79 => ("ADC", AbsoluteY),
        0x7A => ("PLY", Implied),
        0x7C => ("JMP", AbsoluteIndexedIndirect),
        0x7D => ("ADC", AbsoluteX),
        0x7E => ("ROR", AbsoluteX),
        0x7F => ("BBR7", DirectPageRelative),
        0x80 => ("BRA", Relative),
        0x81 => ("STA", IndexedIndirect),
        0x82 => ("CLX", Implied),
        0x83 => ("TST", TestDirectPage),
        0x84 => ("STY", DirectPage),
        0x85 => ("STA", DirectPage),
        0x86 => ("STX", DirectPage),
        0x87 => ("SMB0", DirectPage),
        0x88 => ("DEY", Implied),
        0x89 => ("BIT", Immediate),
        0x8A => ("TXA", Implied),
        0x8C => ("STY", Absolute),
        0x8D => ("STA", Absolute),
        0x8E => ("STX", Absolute),
        0x8F => ("BBS0", DirectPageRelative),
        0x90 => ("BCC", Relative),
        0x91 => ("STA", IndirectIndexed),
        0x92 => ("STA", Indirect),
        0x93 => ("TST", TestAbsolute),
        0x94 => ("STY", DirectPageX),
        0x95 => ("STA", DirectPageX),
        0x96 => ("STX", DirectPageY),
        0x97 => ("SMB1", DirectPage),
        0x98 => ("TYA", Implied),
        0x99 => ("STA", AbsoluteY),
        0x9A => ("TXS", Implied),
        0x9C => ("STZ", Absolute),
        0x9D => ("STA", AbsoluteX),
        0x9E => ("STZ", AbsoluteX),
        0x9F => ("BBS1", DirectPageRelative),
        0xA0 => ("LDY", Immediate),
        0xA1 => ("LDA", IndexedIndirect),
        0xA2 => ("LDX", Immediate),
        0xA3 => ("TST", TestDirectPageX),
        0xA4 => ("LDY", DirectPage),
        0xA5 => ("LDA", DirectPage),
        0xA6 => ("LDX", DirectPage),
        0xA7 => ("SMB2", DirectPage),
        0xA8 => ("TAY", Implied),
        0xA9 => ("LDA", Immediate),
        0xAA => ("TAX", Implied),
        0xAC => ("LDY", Absolute),
        0xAD => ("LDA", Absolute),
        0xAE => ("LDX", Absolute),
        0xAF => ("BBS2", DirectPageRelative),
        0xB0 => ("BCS", Relative),
        0xB1 => ("LDA", IndirectIndexed),
        0xB2 => ("LDA", Indirect),
        0xB3 => ("TST", TestAbsoluteX),
        0xB4 => ("LDY", DirectPageX),
        0xB5 => ("LDA", DirectPageX),
        0xB6 => ("LDX", DirectPageY),
        0xB7 => ("SMB3", DirectPage),
        0xB8 => ("CLV", Implied),
        0xB9 => ("LDA", AbsoluteY),
        0xBA => ("TSX", Implied),
        0xBC => ("LDY", AbsoluteX),
        0xBD => ("LDA", AbsoluteX),
        0xBE => ("LDX", AbsoluteY),
        0xBF => ("BBS3", DirectPageRelative),
        0xC0 => ("CPY", Immediate),
        0xC1 => ("CMP", IndexedIndirect),
        0xC2 => ("CLY", Implied),
        0xC3 => ("TDD", BlockTransfer),
        0xC4 => ("CPY", DirectPage),
        0xC5 => ("CMP", DirectPage),
        0xC6 => ("DEC", DirectPage),
        0xC7 => ("SMB4", DirectPage),
        0xC8 => ("INY", Implied),
        0xC9 => ("CMP", Immediate),
        0xCA => ("DEX", Implied),
        0xCC => ("CPY", Absolute),
        0xCD => ("CMP", Absolute),
        0xCE => ("DEC", Absolute),
        0xCF => ("BBS4", DirectPageRelative),
        0xD0 => ("BNE", Relative),
        0xD1 => ("CMP", IndirectIndexed),
        0xD2 => ("CMP", Indirect),
        0xD3 => ("TIN", BlockTransfer),
        0xD4 => ("CSH", Implied),
        0xD5 => ("CMP", DirectPageX),
        0xD6 => ("DEC", DirectPageX),
        0xD7 => ("SMB5", DirectPage),
        0xD8 => ("CLD", Implied),
        0xD9 => ("CMP", AbsoluteY),
        0xDA => ("PHX", Implied),
        0xDD => ("CMP", AbsoluteX),
        0xDE => ("DEC", AbsoluteX),
        0xDF => ("BBS5", DirectPageRelative),
        0xE0 => ("CPX", Immediate),
        0xE1 => ("SBC", IndexedIndirect),
        0xE3 => ("TIA", BlockTransfer),
        0xE4 => ("CPX", DirectPage),
        0xE5 => ("SBC", DirectPage),
        0xE6 => ("INC", DirectPage),
        0xE7 => ("SMB6", DirectPage),
        0xE8 => ("INX", Implied),
        0xE9 => ("SBC", Immediate),
        0xEA => ("NOP", Implied),
        0xEC => ("CPX", Absolute),
        0xED => ("SBC", Absolute),
        0xEE => ("INC", Absolute),
        0xEF => ("BBS6", DirectPageRelative),
        0xF0 => ("BEQ", Relative),
        0xF1 => ("SBC", IndirectIndexed),
        0xF2 => ("SBC", Indirect),
        0xF3 => ("TAI", BlockTransfer),
        0xF4 => ("SET", Implied),
        0xF5 => ("SBC", DirectPageX),
        0xF6 => ("INC", DirectPageX),
        0xF7 => ("SMB7", DirectPage),
        0xF8 => ("SED", Implied),
        0xF9 => ("SBC", AbsoluteY),
        0xFA => ("PLX", Implied),
        0xFD => ("SBC", AbsoluteX),
        0xFE => ("INC", AbsoluteX),
        0xFF => ("BBS7", DirectPageRelative),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{decode_instruction, instruction_len, opcode_info};

    fn decode(bytes: &[u8], address: u16) -> super::DisassembledLine {
        decode_instruction(
            &|read_address| bytes[read_address.wrapping_sub(address) as usize],
            address,
        )
    }

    #[test]
    fn every_documented_opcode_has_the_expected_size() {
        let reserved = [
            0x0B, 0x1B, 0x2B, 0x33, 0x3B, 0x4B, 0x5B, 0x5C, 0x63, 0x6B, 0x7B, 0x8B, 0x9B, 0xAB,
            0xBB, 0xCB, 0xDB, 0xDC, 0xE2, 0xEB, 0xFB, 0xFC,
        ];
        let expected = [
            2, 2, 1, 2, 2, 2, 2, 2, 1, 2, 1, 1, 3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2, 2, 1, 3, 1, 1, 3,
            3, 3, 3, 3, 2, 1, 2, 2, 2, 2, 2, 1, 2, 1, 1, 3, 3, 3, 3, 2, 2, 2, 1, 2, 2, 2, 2, 1, 3,
            1, 1, 3, 3, 3, 3, 1, 2, 1, 2, 2, 2, 2, 2, 1, 2, 1, 1, 3, 3, 3, 3, 2, 2, 2, 2, 1, 2, 2,
            2, 1, 3, 1, 1, 1, 3, 3, 3, 1, 2, 1, 1, 2, 2, 2, 2, 1, 2, 1, 1, 3, 3, 3, 3, 2, 2, 2, 7,
            2, 2, 2, 2, 1, 3, 1, 1, 3, 3, 3, 3, 2, 2, 1, 3, 2, 2, 2, 2, 1, 2, 1, 1, 3, 3, 3, 3, 2,
            2, 2, 4, 2, 2, 2, 2, 1, 3, 1, 1, 3, 3, 3, 3, 2, 2, 2, 3, 2, 2, 2, 2, 1, 2, 1, 1, 3, 3,
            3, 3, 2, 2, 2, 4, 2, 2, 2, 2, 1, 3, 1, 1, 3, 3, 3, 3, 2, 2, 1, 7, 2, 2, 2, 2, 1, 2, 1,
            1, 3, 3, 3, 3, 2, 2, 2, 7, 1, 2, 2, 2, 1, 3, 1, 1, 1, 3, 3, 3, 2, 2, 1, 7, 2, 2, 2, 2,
            1, 2, 1, 1, 3, 3, 3, 3, 2, 2, 2, 7, 1, 2, 2, 2, 1, 3, 1, 1, 1, 3, 3, 3,
        ];

        for opcode in 0..=u8::MAX {
            let actual = opcode_info(opcode).map_or(1, |(_, mode)| super::mode_len(mode));
            assert_eq!(actual, expected[opcode as usize], "opcode {opcode:02X}");
            assert_eq!(
                opcode_info(opcode).is_none(),
                reserved.contains(&opcode),
                "opcode {opcode:02X}"
            );
        }
    }

    #[test]
    fn formats_huc6280_operands_and_control_targets() {
        let branch = decode(&[0x44, 0xFC], 0x1000);
        assert_eq!(branch.mnemonic.as_str(), "BSR $0FFE");
        assert_eq!(branch.control_target, Some(0x0FFEu16.into()));

        let bit_branch = decode(&[0xAF, 0x12, 0x7F], 0x0100);
        assert_eq!(bit_branch.mnemonic.as_str(), "BBS2 $12,$0182");
        assert_eq!(bit_branch.control_target, Some(0x0182u16.into()));

        let transfer = decode(&[0x73, 0x34, 0x12, 0x78, 0x56, 0xBC, 0x9A], 0);
        assert_eq!(transfer.mnemonic.as_str(), "TII $1234,$5678,$9ABC");

        let test = decode(&[0xB3, 0xF0, 0x34, 0x12], 0);
        assert_eq!(test.mnemonic.as_str(), "TST #$F0,$1234,X");

        let jump = decode(&[0x4C, 0x34, 0x12], 0);
        assert_eq!(jump.control_target, Some(0x1234u16.into()));
    }

    #[test]
    fn reserved_opcodes_are_single_byte_data() {
        let line = decode(&[0x5C], 0x2000);
        assert_eq!(instruction_len(&|_| 0x5C, 0x2000), 1);
        assert_eq!(line.mnemonic.as_str(), "DB $5C");
        assert_eq!(line.bytes.as_slice(), &[0x5C]);
    }
}
