use super::{DisassembledLine, InstructionBytes, Mnemonic};

const ADDRESS_MASK: u32 = 0x000F_FFFF;

pub(super) fn disassemble_around(
    bus_read: &impl Fn(u32) -> u8,
    pc: u32,
    lines_before_pc: usize,
    total_lines: usize,
) -> Vec<DisassembledLine> {
    let start = centered_start(bus_read, pc, lines_before_pc);
    let mut lines = Vec::with_capacity(total_lines);
    let mut address = start;
    for _ in 0..total_lines {
        let line = decode(bus_read, address);
        address = address.wrapping_add(line.bytes.len().max(1) as u32) & ADDRESS_MASK;
        lines.push(line);
    }
    lines
}

fn centered_start(bus_read: &impl Fn(u32) -> u8, pc: u32, before: usize) -> u32 {
    let mut best = pc;
    let mut best_steps = 0;
    for back in 0..=96u32 {
        let candidate = pc.wrapping_sub(back) & ADDRESS_MASK;
        let mut address = candidate;
        let mut steps = 0;
        while steps <= before {
            if address == pc {
                if steps >= best_steps {
                    best = candidate;
                    best_steps = steps;
                }
                break;
            }
            address =
                address.wrapping_add(instruction_len(bus_read, address) as u32) & ADDRESS_MASK;
            steps += 1;
        }
    }
    best
}

fn instruction_len(bus_read: &impl Fn(u32) -> u8, address: u32) -> usize {
    decode(bus_read, address).bytes.len().max(1)
}

fn decode(bus_read: &impl Fn(u32) -> u8, address: u32) -> DisassembledLine {
    let mut pos = 0usize;
    while pos < 4 && is_prefix(read(bus_read, address, pos)) {
        pos += 1;
    }
    let opcode = read(bus_read, address, pos);
    let prefix_len = pos;
    let mut len = prefix_len + base_len(opcode);
    if has_modrm(opcode) {
        let modrm = read(bus_read, address, prefix_len + 1);
        len += 1 + modrm_displacement_len(modrm) + modrm_immediate_len(opcode, modrm);
    }
    let mut bytes = InstructionBytes::new();
    for i in 0..len.min(16) {
        bytes.push(read(bus_read, address, i));
    }
    let mut line = DisassembledLine {
        address,
        storage_offset: None,
        symbol: None,
        control_target: None,
        control_target_storage: None,
        control_target_symbol: None,
        source: None,
        bytes,
        mnemonic: mnemonic(opcode, prefix_len, bus_read, address),
    };
    if let Some(target) = control_target(opcode, prefix_len, bus_read, address) {
        line.control_target = Some(target);
    }
    line
}

fn read(bus_read: &impl Fn(u32) -> u8, address: u32, offset: usize) -> u8 {
    bus_read(address.wrapping_add(offset as u32) & ADDRESS_MASK)
}

fn is_prefix(opcode: u8) -> bool {
    matches!(
        opcode,
        0x26 | 0x2E | 0x36 | 0x3E | 0x64 | 0x65 | 0x66 | 0x67 | 0xF0 | 0xF2 | 0xF3
    )
}

fn base_len(opcode: u8) -> usize {
    match opcode {
        0x04
        | 0x0C
        | 0x14
        | 0x1C
        | 0x24
        | 0x2C
        | 0x34
        | 0x3C
        | 0x6A
        | 0xA8
        | 0xCD
        | 0xD4
        | 0xD5
        | 0xE0..=0xE7
        | 0xEB
        | 0x70..=0x7F
        | 0xB0..=0xB7 => 2,
        0x05
        | 0x0D
        | 0x15
        | 0x1D
        | 0x25
        | 0x2D
        | 0x35
        | 0x3D
        | 0x68
        | 0xA0..=0xA3
        | 0xA9
        | 0xB8..=0xBF
        | 0xC2
        | 0xCA
        | 0xE8
        | 0xE9 => 3,
        0x9A | 0xEA => 5,
        _ => 1,
    }
}

fn has_modrm(opcode: u8) -> bool {
    matches!(opcode,
        0x00..=0x03 | 0x08..=0x0B | 0x10..=0x13 | 0x18..=0x1B | 0x20..=0x23 | 0x28..=0x2B |
        0x30..=0x33 | 0x38..=0x3B | 0x62 | 0x63 | 0x69 | 0x6B | 0x80..=0x8F | 0xC0 | 0xC1 |
        0xC4..=0xC7 | 0xD0..=0xD3 | 0xD8..=0xDF | 0xF6 | 0xF7 | 0xFE | 0xFF)
}

fn modrm_displacement_len(modrm: u8) -> usize {
    match modrm >> 6 {
        0 => usize::from((modrm & 7) == 6) * 2,
        1 => 1,
        2 => 2,
        _ => 0,
    }
}

fn modrm_immediate_len(opcode: u8, modrm: u8) -> usize {
    match opcode {
        0x69 | 0x81 | 0xC7 => 2,
        0x6B | 0x80 | 0x82 | 0x83 | 0xC0 | 0xC1 | 0xC6 => 1,
        0xF6 if (modrm >> 3) & 7 == 0 => 1,
        0xF7 if (modrm >> 3) & 7 == 0 => 2,
        _ => 0,
    }
}

fn mnemonic(
    opcode: u8,
    prefix_len: usize,
    bus_read: &impl Fn(u32) -> u8,
    address: u32,
) -> Mnemonic {
    let prefix = prefix_len > 0;
    let at = |offset| read(bus_read, address, prefix_len + offset);
    match opcode {
        0x90 => mn!("NOP"),
        0xC3 => mn!("RET"),
        0xCB => mn!("RETF"),
        0xCF => mn!("IRET"),
        0xF4 => mn!("HLT"),
        0xFA => mn!("CLI"),
        0xFB => mn!("STI"),
        0xFC => mn!("CLD"),
        0xFD => mn!("STD"),
        0xE8 => mn!(
            "CALL ${:05X}",
            near_target(address, prefix_len + 3, at(1), at(2))
        ),
        0xE9 => mn!(
            "JMP ${:05X}",
            near_target(address, prefix_len + 3, at(1), at(2))
        ),
        0xEB => mn!("JMP ${:05X}", rel8_target(address, prefix_len + 2, at(1))),
        0x70..=0x7F => mn!(
            "{} ${:05X}",
            condition(opcode),
            rel8_target(address, prefix_len + 2, at(1))
        ),
        0x9A => mn!(
            "CALLF ${:04X}:${:04X}",
            u16::from_le_bytes([at(3), at(4)]),
            u16::from_le_bytes([at(1), at(2)])
        ),
        0xEA => mn!(
            "JMPF ${:04X}:${:04X}",
            u16::from_le_bytes([at(3), at(4)]),
            u16::from_le_bytes([at(1), at(2)])
        ),
        0xB0..=0xB7 => mn!("MOV {}, #${:02X}", reg8(opcode & 7), at(1)),
        0xB8..=0xBF => mn!(
            "MOV {}, #${:04X}",
            reg16(opcode & 7),
            u16::from_le_bytes([at(1), at(2)])
        ),
        0xCD => mn!("INT #${:02X}", at(1)),
        _ if prefix => mn!("PFX ${opcode:02X}"),
        _ => mn!("OP ${opcode:02X}"),
    }
}

fn control_target(
    opcode: u8,
    prefix_len: usize,
    bus_read: &impl Fn(u32) -> u8,
    address: u32,
) -> Option<u32> {
    let at = |offset| read(bus_read, address, prefix_len + offset);
    match opcode {
        0xE8 | 0xE9 => Some(near_target(address, prefix_len + 3, at(1), at(2))),
        0xEB | 0x70..=0x7F => Some(rel8_target(address, prefix_len + 2, at(1))),
        0x9A | 0xEA => Some(
            (u32::from(u16::from_le_bytes([at(3), at(4)])) << 4
                | u32::from(u16::from_le_bytes([at(1), at(2)])))
                & ADDRESS_MASK,
        ),
        _ => None,
    }
}

fn near_target(address: u32, len: usize, lo: u8, hi: u8) -> u32 {
    address
        .wrapping_add(len as u32)
        .wrapping_add_signed(i32::from(i16::from_le_bytes([lo, hi])))
        & ADDRESS_MASK
}

fn rel8_target(address: u32, len: usize, offset: u8) -> u32 {
    address
        .wrapping_add(len as u32)
        .wrapping_add_signed(i32::from(offset as i8))
        & ADDRESS_MASK
}

fn condition(opcode: u8) -> &'static str {
    [
        "JO", "JNO", "JB", "JAE", "JE", "JNE", "JBE", "JA", "JS", "JNS", "JP", "JNP", "JL", "JGE",
        "JLE", "JG",
    ][(opcode & 0x0F) as usize]
}
fn reg8(index: u8) -> &'static str {
    ["AL", "CL", "DL", "BL", "AH", "CH", "DH", "BH"][index as usize]
}
fn reg16(index: u8) -> &'static str {
    ["AX", "CX", "DX", "BX", "SP", "BP", "SI", "DI"][index as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_control_flow_targets() {
        let bytes = [0xE8, 0x04, 0x00, 0x75, 0xFB];
        let lines = disassemble_around(
            &|address| bytes.get(address as usize).copied().unwrap_or(0),
            0,
            0,
            2,
        );
        assert_eq!(lines[0].mnemonic.as_str(), "CALL $00007");
        assert_eq!(lines[0].control_target, Some(7));
        assert_eq!(lines[1].mnemonic.as_str(), "JNE $00000");
        assert_eq!(lines[1].control_target, Some(0));
    }

    #[test]
    fn keeps_modrm_immediate_forms_aligned() {
        let bytes = [0x83, 0xC0, 0x01, 0x90, 0xC7, 0x46, 0xFE, 0x34, 0x12, 0xF4];
        let lines = disassemble_around(
            &|address| bytes.get(address as usize).copied().unwrap_or(0),
            0,
            0,
            4,
        );
        assert_eq!(lines[0].bytes.len(), 3);
        assert_eq!(lines[1].mnemonic.as_str(), "NOP");
        assert_eq!(lines[2].bytes.len(), 5);
        assert_eq!(lines[3].mnemonic.as_str(), "HLT");
    }
}
