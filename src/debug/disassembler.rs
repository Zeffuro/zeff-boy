pub(crate) type Mnemonic = arrayvec::ArrayString<32>;
pub(crate) type InstructionBytes = arrayvec::ArrayVec<u8, 16>;
use zeff_emu_common::address::Address;

macro_rules! mn {
    ($($arg:tt)*) => {{
        let mut s = Mnemonic::new();
        let _ = std::fmt::Write::write_fmt(&mut s, format_args!($($arg)*));
        s
    }};
}

#[derive(Clone)]
pub(crate) struct DisassembledLine {
    pub(crate) address: Address,
    pub(crate) storage_offset: Option<u64>,
    pub(crate) bank: Option<u32>,
    pub(crate) symbol: Option<String>,
    pub(crate) control_target: Option<Address>,
    pub(crate) control_target_storage: Option<u64>,
    pub(crate) control_target_bank: Option<u32>,
    pub(crate) control_target_symbol: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) bytes: InstructionBytes,
    pub(crate) mnemonic: Mnemonic,
}

#[derive(Clone)]
pub(crate) struct DisassemblyView {
    pub(crate) pc: Address,
    pub(crate) mapping: Option<u64>,
    pub(crate) is_navigation_target: bool,
    pub(crate) is_static_target: bool,
    pub(crate) location_symbol: Option<String>,
    pub(crate) lines: Vec<DisassembledLine>,
    pub(crate) breakpoints: Vec<Address>,
    pub(crate) one_shot_breakpoints: Vec<Address>,
    pub(crate) rom_breakpoints: Vec<u64>,
    pub(crate) hit_rom_breakpoint: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DisassemblyTarget {
    pub(crate) cpu_address: Address,
    pub(crate) storage_offset: Option<u64>,
    pub(crate) bank: Option<u32>,
    pub(crate) thumb: Option<bool>,
}

mod gb;
mod gba;
mod huc6280;
mod nes;
mod v30;
mod z80;

pub(crate) fn disassemble_around(
    bus_read: impl Fn(u16) -> u8,
    pc: u16,
    lines_before_pc: usize,
    total_lines: usize,
) -> Vec<DisassembledLine> {
    disassemble_around_with(
        |addr| gb::instruction_len(&bus_read, addr),
        |addr| gb::decode_instruction(&bus_read, addr),
        pc,
        lines_before_pc,
        total_lines,
    )
}

pub(crate) fn nes_disassemble_around(
    bus_read: impl Fn(u16) -> u8,
    pc: u16,
    lines_before_pc: usize,
    total_lines: usize,
) -> Vec<DisassembledLine> {
    disassemble_around_with(
        |addr| nes::instruction_len(&bus_read, addr),
        |addr| nes::decode_instruction(&bus_read, addr),
        pc,
        lines_before_pc,
        total_lines,
    )
}

pub(crate) fn huc6280_disassemble_around(
    bus_read: impl Fn(u16) -> u8,
    pc: u16,
    lines_before_pc: usize,
    total_lines: usize,
) -> Vec<DisassembledLine> {
    disassemble_around_with(
        |addr| huc6280::instruction_len(&bus_read, addr),
        |addr| huc6280::decode_instruction(&bus_read, addr),
        pc,
        lines_before_pc,
        total_lines,
    )
}

pub(crate) fn z80_disassemble_around(
    bus_read: impl Fn(u16) -> u8,
    pc: u16,
    lines_before_pc: usize,
    total_lines: usize,
) -> Vec<DisassembledLine> {
    disassemble_around_with(
        |addr| z80::instruction_len(&bus_read, addr),
        |addr| z80::decode_instruction(&bus_read, addr),
        pc,
        lines_before_pc,
        total_lines,
    )
}

pub(crate) fn gba_disassemble_around(
    bus_read: impl Fn(u32) -> u8,
    pc: u32,
    thumb: bool,
    lines_before_pc: usize,
    total_lines: usize,
) -> Vec<DisassembledLine> {
    gba::disassemble_around(&bus_read, pc, thumb, lines_before_pc, total_lines)
}

pub(crate) fn v30_disassemble_around(
    bus_read: impl Fn(u32) -> u8,
    pc: u32,
    lines_before_pc: usize,
    total_lines: usize,
) -> Vec<DisassembledLine> {
    v30::disassemble_around(&bus_read, pc, lines_before_pc, total_lines)
}

fn disassemble_around_with(
    inst_len: impl Fn(u16) -> usize,
    decode: impl Fn(u16) -> DisassembledLine,
    pc: u16,
    lines_before_pc: usize,
    total_lines: usize,
) -> Vec<DisassembledLine> {
    let start = choose_centered_start(inst_len, pc, lines_before_pc);
    disassemble_at(decode, start, total_lines)
}

fn disassemble_at(
    decode: impl Fn(u16) -> DisassembledLine,
    start: u16,
    count: usize,
) -> Vec<DisassembledLine> {
    let mut lines = Vec::with_capacity(count);
    let mut addr = start;
    for _ in 0..count {
        let line = decode(addr);
        let len = line.bytes.len().max(1) as u16;
        addr = addr.wrapping_add(len);
        lines.push(line);
    }
    lines
}

fn choose_centered_start(inst_len: impl Fn(u16) -> usize, pc: u16, lines_before_pc: usize) -> u16 {
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
            addr = addr.wrapping_add(inst_len(addr) as u16);
            steps += 1;
        }
    }

    best_start
}

fn fmt_signed(value: i8) -> Mnemonic {
    if value < 0 {
        mn!("-${:02X}", value.unsigned_abs())
    } else {
        mn!("+${:02X}", value as u8)
    }
}
