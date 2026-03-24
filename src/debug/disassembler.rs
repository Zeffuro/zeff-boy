use std::fmt::Write;

type Mnemonic = arrayvec::ArrayString<32>;

macro_rules! mn {
    ($($arg:tt)*) => {{
        let mut s = Mnemonic::new();
        let _ = write!(s, $($arg)*);
        s
    }};
}

#[derive(Clone)]
pub(crate) struct DisassembledLine {
    pub(crate) address: u16,
    pub(crate) bytes: Vec<u8>,
    pub(crate) mnemonic: Mnemonic,
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
        0x00 => mn!("NOP"),
        0x01 => mn!("LD BC,${:04X}", d16),
        0x02 => mn!("LD (BC),A"),
        0x03 => mn!("INC BC"),
        0x04 => mn!("INC B"),
        0x05 => mn!("DEC B"),
        0x06 => mn!("LD B,${:02X}", d8),
        0x07 => mn!("RLCA"),
        0x08 => mn!("LD (${:04X}),SP", d16),
        0x09 => mn!("ADD HL,BC"),
        0x0A => mn!("LD A,(BC)"),
        0x0B => mn!("DEC BC"),
        0x0C => mn!("INC C"),
        0x0D => mn!("DEC C"),
        0x0E => mn!("LD C,${:02X}", d8),
        0x0F => mn!("RRCA"),
        0x10 => mn!("STOP"),
        0x11 => mn!("LD DE,${:04X}", d16),
        0x12 => mn!("LD (DE),A"),
        0x13 => mn!("INC DE"),
        0x14 => mn!("INC D"),
        0x15 => mn!("DEC D"),
        0x16 => mn!("LD D,${:02X}", d8),
        0x17 => mn!("RLA"),
        0x18 => mn!("JR {}", fmt_rel(r8, r8_target)),
        0x19 => mn!("ADD HL,DE"),
        0x1A => mn!("LD A,(DE)"),
        0x1B => mn!("DEC DE"),
        0x1C => mn!("INC E"),
        0x1D => mn!("DEC E"),
        0x1E => mn!("LD E,${:02X}", d8),
        0x1F => mn!("RRA"),
        0x20 => mn!("JR NZ,{}", fmt_rel(r8, r8_target)),
        0x21 => mn!("LD HL,${:04X}", d16),
        0x22 => mn!("LD (HL+),A"),
        0x23 => mn!("INC HL"),
        0x24 => mn!("INC H"),
        0x25 => mn!("DEC H"),
        0x26 => mn!("LD H,${:02X}", d8),
        0x27 => mn!("DAA"),
        0x28 => mn!("JR Z,{}", fmt_rel(r8, r8_target)),
        0x29 => mn!("ADD HL,HL"),
        0x2A => mn!("LD A,(HL+)"),
        0x2B => mn!("DEC HL"),
        0x2C => mn!("INC L"),
        0x2D => mn!("DEC L"),
        0x2E => mn!("LD L,${:02X}", d8),
        0x2F => mn!("CPL"),
        0x30 => mn!("JR NC,{}", fmt_rel(r8, r8_target)),
        0x31 => mn!("LD SP,${:04X}", d16),
        0x32 => mn!("LD (HL-),A"),
        0x33 => mn!("INC SP"),
        0x34 => mn!("INC (HL)"),
        0x35 => mn!("DEC (HL)"),
        0x36 => mn!("LD (HL),${:02X}", d8),
        0x37 => mn!("SCF"),
        0x38 => mn!("JR C,{}", fmt_rel(r8, r8_target)),
        0x39 => mn!("ADD HL,SP"),
        0x3A => mn!("LD A,(HL-)"),
        0x3B => mn!("DEC SP"),
        0x3C => mn!("INC A"),
        0x3D => mn!("DEC A"),
        0x3E => mn!("LD A,${:02X}", d8),
        0x3F => mn!("CCF"),
        0x40..=0x7F => {
            if opcode == 0x76 {
                mn!("HALT")
            } else {
                let dst = REG8[((opcode >> 3) & 0x07) as usize];
                let src = REG8[(opcode & 0x07) as usize];
                mn!("LD {},{}", dst, src)
            }
        }
        0x80..=0xBF => {
            let op = ALU_OPS[((opcode >> 3) & 0x07) as usize];
            let src = REG8[(opcode & 0x07) as usize];
            if opcode < 0x90 {
                mn!("{},{}", op, src)
            } else {
                mn!("{} {}", op, src)
            }
        }
        0xC0 => mn!("RET NZ"),
        0xC1 => mn!("POP BC"),
        0xC2 => mn!("JP NZ,${:04X}", d16),
        0xC3 => mn!("JP ${:04X}", d16),
        0xC4 => mn!("CALL NZ,${:04X}", d16),
        0xC5 => mn!("PUSH BC"),
        0xC6 => mn!("ADD A,${:02X}", d8),
        0xC7 => mn!("RST $00"),
        0xC8 => mn!("RET Z"),
        0xC9 => mn!("RET"),
        0xCA => mn!("JP Z,${:04X}", d16),
        0xCB => unreachable!(),
        0xCC => mn!("CALL Z,${:04X}", d16),
        0xCD => mn!("CALL ${:04X}", d16),
        0xCE => mn!("ADC A,${:02X}", d8),
        0xCF => mn!("RST $08"),
        0xD0 => mn!("RET NC"),
        0xD1 => mn!("POP DE"),
        0xD2 => mn!("JP NC,${:04X}", d16),
        0xD3 => mn!("DB $D3"),
        0xD4 => mn!("CALL NC,${:04X}", d16),
        0xD5 => mn!("PUSH DE"),
        0xD6 => mn!("SUB ${:02X}", d8),
        0xD7 => mn!("RST $10"),
        0xD8 => mn!("RET C"),
        0xD9 => mn!("RETI"),
        0xDA => mn!("JP C,${:04X}", d16),
        0xDB => mn!("DB $DB"),
        0xDC => mn!("CALL C,${:04X}", d16),
        0xDD => mn!("DB $DD"),
        0xDE => mn!("SBC A,${:02X}", d8),
        0xDF => mn!("RST $18"),
        0xE0 => mn!("LDH ($FF{:02X}),A", d8),
        0xE1 => mn!("POP HL"),
        0xE2 => mn!("LD (C),A"),
        0xE3 => mn!("DB $E3"),
        0xE4 => mn!("DB $E4"),
        0xE5 => mn!("PUSH HL"),
        0xE6 => mn!("AND ${:02X}", d8),
        0xE7 => mn!("RST $20"),
        0xE8 => mn!("ADD SP,{}", fmt_rel(r8, r8_target)),
        0xE9 => mn!("JP HL"),
        0xEA => mn!("LD (${:04X}),A", d16),
        0xEB => mn!("DB $EB"),
        0xEC => mn!("DB $EC"),
        0xED => mn!("DB $ED"),
        0xEE => mn!("XOR ${:02X}", d8),
        0xEF => mn!("RST $28"),
        0xF0 => mn!("LDH A,($FF{:02X})", d8),
        0xF1 => mn!("POP AF"),
        0xF2 => mn!("LD A,(C)"),
        0xF3 => mn!("DI"),
        0xF4 => mn!("DB $F4"),
        0xF5 => mn!("PUSH AF"),
        0xF6 => mn!("OR ${:02X}", d8),
        0xF7 => mn!("RST $30"),
        0xF8 => mn!("LD HL,SP{}", fmt_signed(r8)),
        0xF9 => mn!("LD SP,HL"),
        0xFA => mn!("LD A,(${:04X})", d16),
        0xFB => mn!("EI"),
        0xFC => mn!("DB $FC"),
        0xFD => mn!("DB $FD"),
        0xFE => mn!("CP ${:02X}", d8),
        0xFF => mn!("RST $38"),
    };

    DisassembledLine {
        address: addr,
        bytes,
        mnemonic,
    }
}

fn cb_mnemonic(opcode: u8) -> Mnemonic {
    let register = REG8[(opcode & 0x07) as usize];
    match opcode {
        0x00..=0x3F => {
            let op = ROT_OPS[(opcode / 8) as usize];
            mn!("{} {}", op, register)
        }
        0x40..=0x7F => {
            let bit = (opcode - 0x40) / 8;
            mn!("BIT {},{}", bit, register)
        }
        0x80..=0xBF => {
            let bit = (opcode - 0x80) / 8;
            mn!("RES {},{}", bit, register)
        }
        _ => {
            let bit = (opcode - 0xC0) / 8;
            mn!("SET {},{}", bit, register)
        }
    }
}

fn read_u16(bus_read: &impl Fn(u16) -> u8, addr: u16) -> u16 {
    let lo = bus_read(addr) as u16;
    let hi = bus_read(addr.wrapping_add(1)) as u16;
    (hi << 8) | lo
}

fn fmt_signed(value: i8) -> Mnemonic {
    if value < 0 {
        mn!("-${:02X}", value.unsigned_abs())
    } else {
        mn!("+${:02X}", value as u8)
    }
}

fn fmt_rel(offset: i8, target: u16) -> Mnemonic {
    mn!("{} (${:04X})", fmt_signed(offset), target)
}
