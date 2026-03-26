pub(crate) type Mnemonic = arrayvec::ArrayString<32>;

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
