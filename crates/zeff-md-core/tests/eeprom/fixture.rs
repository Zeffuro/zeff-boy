use zeff_md_core::Emulator;

pub(super) const PORT: u32 = 0x200001;
const RESULTS: u32 = 0xff2000;
const DONE: u32 = 0xff1000;
pub(super) const DECLARATION: &[u8; 12] = b"RA\xe8\x40\x00\x20\x00\x01\x00\x20\x00\x01";

pub(super) struct Rom {
    pub(super) bytes: Vec<u8>,
    pub(super) pc: usize,
    pub(super) results: usize,
    pub(super) cuts: Vec<u32>,
    pub(super) word_port: bool,
}

impl Rom {
    pub(super) fn new() -> Self {
        let mut bytes = vec![0; 0x10000];
        bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
        bytes[0x180..0x18e].copy_from_slice(b"GM T-12046 -00");
        bytes[0x1b0..0x1bc].copy_from_slice(DECLARATION);
        Self {
            bytes,
            pc: 0x200,
            results: 0,
            cuts: Vec::new(),
            word_port: false,
        }
    }

    pub(super) fn word(&mut self, value: u16) {
        self.bytes[self.pc..self.pc + 2].copy_from_slice(&value.to_be_bytes());
        self.pc += 2;
    }

    pub(super) fn long(&mut self, value: u32) {
        self.word((value >> 16) as u16);
        self.word(value as u16);
    }

    pub(super) fn write(&mut self, opcode: u16, address: u32, value: u16) {
        self.word(opcode);
        self.word(value);
        self.long(address);
    }

    pub(super) fn sample(&mut self) -> usize {
        let index = self.results;
        self.word(if self.word_port { 0x3039 } else { 0x1039 });
        self.long(if self.word_port { 0x200000 } else { PORT });
        self.word(0x13c0);
        self.long(RESULTS + index as u32);
        self.results += 1;
        index
    }

    pub(super) fn finish(mut self) -> (Vec<u8>, Vec<u32>) {
        self.write(0x13fc, DONE, 0x7a);
        self.word(0x60fe);
        (self.bytes, self.cuts)
    }
}

pub(super) fn advance(machine: &mut Emulator, pc: Option<u32>) {
    for _ in 0..100_000 {
        if pc.is_some_and(|pc| machine.registers().pc == pc)
            || (pc.is_none() && machine.peek(DONE) == Some(0x7a))
        {
            return;
        }
        machine.step_instruction().unwrap();
    }
    panic!("EEPROM guest did not reach checkpoint");
}

pub(super) fn sample(machine: &Emulator, index: usize) -> u8 {
    machine.peek(RESULTS + index as u32).unwrap()
}
