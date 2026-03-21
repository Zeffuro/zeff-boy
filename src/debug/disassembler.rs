#[derive(Clone)]
pub(crate) struct DisassembledLine {
    pub(crate) address: u16,
    pub(crate) bytes: Vec<u8>,
    pub(crate) mnemonic: String,
}

#[derive(Clone)]
pub(crate) struct DisassemblyView {
    pub(crate) pc: u16,
    pub(crate) lines: Vec<DisassembledLine>,
    pub(crate) breakpoints: Vec<u16>,
}

const REG8: [&str; 8] = ["B", "C", "D", "E", "H", "L", "(HL)", "A"];
const ALU_OPS: [&str; 8] = ["ADD A", "ADC A", "SUB", "SBC A", "AND", "XOR", "OR", "CP"];
const ROT_OPS: [&str; 8] = ["RLC", "RRC", "RL", "RR", "SLA", "SRA", "SWAP", "SRL"];

pub(crate) fn disassemble_at(
    bus_read: impl Fn(u16) -> u8,
    start: u16,
    count: usize,
) -> Vec<DisassembledLine> {
    let mut lines = Vec::with_capacity(count);
    let mut addr = start;
    for _ in 0..count {
        let line = decode_instruction(&bus_read, addr);
        let len = line.bytes.len().max(1) as u16;
        addr = addr.wrapping_add(len);
        lines.push(line);
    }
    lines
}

pub(crate) fn disassemble_around(
    bus_read: impl Fn(u16) -> u8,
    pc: u16,
    lines_before_pc: usize,
    total_lines: usize,
) -> Vec<DisassembledLine> {
    let start = choose_centered_start(&bus_read, pc, lines_before_pc);
    disassemble_at(bus_read, start, total_lines)
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
    if opcode == 0xCB {
        return 2;
    }

    match opcode {
        0x01 | 0x08 | 0x11 | 0x21 | 0x31 | 0xC2 | 0xC3 | 0xC4 | 0xCA | 0xCC | 0xCD | 0xD2
        | 0xD4 | 0xDA | 0xDC | 0xEA | 0xFA => 3,
        0x06 | 0x0E | 0x10 | 0x16 | 0x18 | 0x1E | 0x20 | 0x26 | 0x28 | 0x2E | 0x30 | 0x36
        | 0x38 | 0x3E | 0xC6 | 0xCE | 0xD6 | 0xDE | 0xE0 | 0xE6 | 0xE8 | 0xEE | 0xF0 | 0xF6
        | 0xF8 | 0xFE => 2,
        _ => 1,
    }
}

fn decode_instruction(bus_read: &impl Fn(u16) -> u8, addr: u16) -> DisassembledLine {
    let opcode = bus_read(addr);
    if opcode == 0xCB {
        let cb = bus_read(addr.wrapping_add(1));
        return DisassembledLine {
            address: addr,
            bytes: vec![opcode, cb],
            mnemonic: cb_mnemonic(cb),
        };
    }

    let len = instruction_len(bus_read, addr);
    let bytes = (0..len)
        .map(|i| bus_read(addr.wrapping_add(i as u16)))
        .collect::<Vec<_>>();

    let d8 = bus_read(addr.wrapping_add(1));
    let d16 = read_u16(bus_read, addr.wrapping_add(1));
    let r8 = d8 as i8;
    let r8_target = addr.wrapping_add(2).wrapping_add_signed(r8 as i16);

    let mnemonic = match opcode {
        0x00 => "NOP".to_string(),
        0x01 => format!("LD BC,${:04X}", d16),
        0x02 => "LD (BC),A".to_string(),
        0x03 => "INC BC".to_string(),
        0x04 => "INC B".to_string(),
        0x05 => "DEC B".to_string(),
        0x06 => format!("LD B,${:02X}", d8),
        0x07 => "RLCA".to_string(),
        0x08 => format!("LD (${:04X}),SP", d16),
        0x09 => "ADD HL,BC".to_string(),
        0x0A => "LD A,(BC)".to_string(),
        0x0B => "DEC BC".to_string(),
        0x0C => "INC C".to_string(),
        0x0D => "DEC C".to_string(),
        0x0E => format!("LD C,${:02X}", d8),
        0x0F => "RRCA".to_string(),
        0x10 => "STOP".to_string(),
        0x11 => format!("LD DE,${:04X}", d16),
        0x12 => "LD (DE),A".to_string(),
        0x13 => "INC DE".to_string(),
        0x14 => "INC D".to_string(),
        0x15 => "DEC D".to_string(),
        0x16 => format!("LD D,${:02X}", d8),
        0x17 => "RLA".to_string(),
        0x18 => format!("JR {}", fmt_rel(r8, r8_target)),
        0x19 => "ADD HL,DE".to_string(),
        0x1A => "LD A,(DE)".to_string(),
        0x1B => "DEC DE".to_string(),
        0x1C => "INC E".to_string(),
        0x1D => "DEC E".to_string(),
        0x1E => format!("LD E,${:02X}", d8),
        0x1F => "RRA".to_string(),
        0x20 => format!("JR NZ,{}", fmt_rel(r8, r8_target)),
        0x21 => format!("LD HL,${:04X}", d16),
        0x22 => "LD (HL+),A".to_string(),
        0x23 => "INC HL".to_string(),
        0x24 => "INC H".to_string(),
        0x25 => "DEC H".to_string(),
        0x26 => format!("LD H,${:02X}", d8),
        0x27 => "DAA".to_string(),
        0x28 => format!("JR Z,{}", fmt_rel(r8, r8_target)),
        0x29 => "ADD HL,HL".to_string(),
        0x2A => "LD A,(HL+)".to_string(),
        0x2B => "DEC HL".to_string(),
        0x2C => "INC L".to_string(),
        0x2D => "DEC L".to_string(),
        0x2E => format!("LD L,${:02X}", d8),
        0x2F => "CPL".to_string(),
        0x30 => format!("JR NC,{}", fmt_rel(r8, r8_target)),
        0x31 => format!("LD SP,${:04X}", d16),
        0x32 => "LD (HL-),A".to_string(),
        0x33 => "INC SP".to_string(),
        0x34 => "INC (HL)".to_string(),
        0x35 => "DEC (HL)".to_string(),
        0x36 => format!("LD (HL),${:02X}", d8),
        0x37 => "SCF".to_string(),
        0x38 => format!("JR C,{}", fmt_rel(r8, r8_target)),
        0x39 => "ADD HL,SP".to_string(),
        0x3A => "LD A,(HL-)".to_string(),
        0x3B => "DEC SP".to_string(),
        0x3C => "INC A".to_string(),
        0x3D => "DEC A".to_string(),
        0x3E => format!("LD A,${:02X}", d8),
        0x3F => "CCF".to_string(),
        0x40..=0x7F => {
            if opcode == 0x76 {
                "HALT".to_string()
            } else {
                let dst = REG8[((opcode >> 3) & 0x07) as usize];
                let src = REG8[(opcode & 0x07) as usize];
                format!("LD {},{}", dst, src)
            }
        }
        0x80..=0xBF => {
            let op = ALU_OPS[((opcode >> 3) & 0x07) as usize];
            let src = REG8[(opcode & 0x07) as usize];
            if opcode < 0x90 {
                format!("{}{}", op, format!(",{}", src))
            } else if opcode < 0xA0 {
                format!("{} {}", op, src)
            } else {
                format!("{} {}", op, src)
            }
        }
        0xC0 => "RET NZ".to_string(),
        0xC1 => "POP BC".to_string(),
        0xC2 => format!("JP NZ,${:04X}", d16),
        0xC3 => format!("JP ${:04X}", d16),
        0xC4 => format!("CALL NZ,${:04X}", d16),
        0xC5 => "PUSH BC".to_string(),
        0xC6 => format!("ADD A,${:02X}", d8),
        0xC7 => "RST $00".to_string(),
        0xC8 => "RET Z".to_string(),
        0xC9 => "RET".to_string(),
        0xCA => format!("JP Z,${:04X}", d16),
        0xCB => unreachable!(),
        0xCC => format!("CALL Z,${:04X}", d16),
        0xCD => format!("CALL ${:04X}", d16),
        0xCE => format!("ADC A,${:02X}", d8),
        0xCF => "RST $08".to_string(),
        0xD0 => "RET NC".to_string(),
        0xD1 => "POP DE".to_string(),
        0xD2 => format!("JP NC,${:04X}", d16),
        0xD3 => "DB $D3".to_string(),
        0xD4 => format!("CALL NC,${:04X}", d16),
        0xD5 => "PUSH DE".to_string(),
        0xD6 => format!("SUB ${:02X}", d8),
        0xD7 => "RST $10".to_string(),
        0xD8 => "RET C".to_string(),
        0xD9 => "RETI".to_string(),
        0xDA => format!("JP C,${:04X}", d16),
        0xDB => "DB $DB".to_string(),
        0xDC => format!("CALL C,${:04X}", d16),
        0xDD => "DB $DD".to_string(),
        0xDE => format!("SBC A,${:02X}", d8),
        0xDF => "RST $18".to_string(),
        0xE0 => format!("LDH ($FF{:02X}),A", d8),
        0xE1 => "POP HL".to_string(),
        0xE2 => "LD (C),A".to_string(),
        0xE3 => "DB $E3".to_string(),
        0xE4 => "DB $E4".to_string(),
        0xE5 => "PUSH HL".to_string(),
        0xE6 => format!("AND ${:02X}", d8),
        0xE7 => "RST $20".to_string(),
        0xE8 => format!("ADD SP,{}", fmt_rel(r8, r8_target)),
        0xE9 => "JP HL".to_string(),
        0xEA => format!("LD (${:04X}),A", d16),
        0xEB => "DB $EB".to_string(),
        0xEC => "DB $EC".to_string(),
        0xED => "DB $ED".to_string(),
        0xEE => format!("XOR ${:02X}", d8),
        0xEF => "RST $28".to_string(),
        0xF0 => format!("LDH A,($FF{:02X})", d8),
        0xF1 => "POP AF".to_string(),
        0xF2 => "LD A,(C)".to_string(),
        0xF3 => "DI".to_string(),
        0xF4 => "DB $F4".to_string(),
        0xF5 => "PUSH AF".to_string(),
        0xF6 => format!("OR ${:02X}", d8),
        0xF7 => "RST $30".to_string(),
        0xF8 => format!("LD HL,SP+{}", fmt_signed(r8)),
        0xF9 => "LD SP,HL".to_string(),
        0xFA => format!("LD A,(${:04X})", d16),
        0xFB => "EI".to_string(),
        0xFC => "DB $FC".to_string(),
        0xFD => "DB $FD".to_string(),
        0xFE => format!("CP ${:02X}", d8),
        0xFF => "RST $38".to_string(),
    };

    DisassembledLine {
        address: addr,
        bytes,
        mnemonic,
    }
}

fn cb_mnemonic(opcode: u8) -> String {
    let register = REG8[(opcode & 0x07) as usize];
    match opcode {
        0x00..=0x3F => {
            let op = ROT_OPS[(opcode / 8) as usize];
            format!("{} {}", op, register)
        }
        0x40..=0x7F => {
            let bit = (opcode - 0x40) / 8;
            format!("BIT {},{}", bit, register)
        }
        0x80..=0xBF => {
            let bit = (opcode - 0x80) / 8;
            format!("RES {},{}", bit, register)
        }
        _ => {
            let bit = (opcode - 0xC0) / 8;
            format!("SET {},{}", bit, register)
        }
    }
}

fn read_u16(bus_read: &impl Fn(u16) -> u8, addr: u16) -> u16 {
    let lo = bus_read(addr) as u16;
    let hi = bus_read(addr.wrapping_add(1)) as u16;
    (hi << 8) | lo
}

fn fmt_signed(value: i8) -> String {
    if value < 0 {
        format!("-${:02X}", value.unsigned_abs())
    } else {
        format!("+${:02X}", value as u8)
    }
}

fn fmt_rel(offset: i8, target: u16) -> String {
    format!("{} (${:04X})", fmt_signed(offset), target)
}

