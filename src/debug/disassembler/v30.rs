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
        0xC8 => 4,
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
        0x00..=0x03
        | 0x08..=0x0B
        | 0x10..=0x13
        | 0x18..=0x1B
        | 0x20..=0x23
        | 0x28..=0x2B
        | 0x30..=0x33
        | 0x38..=0x3B => {
            let word = opcode & 1 != 0;
            let modrm = at(1);
            let (rm, reg) = modrm_operands(bus_read, address, prefix_len, modrm, word);
            if opcode & 2 == 0 {
                mn!("{} {}, {}", alu_name(opcode), rm, reg)
            } else {
                mn!("{} {}, {}", alu_name(opcode), reg, rm)
            }
        }
        0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x34 | 0x3C => {
            mn!("{} AL, #${:02X}", alu_name(opcode), at(1))
        }
        0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x35 | 0x3D => mn!(
            "{} AX, #${:04X}",
            alu_name(opcode),
            u16::from_le_bytes([at(1), at(2)])
        ),
        0x06 => mn!("PUSH ES"),
        0x07 => mn!("POP ES"),
        0x0E => mn!("PUSH CS"),
        0x16 => mn!("PUSH SS"),
        0x17 => mn!("POP SS"),
        0x1E => mn!("PUSH DS"),
        0x1F => mn!("POP DS"),
        0x27 => mn!("DAA"),
        0x2F => mn!("DAS"),
        0x37 => mn!("AAA"),
        0x3F => mn!("AAS"),
        0x40..=0x47 => mn!("INC {}", reg16(opcode - 0x40)),
        0x48..=0x4F => mn!("DEC {}", reg16(opcode - 0x48)),
        0x50..=0x57 => mn!("PUSH {}", reg16(opcode - 0x50)),
        0x58..=0x5F => mn!("POP {}", reg16(opcode - 0x58)),
        0x60 => mn!("PUSHA"),
        0x61 => mn!("POPA"),
        0x68 => mn!("PUSH #${:04X}", u16::from_le_bytes([at(1), at(2)])),
        0x6A => mn!("PUSH #${:02X}", at(1)),
        0x69 | 0x6B => {
            let modrm = at(1);
            let (rm, reg) = modrm_operands(bus_read, address, prefix_len, modrm, true);
            let immediate = 2 + modrm_displacement_len(modrm);
            if opcode == 0x69 {
                mn!(
                    "IMUL {}, {}, #${:04X}",
                    reg,
                    rm,
                    u16::from_le_bytes([at(immediate), at(immediate + 1)])
                )
            } else {
                mn!("IMUL {}, {}, #${:02X}", reg, rm, at(immediate))
            }
        }
        0x6C => mn!("INSB"),
        0x6D => mn!("INSW"),
        0x6E => mn!("OUTSB"),
        0x6F => mn!("OUTSW"),
        0x80..=0x83 => {
            let modrm = at(1);
            let word = opcode == 0x81 || opcode == 0x83;
            let (rm, _) = modrm_operands(bus_read, address, prefix_len, modrm, word);
            let immediate = 2 + modrm_displacement_len(modrm);
            let operation = group_alu_name((modrm >> 3) & 7);
            if opcode == 0x81 {
                mn!(
                    "{} {}, #${:04X}",
                    operation,
                    rm,
                    u16::from_le_bytes([at(immediate), at(immediate + 1)])
                )
            } else {
                mn!("{} {}, #${:02X}", operation, rm, at(immediate))
            }
        }
        0x84..=0x8B => {
            let word = opcode & 1 != 0;
            let modrm = at(1);
            let (rm, reg) = modrm_operands(bus_read, address, prefix_len, modrm, word);
            match opcode {
                0x84 | 0x85 => mn!("TEST {}, {}", rm, reg),
                0x86 | 0x87 => mn!("XCHG {}, {}", rm, reg),
                0x88 | 0x89 => mn!("MOV {}, {}", rm, reg),
                _ => mn!("MOV {}, {}", reg, rm),
            }
        }
        0x8C | 0x8E => {
            let modrm = at(1);
            let (rm, _) = modrm_operands(bus_read, address, prefix_len, modrm, true);
            let segment = segment_reg((modrm >> 3) & 3);
            if opcode == 0x8C {
                mn!("MOV {}, {}", rm, segment)
            } else {
                mn!("MOV {}, {}", segment, rm)
            }
        }
        0x8D => {
            let modrm = at(1);
            let (rm, reg) = modrm_operands(bus_read, address, prefix_len, modrm, true);
            mn!("LEA {}, {}", reg, rm)
        }
        0x8F => {
            let modrm = at(1);
            let (rm, _) = modrm_operands(bus_read, address, prefix_len, modrm, true);
            mn!("POP {}", rm)
        }
        0x90 => mn!("NOP"),
        0x91..=0x97 => mn!("XCHG AX, {}", reg16(opcode - 0x90)),
        0x98 => mn!("CBW"),
        0x99 => mn!("CWD"),
        0x9B => mn!("WAIT"),
        0x9C => mn!("PUSHF"),
        0x9D => mn!("POPF"),
        0x9E => mn!("SAHF"),
        0x9F => mn!("LAHF"),
        0xA0 => mn!("MOV AL, [${:04X}]", u16::from_le_bytes([at(1), at(2)])),
        0xA1 => mn!("MOV AX, [${:04X}]", u16::from_le_bytes([at(1), at(2)])),
        0xA2 => mn!("MOV [${:04X}], AL", u16::from_le_bytes([at(1), at(2)])),
        0xA3 => mn!("MOV [${:04X}], AX", u16::from_le_bytes([at(1), at(2)])),
        0xA4 => mn!("MOVSB"),
        0xA5 => mn!("MOVSW"),
        0xA6 => mn!("CMPSB"),
        0xA7 => mn!("CMPSW"),
        0xA8 => mn!("TEST AL, #${:02X}", at(1)),
        0xA9 => mn!("TEST AX, #${:04X}", u16::from_le_bytes([at(1), at(2)])),
        0xAA => mn!("STOSB"),
        0xAB => mn!("STOSW"),
        0xAC => mn!("LODSB"),
        0xAD => mn!("LODSW"),
        0xAE => mn!("SCASB"),
        0xAF => mn!("SCASW"),
        0xC3 => mn!("RET"),
        0xCB => mn!("RETF"),
        0xCF => mn!("IRET"),
        0xC2 => mn!("RET #${:04X}", u16::from_le_bytes([at(1), at(2)])),
        0xC0 | 0xC1 | 0xD0..=0xD3 => shift_mnemonic(opcode, prefix_len, bus_read, address),
        0xC4 | 0xC5 => {
            let modrm = at(1);
            let (rm, reg) = modrm_operands(bus_read, address, prefix_len, modrm, true);
            mn!(
                "{} {}, {}",
                if opcode == 0xC4 { "LES" } else { "LDS" },
                reg,
                rm
            )
        }
        0xC6 | 0xC7 => {
            let modrm = at(1);
            let word = opcode == 0xC7;
            let (rm, _) = modrm_operands(bus_read, address, prefix_len, modrm, word);
            let immediate = 2 + modrm_displacement_len(modrm);
            if word {
                mn!(
                    "MOV {}, #${:04X}",
                    rm,
                    u16::from_le_bytes([at(immediate), at(immediate + 1)])
                )
            } else {
                mn!("MOV {}, #${:02X}", rm, at(immediate))
            }
        }
        0xC8 => mn!(
            "ENTER #${:04X}, #${:02X}",
            u16::from_le_bytes([at(1), at(2)]),
            at(3)
        ),
        0xC9 => mn!("LEAVE"),
        0xCA => mn!("RETF #${:04X}", u16::from_le_bytes([at(1), at(2)])),
        0xCC => mn!("INT3"),
        0xCE => mn!("INTO"),
        0xD4 => mn!("AAM #${:02X}", at(1)),
        0xD5 => mn!("AAD #${:02X}", at(1)),
        0xD6 => mn!("SALC"),
        0xD7 => mn!("XLAT"),
        0xF4 => mn!("HLT"),
        0xF5 => mn!("CMC"),
        0xF8 => mn!("CLC"),
        0xF9 => mn!("STC"),
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
        0xE0..=0xE3 => mn!(
            "{} ${:05X}",
            ["LOOPNE", "LOOPE", "LOOP", "JCXZ"][(opcode - 0xE0) as usize],
            rel8_target(address, prefix_len + 2, at(1))
        ),
        0xE4 => mn!("IN AL, #${:02X}", at(1)),
        0xE5 => mn!("IN AX, #${:02X}", at(1)),
        0xE6 => mn!("OUT #${:02X}, AL", at(1)),
        0xE7 => mn!("OUT #${:02X}, AX", at(1)),
        0xEC => mn!("IN AL, DX"),
        0xED => mn!("IN AX, DX"),
        0xEE => mn!("OUT DX, AL"),
        0xEF => mn!("OUT DX, AX"),
        0xF6 | 0xF7 | 0xFE | 0xFF => group_mnemonic(opcode, prefix_len, bus_read, address),
        _ if prefix => mn!("PFX ${opcode:02X}"),
        _ => mn!("OP ${opcode:02X}"),
    }
}

fn alu_name(opcode: u8) -> &'static str {
    match opcode & 0x38 {
        0x00 => "ADD",
        0x08 => "OR",
        0x10 => "ADC",
        0x18 => "SBB",
        0x20 => "AND",
        0x28 => "SUB",
        0x30 => "XOR",
        _ => "CMP",
    }
}

fn group_alu_name(group: u8) -> &'static str {
    ["ADD", "OR", "ADC", "SBB", "AND", "SUB", "XOR", "CMP"][group as usize]
}

fn segment_reg(index: u8) -> &'static str {
    ["ES", "CS", "SS", "DS"][index as usize]
}

fn modrm_operands(
    bus_read: &impl Fn(u32) -> u8,
    address: u32,
    prefix_len: usize,
    modrm: u8,
    word: bool,
) -> (String, &'static str) {
    let reg = if word {
        reg16((modrm >> 3) & 7)
    } else {
        reg8((modrm >> 3) & 7)
    };
    (rm_operand(bus_read, address, prefix_len, modrm, word), reg)
}

fn rm_operand(
    bus_read: &impl Fn(u32) -> u8,
    address: u32,
    prefix_len: usize,
    modrm: u8,
    word: bool,
) -> String {
    let mode = modrm >> 6;
    let rm = modrm & 7;
    if mode == 3 {
        return if word { reg16(rm) } else { reg8(rm) }.to_owned();
    }
    let at = |offset| read(bus_read, address, prefix_len + offset);
    if mode == 0 && rm == 6 {
        return format!("[${:04X}]", u16::from_le_bytes([at(2), at(3)]));
    }
    let base = ["BX+SI", "BX+DI", "BP+SI", "BP+DI", "SI", "DI", "BP", "BX"][rm as usize];
    let displacement = match mode {
        1 => i16::from(at(2) as i8),
        2 => i16::from_le_bytes([at(2), at(3)]),
        _ => 0,
    };
    if displacement > 0 {
        format!("[{base}+${displacement:X}]")
    } else if displacement < 0 {
        format!("[{base}-${:X}]", displacement.unsigned_abs())
    } else {
        format!("[{base}]")
    }
}

fn shift_mnemonic(
    opcode: u8,
    prefix_len: usize,
    bus_read: &impl Fn(u32) -> u8,
    address: u32,
) -> Mnemonic {
    let at = |offset| read(bus_read, address, prefix_len + offset);
    let modrm = at(1);
    let word = opcode & 1 != 0;
    let operand = rm_operand(bus_read, address, prefix_len, modrm, word);
    let operation =
        ["ROL", "ROR", "RCL", "RCR", "SHL", "SHR", "SAL", "SAR"][((modrm >> 3) & 7) as usize];
    match opcode {
        0xC0 | 0xC1 => {
            let immediate = 2 + modrm_displacement_len(modrm);
            mn!("{} {}, #${:02X}", operation, operand, at(immediate))
        }
        0xD0 | 0xD1 => mn!("{} {}, 1", operation, operand),
        _ => mn!("{} {}, CL", operation, operand),
    }
}

fn group_mnemonic(
    opcode: u8,
    prefix_len: usize,
    bus_read: &impl Fn(u32) -> u8,
    address: u32,
) -> Mnemonic {
    let at = |offset| read(bus_read, address, prefix_len + offset);
    let modrm = at(1);
    let group = (modrm >> 3) & 7;
    let word = matches!(opcode, 0xF7 | 0xFF);
    let operand = rm_operand(bus_read, address, prefix_len, modrm, word);
    match opcode {
        0xF6 | 0xF7 if group == 0 => {
            let immediate = 2 + modrm_displacement_len(modrm);
            if word {
                mn!(
                    "TEST {}, #${:04X}",
                    operand,
                    u16::from_le_bytes([at(immediate), at(immediate + 1)])
                )
            } else {
                mn!("TEST {}, #${:02X}", operand, at(immediate))
            }
        }
        0xF6 | 0xF7 => mn!(
            "{} {}",
            ["TEST", "TEST", "NOT", "NEG", "MUL", "IMUL", "DIV", "IDIV"][group as usize],
            operand
        ),
        0xFE => mn!("{} {}", if group == 0 { "INC" } else { "DEC" }, operand),
        0xFF => mn!(
            "{} {}",
            ["INC", "DEC", "CALL", "CALLF", "JMP", "JMPF", "PUSH", "OP"][group as usize],
            operand
        ),
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
        0xEB | 0x70..=0x7F | 0xE0..=0xE3 => Some(rel8_target(address, prefix_len + 2, at(1))),
        0x9A | 0xEA => Some(
            (u32::from(u16::from_le_bytes([at(3), at(4)])) << 4)
                .wrapping_add(u32::from(u16::from_le_bytes([at(1), at(2)])))
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
    fn far_targets_use_segment_addition() {
        let bytes = [0x9A, 0x78, 0x56, 0x34, 0x12];
        let lines = disassemble_around(
            &|address| bytes.get(address as usize).copied().unwrap_or(0),
            0,
            0,
            1,
        );
        assert_eq!(lines[0].control_target, Some(0x179B8));
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

    #[test]
    fn decodes_common_register_stack_and_frame_forms() {
        let bytes = [
            0x8B, 0x46, 0xFE, 0x83, 0xC0, 0x01, 0xC8, 0x10, 0x00, 0x02, 0xFF, 0xD0, 0xF4,
        ];
        let lines = disassemble_around(
            &|address| bytes.get(address as usize).copied().unwrap_or(0),
            0,
            0,
            5,
        );
        assert_eq!(lines[0].mnemonic.as_str(), "MOV AX, [BP-$2]");
        assert_eq!(lines[1].mnemonic.as_str(), "ADD AX, #$01");
        assert_eq!(lines[2].mnemonic.as_str(), "ENTER #$0010, #$02");
        assert_eq!(lines[2].bytes.len(), 4);
        assert_eq!(lines[3].mnemonic.as_str(), "CALL AX");
        assert_eq!(lines[4].mnemonic.as_str(), "HLT");
    }
}
