use super::{DisassembledLine, Mnemonic};
use std::fmt::Write;

macro_rules! mn {
    ($($arg:tt)*) => {{
        let mut s = Mnemonic::new();
        let _ = write!(s, $($arg)*);
        s
    }};
}

#[derive(Clone, Copy)]
enum NesAddrMode {
    Imp,
    Acc,
    Imm,
    Zp,
    ZpX,
    ZpY,
    Abs,
    AbsX,
    AbsY,
    Ind,
    IndX,
    IndY,
    Rel,
}

pub(crate) fn disassemble_around(
    bus_read: impl Fn(u16) -> u8,
    pc: u16,
    lines_before_pc: usize,
    total_lines: usize,
) -> Vec<DisassembledLine> {
    let start = choose_centered_start(&bus_read, pc, lines_before_pc);
    let mut lines = Vec::with_capacity(total_lines);
    let mut addr = start;
    for _ in 0..total_lines {
        let line = decode_instruction(&bus_read, addr);
        let len = line.bytes.len().max(1) as u16;
        addr = addr.wrapping_add(len);
        lines.push(line);
    }
    lines
}

fn choose_centered_start(bus_read: &impl Fn(u16) -> u8, pc: u16, lines_before_pc: usize) -> u16 {
    let mut best_start = pc;
    let mut best_steps = 0usize;

    for back in 0u16..=96 {
        let candidate = pc.wrapping_sub(back);
        let mut addr = candidate;
        let mut steps = 0usize;
        while steps <= lines_before_pc {
            if addr == pc {
                if steps >= best_steps {
                    best_steps = steps;
                    best_start = candidate;
                }
                break;
            }
            addr = addr.wrapping_add(instruction_len(bus_read, addr) as u16);
            steps += 1;
        }
    }

    best_start
}

fn instruction_len(bus_read: &impl Fn(u16) -> u8, addr: u16) -> usize {
    let opcode = bus_read(addr);
    match opcode_info(opcode) {
        Some((_, mode)) => mode_len(mode),
        None => 1,
    }
}

fn mode_len(mode: NesAddrMode) -> usize {
    match mode {
        NesAddrMode::Imp | NesAddrMode::Acc => 1,
        NesAddrMode::Imm
        | NesAddrMode::Zp
        | NesAddrMode::ZpX
        | NesAddrMode::ZpY
        | NesAddrMode::IndX
        | NesAddrMode::IndY
        | NesAddrMode::Rel => 2,
        NesAddrMode::Abs | NesAddrMode::AbsX | NesAddrMode::AbsY | NesAddrMode::Ind => 3,
    }
}

fn decode_instruction(bus_read: &impl Fn(u16) -> u8, addr: u16) -> DisassembledLine {
    let opcode = bus_read(addr);
    let Some((mnemonic, mode)) = opcode_info(opcode) else {
        return DisassembledLine {
            address: addr,
            bytes: vec![opcode],
            mnemonic: mn!("DB ${:02X}", opcode),
        };
    };

    let len = mode_len(mode);
    let bytes = (0..len)
        .map(|i| bus_read(addr.wrapping_add(i as u16)))
        .collect::<Vec<_>>();

    let operand = match mode {
        NesAddrMode::Imp => Mnemonic::new(),
        NesAddrMode::Acc => mn!("A"),
        NesAddrMode::Imm => mn!("#${:02X}", bytes[1]),
        NesAddrMode::Zp => mn!("${:02X}", bytes[1]),
        NesAddrMode::ZpX => mn!("${:02X},X", bytes[1]),
        NesAddrMode::ZpY => mn!("${:02X},Y", bytes[1]),
        NesAddrMode::Abs => mn!("${:04X}", u16::from_le_bytes([bytes[1], bytes[2]])),
        NesAddrMode::AbsX => mn!("${:04X},X", u16::from_le_bytes([bytes[1], bytes[2]])),
        NesAddrMode::AbsY => mn!("${:04X},Y", u16::from_le_bytes([bytes[1], bytes[2]])),
        NesAddrMode::Ind => mn!("(${:04X})", u16::from_le_bytes([bytes[1], bytes[2]])),
        NesAddrMode::IndX => mn!("(${:02X},X)", bytes[1]),
        NesAddrMode::IndY => mn!("(${:02X}),Y", bytes[1]),
        NesAddrMode::Rel => {
            let rel = bytes[1] as i8;
            let target = addr.wrapping_add(2).wrapping_add_signed(rel as i16);
            mn!("{} (${:04X})", fmt_signed(rel), target)
        }
    };

    let rendered = if operand.is_empty() {
        mn!("{}", mnemonic)
    } else {
        mn!("{} {}", mnemonic, operand)
    };

    DisassembledLine {
        address: addr,
        bytes,
        mnemonic: rendered,
    }
}

fn opcode_info(op: u8) -> Option<(&'static str, NesAddrMode)> {
    use NesAddrMode::*;
    Some(match op {
        0x00 => ("BRK", Imp),
        0x01 => ("ORA", IndX),
        0x05 => ("ORA", Zp),
        0x06 => ("ASL", Zp),
        0x08 => ("PHP", Imp),
        0x09 => ("ORA", Imm),
        0x0A => ("ASL", Acc),
        0x0D => ("ORA", Abs),
        0x0E => ("ASL", Abs),
        0x10 => ("BPL", Rel),
        0x11 => ("ORA", IndY),
        0x15 => ("ORA", ZpX),
        0x16 => ("ASL", ZpX),
        0x18 => ("CLC", Imp),
        0x19 => ("ORA", AbsY),
        0x1D => ("ORA", AbsX),
        0x1E => ("ASL", AbsX),
        0x20 => ("JSR", Abs),
        0x21 => ("AND", IndX),
        0x24 => ("BIT", Zp),
        0x25 => ("AND", Zp),
        0x26 => ("ROL", Zp),
        0x28 => ("PLP", Imp),
        0x29 => ("AND", Imm),
        0x2A => ("ROL", Acc),
        0x2C => ("BIT", Abs),
        0x2D => ("AND", Abs),
        0x2E => ("ROL", Abs),
        0x30 => ("BMI", Rel),
        0x31 => ("AND", IndY),
        0x35 => ("AND", ZpX),
        0x36 => ("ROL", ZpX),
        0x38 => ("SEC", Imp),
        0x39 => ("AND", AbsY),
        0x3D => ("AND", AbsX),
        0x3E => ("ROL", AbsX),
        0x40 => ("RTI", Imp),
        0x41 => ("EOR", IndX),
        0x45 => ("EOR", Zp),
        0x46 => ("LSR", Zp),
        0x48 => ("PHA", Imp),
        0x49 => ("EOR", Imm),
        0x4A => ("LSR", Acc),
        0x4C => ("JMP", Abs),
        0x4D => ("EOR", Abs),
        0x4E => ("LSR", Abs),
        0x50 => ("BVC", Rel),
        0x51 => ("EOR", IndY),
        0x55 => ("EOR", ZpX),
        0x56 => ("LSR", ZpX),
        0x58 => ("CLI", Imp),
        0x59 => ("EOR", AbsY),
        0x5D => ("EOR", AbsX),
        0x5E => ("LSR", AbsX),
        0x60 => ("RTS", Imp),
        0x61 => ("ADC", IndX),
        0x65 => ("ADC", Zp),
        0x66 => ("ROR", Zp),
        0x68 => ("PLA", Imp),
        0x69 => ("ADC", Imm),
        0x6A => ("ROR", Acc),
        0x6C => ("JMP", Ind),
        0x6D => ("ADC", Abs),
        0x6E => ("ROR", Abs),
        0x70 => ("BVS", Rel),
        0x71 => ("ADC", IndY),
        0x75 => ("ADC", ZpX),
        0x76 => ("ROR", ZpX),
        0x78 => ("SEI", Imp),
        0x79 => ("ADC", AbsY),
        0x7D => ("ADC", AbsX),
        0x7E => ("ROR", AbsX),
        0x81 => ("STA", IndX),
        0x84 => ("STY", Zp),
        0x85 => ("STA", Zp),
        0x86 => ("STX", Zp),
        0x88 => ("DEY", Imp),
        0x8A => ("TXA", Imp),
        0x8C => ("STY", Abs),
        0x8D => ("STA", Abs),
        0x8E => ("STX", Abs),
        0x90 => ("BCC", Rel),
        0x91 => ("STA", IndY),
        0x94 => ("STY", ZpX),
        0x95 => ("STA", ZpX),
        0x96 => ("STX", ZpY),
        0x98 => ("TYA", Imp),
        0x99 => ("STA", AbsY),
        0x9A => ("TXS", Imp),
        0x9D => ("STA", AbsX),
        0xA0 => ("LDY", Imm),
        0xA1 => ("LDA", IndX),
        0xA2 => ("LDX", Imm),
        0xA4 => ("LDY", Zp),
        0xA5 => ("LDA", Zp),
        0xA6 => ("LDX", Zp),
        0xA8 => ("TAY", Imp),
        0xA9 => ("LDA", Imm),
        0xAA => ("TAX", Imp),
        0xAC => ("LDY", Abs),
        0xAD => ("LDA", Abs),
        0xAE => ("LDX", Abs),
        0xB0 => ("BCS", Rel),
        0xB1 => ("LDA", IndY),
        0xB4 => ("LDY", ZpX),
        0xB5 => ("LDA", ZpX),
        0xB6 => ("LDX", ZpY),
        0xB8 => ("CLV", Imp),
        0xB9 => ("LDA", AbsY),
        0xBA => ("TSX", Imp),
        0xBC => ("LDY", AbsX),
        0xBD => ("LDA", AbsX),
        0xBE => ("LDX", AbsY),
        0xC0 => ("CPY", Imm),
        0xC1 => ("CMP", IndX),
        0xC4 => ("CPY", Zp),
        0xC5 => ("CMP", Zp),
        0xC6 => ("DEC", Zp),
        0xC8 => ("INY", Imp),
        0xC9 => ("CMP", Imm),
        0xCA => ("DEX", Imp),
        0xCC => ("CPY", Abs),
        0xCD => ("CMP", Abs),
        0xCE => ("DEC", Abs),
        0xD0 => ("BNE", Rel),
        0xD1 => ("CMP", IndY),
        0xD5 => ("CMP", ZpX),
        0xD6 => ("DEC", ZpX),
        0xD8 => ("CLD", Imp),
        0xD9 => ("CMP", AbsY),
        0xDD => ("CMP", AbsX),
        0xDE => ("DEC", AbsX),
        0xE0 => ("CPX", Imm),
        0xE1 => ("SBC", IndX),
        0xE4 => ("CPX", Zp),
        0xE5 => ("SBC", Zp),
        0xE6 => ("INC", Zp),
        0xE8 => ("INX", Imp),
        0xE9 => ("SBC", Imm),
        0xEA => ("NOP", Imp),
        0xEC => ("CPX", Abs),
        0xED => ("SBC", Abs),
        0xEE => ("INC", Abs),
        0xF0 => ("BEQ", Rel),
        0xF1 => ("SBC", IndY),
        0xF5 => ("SBC", ZpX),
        0xF6 => ("INC", ZpX),
        0xF8 => ("SED", Imp),
        0xF9 => ("SBC", AbsY),
        0xFD => ("SBC", AbsX),
        0xFE => ("INC", AbsX),
        _ => return None,
    })
}

fn fmt_signed(value: i8) -> Mnemonic {
    if value < 0 {
        mn!("-${:02X}", value.unsigned_abs())
    } else {
        mn!("+${:02X}", value as u8)
    }
}

