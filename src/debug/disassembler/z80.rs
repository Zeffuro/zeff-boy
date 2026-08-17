use super::{DisassembledLine, InstructionBytes, Mnemonic, fmt_signed};

const REG8: [&str; 8] = ["B", "C", "D", "E", "H", "L", "(HL)", "A"];
const REG16: [&str; 4] = ["BC", "DE", "HL", "SP"];
const REG16_AF: [&str; 4] = ["BC", "DE", "HL", "AF"];
const CONDITIONS: [&str; 8] = ["NZ", "Z", "NC", "C", "PO", "PE", "P", "M"];
const ROT_OPS: [&str; 8] = ["RLC", "RRC", "RL", "RR", "SLA", "SRA", "SLL", "SRL"];
const BLOCK_OPS: [(u8, &str); 16] = [
    (0xA0, "LDI"),
    (0xA1, "CPI"),
    (0xA2, "INI"),
    (0xA3, "OUTI"),
    (0xA8, "LDD"),
    (0xA9, "CPD"),
    (0xAA, "IND"),
    (0xAB, "OUTD"),
    (0xB0, "LDIR"),
    (0xB1, "CPIR"),
    (0xB2, "INIR"),
    (0xB3, "OTIR"),
    (0xB8, "LDDR"),
    (0xB9, "CPDR"),
    (0xBA, "INDR"),
    (0xBB, "OTDR"),
];

#[derive(Clone, Copy)]
struct RegisterNames {
    reg8: [&'static str; 8],
    reg16: [&'static str; 4],
    reg16_af: [&'static str; 4],
    hl_name: &'static str,
}

const BASE_REGISTER_NAMES: RegisterNames = RegisterNames {
    reg8: REG8,
    reg16: REG16,
    reg16_af: REG16_AF,
    hl_name: "HL",
};

pub(super) fn instruction_len(bus_read: &impl Fn(u16) -> u8, addr: u16) -> usize {
    match bus_read(addr) {
        0xCB => 2,
        0xED => ed_instruction_len(bus_read(addr.wrapping_add(1))),
        0xDD | 0xFD => indexed_instruction_len(bus_read, addr),
        opcode => base_instruction_len(opcode),
    }
}

pub(super) fn decode_instruction(bus_read: &impl Fn(u16) -> u8, addr: u16) -> DisassembledLine {
    let opcode = bus_read(addr);
    match opcode {
        0xCB => decode_prefixed(
            addr,
            instruction_bytes(bus_read, addr, 2),
            cb_mnemonic(bus_read(addr.wrapping_add(1)), REG8),
        ),
        0xED => decode_prefixed(
            addr,
            instruction_bytes(
                bus_read,
                addr,
                ed_instruction_len(bus_read(addr.wrapping_add(1))),
            ),
            ed_mnemonic(bus_read, addr),
        ),
        0xDD => decode_indexed(bus_read, addr, "IX", "IXH", "IXL"),
        0xFD => decode_indexed(bus_read, addr, "IY", "IYH", "IYL"),
        _ => decode_base(bus_read, addr),
    }
}

fn decode_prefixed(addr: u16, bytes: InstructionBytes, mnemonic: Mnemonic) -> DisassembledLine {
    DisassembledLine {
        address: addr.into(),
        storage_offset: None,
        symbol: None,
        control_target: None,
        control_target_storage: None,
        control_target_symbol: None,
        source: None,
        bytes,
        mnemonic,
    }
}

fn decode_base(bus_read: &impl Fn(u16) -> u8, addr: u16) -> DisassembledLine {
    let opcode = bus_read(addr);
    let len = base_instruction_len(opcode);
    let bytes = instruction_bytes(bus_read, addr, len);
    let mnemonic = base_mnemonic(bus_read, addr, opcode, BASE_REGISTER_NAMES);
    DisassembledLine {
        address: addr.into(),
        storage_offset: None,
        symbol: None,
        control_target: None,
        control_target_storage: None,
        control_target_symbol: None,
        source: None,
        bytes,
        mnemonic,
    }
}

fn decode_indexed(
    bus_read: &impl Fn(u16) -> u8,
    addr: u16,
    index: &'static str,
    index_hi: &'static str,
    index_lo: &'static str,
) -> DisassembledLine {
    let opcode = bus_read(addr.wrapping_add(1));
    let len = indexed_instruction_len(bus_read, addr);
    let bytes = instruction_bytes(bus_read, addr, len);
    let index_registers = RegisterNames {
        reg8: ["B", "C", "D", "E", index_hi, index_lo, "(HL)", "A"],
        reg16: ["BC", "DE", index, "SP"],
        reg16_af: ["BC", "DE", index, "AF"],
        hl_name: index,
    };

    let mnemonic = if opcode == 0xCB {
        let displacement = bus_read(addr.wrapping_add(2)) as i8;
        let cb = bus_read(addr.wrapping_add(3));
        indexed_cb_mnemonic(index, displacement, cb)
    } else if indexed_uses_displacement(opcode) {
        indexed_displacement_mnemonic(bus_read, addr, index, opcode)
    } else {
        base_mnemonic(bus_read, addr.wrapping_add(1), opcode, index_registers)
    };

    DisassembledLine {
        address: addr.into(),
        storage_offset: None,
        symbol: None,
        control_target: None,
        control_target_storage: None,
        control_target_symbol: None,
        source: None,
        bytes,
        mnemonic,
    }
}

fn base_instruction_len(opcode: u8) -> usize {
    let x = opcode >> 6;
    let y = (opcode >> 3) & 0x07;
    let z = opcode & 0x07;
    let q = y & 0x01;

    match x {
        0 => match z {
            0 => {
                if y >= 2 {
                    2
                } else {
                    1
                }
            }
            1 => {
                if q == 0 {
                    3
                } else {
                    1
                }
            }
            2 => {
                if y >= 4 {
                    3
                } else {
                    1
                }
            }
            6 => 2,
            _ => 1,
        },
        1 | 2 => 1,
        _ => match z {
            2 | 4 => 3,
            3 => match y {
                0 => 3,
                2 | 3 => 2,
                _ => 1,
            },
            5 => {
                if q == 1 && y == 0 {
                    3
                } else {
                    1
                }
            }
            6 => 2,
            _ => 1,
        },
    }
}

fn ed_instruction_len(opcode: u8) -> usize {
    match opcode {
        0x43 | 0x4B | 0x53 | 0x5B | 0x63 | 0x6B | 0x73 | 0x7B => 4,
        _ => 2,
    }
}

fn indexed_instruction_len(bus_read: &impl Fn(u16) -> u8, addr: u16) -> usize {
    let opcode = bus_read(addr.wrapping_add(1));
    if opcode == 0xCB {
        return 4;
    }
    if indexed_uses_displacement(opcode) {
        if opcode == 0x36 { 4 } else { 3 }
    } else {
        1 + base_instruction_len(opcode)
    }
}

fn indexed_uses_displacement(opcode: u8) -> bool {
    matches!(
        opcode,
        0x34 | 0x35 | 0x36 | 0x46 | 0x4E | 0x56 | 0x5E | 0x66 | 0x6E | 0x70
            ..=0x75 | 0x77 | 0x7E | 0x86 | 0x8E | 0x96 | 0x9E | 0xA6 | 0xAE | 0xB6 | 0xBE
    )
}

fn base_mnemonic(
    bus_read: &impl Fn(u16) -> u8,
    addr: u16,
    opcode: u8,
    registers: RegisterNames,
) -> Mnemonic {
    let RegisterNames {
        reg8,
        reg16,
        reg16_af,
        hl_name,
    } = registers;
    let d8 = bus_read(addr.wrapping_add(1));
    let d16 = read_u16(bus_read, addr.wrapping_add(1));
    let rel = d8 as i8;
    let rel_target = addr.wrapping_add(2).wrapping_add_signed(rel as i16);
    let x = opcode >> 6;
    let y = (opcode >> 3) & 0x07;
    let z = opcode & 0x07;
    let p = y >> 1;
    let q = y & 0x01;

    match x {
        0 => match z {
            0 => match y {
                0 => mn!("NOP"),
                1 => mn!("EX AF,AF'"),
                2 => mn!("DJNZ {}", fmt_rel(rel, rel_target)),
                3 => mn!("JR {}", fmt_rel(rel, rel_target)),
                4..=7 => mn!(
                    "JR {},{}",
                    CONDITIONS[(y - 4) as usize],
                    fmt_rel(rel, rel_target)
                ),
                _ => unreachable!(),
            },
            1 => {
                if q == 0 {
                    mn!("LD {},${:04X}", reg16[p as usize], d16)
                } else {
                    mn!("ADD {},{}", hl_name, reg16[p as usize])
                }
            }
            2 => match (q, p) {
                (0, 0) => mn!("LD (BC),A"),
                (1, 0) => mn!("LD A,(BC)"),
                (0, 1) => mn!("LD (DE),A"),
                (1, 1) => mn!("LD A,(DE)"),
                (0, 2) => mn!("LD (${:04X}),{}", d16, hl_name),
                (1, 2) => mn!("LD {},(${:04X})", hl_name, d16),
                (0, 3) => mn!("LD (${:04X}),A", d16),
                (1, 3) => mn!("LD A,(${:04X})", d16),
                _ => unreachable!(),
            },
            3 => {
                if q == 0 {
                    mn!("INC {}", reg16[p as usize])
                } else {
                    mn!("DEC {}", reg16[p as usize])
                }
            }
            4 => mn!("INC {}", reg8[y as usize]),
            5 => mn!("DEC {}", reg8[y as usize]),
            6 => mn!("LD {},${:02X}", reg8[y as usize], d8),
            7 => match y {
                0 => mn!("RLCA"),
                1 => mn!("RRCA"),
                2 => mn!("RLA"),
                3 => mn!("RRA"),
                4 => mn!("DAA"),
                5 => mn!("CPL"),
                6 => mn!("SCF"),
                7 => mn!("CCF"),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        },
        1 => {
            if opcode == 0x76 {
                mn!("HALT")
            } else {
                mn!("LD {},{}", reg8[y as usize], reg8[z as usize])
            }
        }
        2 => alu_mnemonic(y, reg8[z as usize]),
        3 => match z {
            0 => mn!("RET {}", CONDITIONS[y as usize]),
            1 => {
                if q == 0 {
                    mn!("POP {}", reg16_af[p as usize])
                } else {
                    match p {
                        0 => mn!("RET"),
                        1 => mn!("EXX"),
                        2 => mn!("JP ({})", hl_name),
                        3 => mn!("LD SP,{}", hl_name),
                        _ => unreachable!(),
                    }
                }
            }
            2 => mn!("JP {},${:04X}", CONDITIONS[y as usize], d16),
            3 => match y {
                0 => mn!("JP ${:04X}", d16),
                1 => mn!("PREFIX CB"),
                2 => mn!("OUT (${:02X}),A", d8),
                3 => mn!("IN A,(${:02X})", d8),
                4 => mn!("EX (SP),{}", hl_name),
                5 => mn!("EX DE,HL"),
                6 => mn!("DI"),
                7 => mn!("EI"),
                _ => unreachable!(),
            },
            4 => mn!("CALL {},${:04X}", CONDITIONS[y as usize], d16),
            5 => {
                if q == 0 {
                    mn!("PUSH {}", reg16_af[p as usize])
                } else {
                    match p {
                        0 => mn!("CALL ${:04X}", d16),
                        1 => mn!("PREFIX DD"),
                        2 => mn!("PREFIX ED"),
                        3 => mn!("PREFIX FD"),
                        _ => unreachable!(),
                    }
                }
            }
            6 => alu_mnemonic_immediate(y, d8),
            7 => mn!("RST ${:02X}", y * 8),
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }
}

fn indexed_displacement_mnemonic(
    bus_read: &impl Fn(u16) -> u8,
    addr: u16,
    index: &'static str,
    opcode: u8,
) -> Mnemonic {
    let displacement = bus_read(addr.wrapping_add(2)) as i8;
    let operand = indexed_operand(index, displacement);
    let imm = bus_read(addr.wrapping_add(3));
    let y = (opcode >> 3) & 0x07;
    let z = opcode & 0x07;

    match opcode {
        0x34 => mn!("INC {}", operand),
        0x35 => mn!("DEC {}", operand),
        0x36 => mn!("LD {},${:02X}", operand, imm),
        0x46 | 0x4E | 0x56 | 0x5E | 0x66 | 0x6E | 0x7E => {
            mn!("LD {},{}", REG8[y as usize], operand)
        }
        0x70..=0x75 | 0x77 => mn!("LD {},{}", operand, REG8[z as usize]),
        0x86 => mn!("ADD A,{}", operand),
        0x8E => mn!("ADC A,{}", operand),
        0x96 => mn!("SUB {}", operand),
        0x9E => mn!("SBC A,{}", operand),
        0xA6 => mn!("AND {}", operand),
        0xAE => mn!("XOR {}", operand),
        0xB6 => mn!("OR {}", operand),
        0xBE => mn!("CP {}", operand),
        _ => mn!("DB ${:02X}", opcode),
    }
}

fn cb_mnemonic(opcode: u8, reg8: [&'static str; 8]) -> Mnemonic {
    let register = reg8[(opcode & 0x07) as usize];
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

fn indexed_cb_mnemonic(index: &'static str, displacement: i8, opcode: u8) -> Mnemonic {
    let operand = indexed_operand(index, displacement);
    let register = REG8[(opcode & 0x07) as usize];
    match opcode {
        0x00..=0x3F => {
            let op = ROT_OPS[(opcode / 8) as usize];
            if opcode & 0x07 == 6 {
                mn!("{} {}", op, operand)
            } else {
                mn!("{} {},{}", op, operand, register)
            }
        }
        0x40..=0x7F => {
            let bit = (opcode - 0x40) / 8;
            mn!("BIT {},{}", bit, operand)
        }
        0x80..=0xBF => {
            let bit = (opcode - 0x80) / 8;
            if opcode & 0x07 == 6 {
                mn!("RES {},{}", bit, operand)
            } else {
                mn!("RES {},{},{}", bit, operand, register)
            }
        }
        _ => {
            let bit = (opcode - 0xC0) / 8;
            if opcode & 0x07 == 6 {
                mn!("SET {},{}", bit, operand)
            } else {
                mn!("SET {},{},{}", bit, operand, register)
            }
        }
    }
}

fn ed_mnemonic(bus_read: &impl Fn(u16) -> u8, addr: u16) -> Mnemonic {
    let opcode = bus_read(addr.wrapping_add(1));
    if let Some((_, name)) = BLOCK_OPS.iter().find(|(op, _)| *op == opcode) {
        return mn!("{}", name);
    }

    let x = opcode >> 6;
    let y = (opcode >> 3) & 0x07;
    let z = opcode & 0x07;
    let p = y >> 1;
    let q = y & 0x01;
    let d16 = read_u16(bus_read, addr.wrapping_add(2));

    match (x, z) {
        (1, 0) => {
            if y == 6 {
                mn!("IN (C)")
            } else {
                mn!("IN {},(C)", REG8[y as usize])
            }
        }
        (1, 1) => {
            if y == 6 {
                mn!("OUT (C),0")
            } else {
                mn!("OUT (C),{}", REG8[y as usize])
            }
        }
        (1, 2) => {
            if q == 0 {
                mn!("SBC HL,{}", REG16[p as usize])
            } else {
                mn!("ADC HL,{}", REG16[p as usize])
            }
        }
        (1, 3) => {
            if q == 0 {
                mn!("LD (${:04X}),{}", d16, REG16[p as usize])
            } else {
                mn!("LD {},(${:04X})", REG16[p as usize], d16)
            }
        }
        (1, 4) => mn!("NEG"),
        (1, 5) => match y {
            1 => mn!("RETI"),
            _ => mn!("RETN"),
        },
        (1, 6) => match y {
            0 | 1 | 4 | 5 => mn!("IM 0"),
            2 | 6 => mn!("IM 1"),
            3 | 7 => mn!("IM 2"),
            _ => unreachable!(),
        },
        (1, 7) => match y {
            0 => mn!("LD I,A"),
            1 => mn!("LD R,A"),
            2 => mn!("LD A,I"),
            3 => mn!("LD A,R"),
            4 => mn!("RRD"),
            5 => mn!("RLD"),
            _ => mn!("NOP"),
        },
        _ => mn!("DB $ED,${:02X}", opcode),
    }
}

fn alu_mnemonic(op: u8, operand: &str) -> Mnemonic {
    match op {
        0 => mn!("ADD A,{}", operand),
        1 => mn!("ADC A,{}", operand),
        2 => mn!("SUB {}", operand),
        3 => mn!("SBC A,{}", operand),
        4 => mn!("AND {}", operand),
        5 => mn!("XOR {}", operand),
        6 => mn!("OR {}", operand),
        7 => mn!("CP {}", operand),
        _ => unreachable!(),
    }
}

fn alu_mnemonic_immediate(op: u8, value: u8) -> Mnemonic {
    match op {
        0 => mn!("ADD A,${:02X}", value),
        1 => mn!("ADC A,${:02X}", value),
        2 => mn!("SUB ${:02X}", value),
        3 => mn!("SBC A,${:02X}", value),
        4 => mn!("AND ${:02X}", value),
        5 => mn!("XOR ${:02X}", value),
        6 => mn!("OR ${:02X}", value),
        7 => mn!("CP ${:02X}", value),
        _ => unreachable!(),
    }
}

fn indexed_operand(index: &'static str, displacement: i8) -> Mnemonic {
    if displacement < 0 {
        mn!("({}-${:02X})", index, displacement.unsigned_abs())
    } else {
        mn!("({}+${:02X})", index, displacement as u8)
    }
}

fn fmt_rel(displacement: i8, target: u16) -> Mnemonic {
    mn!("{} -> ${:04X}", fmt_signed(displacement), target)
}

fn read_u16(bus_read: &impl Fn(u16) -> u8, addr: u16) -> u16 {
    u16::from_le_bytes([bus_read(addr), bus_read(addr.wrapping_add(1))])
}

fn instruction_bytes(bus_read: &impl Fn(u16) -> u8, addr: u16, len: usize) -> InstructionBytes {
    (0..len.min(4))
        .map(|i| bus_read(addr.wrapping_add(i as u16)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(bytes: &[u8], addr: u16) -> u8 {
        bytes.get(addr as usize).copied().unwrap_or(0)
    }

    #[test]
    fn decodes_common_sms_io_setup_sequence() {
        let bytes = [0x3E, 0x90, 0xD3, 0x7F, 0x76];

        let ld = decode_instruction(&|addr| read(&bytes, addr), 0);
        let out = decode_instruction(&|addr| read(&bytes, addr), 2);
        let halt = decode_instruction(&|addr| read(&bytes, addr), 4);

        assert_eq!(ld.bytes.as_slice(), &[0x3E, 0x90]);
        assert_eq!(ld.mnemonic.as_str(), "LD A,$90");
        assert_eq!(out.bytes.as_slice(), &[0xD3, 0x7F]);
        assert_eq!(out.mnemonic.as_str(), "OUT ($7F),A");
        assert_eq!(halt.mnemonic.as_str(), "HALT");
    }

    #[test]
    fn decodes_indexed_displacement_and_cb_forms() {
        let indexed_ld = [0xDD, 0x36, 0xFE, 0x5A];
        let indexed_cb = [0xFD, 0xCB, 0x02, 0x46];

        let ld = decode_instruction(&|addr| read(&indexed_ld, addr), 0);
        let bit = decode_instruction(&|addr| read(&indexed_cb, addr), 0);

        assert_eq!(instruction_len(&|addr| read(&indexed_ld, addr), 0), 4);
        assert_eq!(ld.bytes.as_slice(), &[0xDD, 0x36, 0xFE, 0x5A]);
        assert_eq!(ld.mnemonic.as_str(), "LD (IX-$02),$5A");
        assert_eq!(instruction_len(&|addr| read(&indexed_cb, addr), 0), 4);
        assert_eq!(bit.mnemonic.as_str(), "BIT 0,(IY+$02)");
    }

    #[test]
    fn decodes_ed_memory_and_block_forms() {
        let ld = [0xED, 0x43, 0x34, 0x12];
        let ldir = [0xED, 0xB0];

        let ld_line = decode_instruction(&|addr| read(&ld, addr), 0);
        let ldir_line = decode_instruction(&|addr| read(&ldir, addr), 0);

        assert_eq!(instruction_len(&|addr| read(&ld, addr), 0), 4);
        assert_eq!(ld_line.mnemonic.as_str(), "LD ($1234),BC");
        assert_eq!(instruction_len(&|addr| read(&ldir, addr), 0), 2);
        assert_eq!(ldir_line.mnemonic.as_str(), "LDIR");
    }
}
