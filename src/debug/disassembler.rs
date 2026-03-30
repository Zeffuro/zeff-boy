pub(crate) type Mnemonic = arrayvec::ArrayString<32>;

macro_rules! mn {
    ($($arg:tt)*) => {{
        let mut s = Mnemonic::new();
        let _ = std::fmt::Write::write_fmt(&mut s, format_args!($($arg)*));
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

mod gb;
mod nes;

pub(crate) use gb::disassemble_around;
pub(crate) use nes::disassemble_around as nes_disassemble_around;

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

fn choose_centered_start(
    inst_len: impl Fn(u16) -> usize,
    pc: u16,
    lines_before_pc: usize,
) -> u16 {
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
