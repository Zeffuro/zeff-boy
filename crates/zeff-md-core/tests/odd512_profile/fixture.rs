use zeff_md_core::{Emulator, LoadConfig, Region};

pub const PRODUCTS: [&[u8; 14]; 3] = [b"GM 00005518-00", b"GM 00005518-03", b"GM 00005532-00"];
const DECLARATION: [u8; 12] = [b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20, 3, 0xff];
pub const TABLE: u32 = 0x1000;

pub struct Rom {
    pub bytes: Vec<u8>,
    pub pc: usize,
}

impl Rom {
    pub fn new(product: &[u8; 14]) -> Self {
        let mut bytes = vec![0; 0x100000];
        bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
        bytes[0x180..0x18e].copy_from_slice(product);
        bytes[0x1b0..0x1bc].copy_from_slice(&DECLARATION);
        Self { bytes, pc: 0x200 }
    }

    pub fn word(&mut self, value: u16) {
        self.bytes[self.pc..self.pc + 2].copy_from_slice(&value.to_be_bytes());
        self.pc += 2;
    }

    pub fn long(&mut self, value: u32) {
        self.word((value >> 16) as u16);
        self.word(value as u16);
    }

    pub fn byte(&mut self, address: u32, value: u8) {
        self.word(0x13fc);
        self.word(u16::from(value));
        self.long(address);
    }

    pub fn write_word(&mut self, address: u32, value: u16) {
        self.word(0x33fc);
        self.word(value);
        self.long(address);
    }

    fn jump(&mut self, opcode: u16) -> usize {
        let at = self.pc;
        self.word(opcode);
        self.word(0);
        at
    }

    fn target(&mut self, at: usize, target: usize) {
        let offset = i16::try_from(target as isize - (at + 2) as isize).unwrap();
        self.bytes[at + 2..at + 4].copy_from_slice(&offset.to_be_bytes());
    }

    fn dbra(&mut self, register: u16, target: usize) {
        let at = self.jump(0x51c8 | register);
        self.target(at, target);
    }

    fn call(&mut self) -> usize {
        self.word(0x4eb9);
        let at = self.pc;
        self.long(0);
        at
    }

    pub fn finish(mut self) -> Vec<u8> {
        self.byte(0xff1000, 0x7a);
        self.word(0x60fe);
        self.bytes
    }
}

pub fn config(region: Region, concurrent_sound: bool) -> LoadConfig {
    LoadConfig {
        region,
        concurrent_sound,
    }
}

pub fn until(machine: &mut Emulator, pc: usize) {
    for _ in 0..20_000 {
        if machine.registers().pc == pc as u32 {
            return;
        }
        machine.step_instruction().unwrap();
    }
    panic!(
        "did not reach {pc:06x}, actual={:06x}",
        machine.registers().pc
    );
}

pub fn pattern() -> Vec<u8> {
    (0..512)
        .map(|i| ((i * 37) as u8) ^ (i >> 3) as u8)
        .collect()
}

pub fn checksum(cells: &[u8]) -> u8 {
    cells[3..511]
        .iter()
        .fold(0u8, |sum, value| sum.wrapping_add(*value))
}

pub struct Program {
    pub bytes: Vec<u8>,
    pub initialized: Vec<u8>,
    pub clear_end: usize,
    pub copy_end: usize,
    pub done: usize,
}

pub fn initialization_program(product: &[u8; 14]) -> Program {
    let mut rom = Rom::new(product);
    let source = pattern();
    rom.bytes[TABLE as usize..TABLE as usize + 512].copy_from_slice(&source);
    rom.word(0x0c39);
    rom.word(0x12);
    rom.long(0x200003);
    let marker_bad = rom.jump(0x6600);
    let sum_existing = rom.call();
    rom.word(0xb039);
    rom.long(0x200005);
    let sum_bad = rom.jump(0x6600);
    rom.word(0x7a02);
    let reused = rom.jump(0x6000);
    let init = rom.pc;
    rom.target(marker_bad, init);
    rom.target(sum_bad, init);
    rom.word(0x7a01);
    rom.word(0x45f9);
    rom.long(0x200001);
    rom.word(0x7200);
    rom.word(0x203c);
    rom.long(511);
    let clear = rom.pc;
    for word in [0x14c1, 0xd4fc, 1] {
        rom.word(word);
    }
    rom.dbra(0, clear);
    let clear_end = rom.pc;
    rom.byte(0x200003, 0x12);
    rom.word(0x4df9);
    rom.long(0x20000b);
    rom.word(0x4bf9);
    rom.long(TABLE);
    rom.word(0x7e0f);
    let group = rom.pc;
    rom.word(0x7c19);
    let copy = rom.pc;
    for word in [0x1cdd, 0xdcfc, 1] {
        rom.word(word);
    }
    rom.dbra(6, copy);
    rom.word(0xdafc);
    rom.word(6);
    rom.dbra(7, group);
    let copy_end = rom.pc;
    let sum_new = rom.call();
    rom.word(0x13c0);
    rom.long(0x200005);
    let done = rom.pc;
    rom.target(reused, done);
    rom.word(0x13c5);
    rom.long(0xff1000);
    rom.word(0x60fe);
    let sum = rom.pc as u32;
    for at in [sum_existing, sum_new] {
        rom.bytes[at..at + 4].copy_from_slice(&sum.to_be_bytes());
    }
    rom.word(0x45f9);
    rom.long(0x200007);
    rom.word(0x7000);
    rom.word(0x223c);
    rom.long(507);
    let add = rom.pc;
    for word in [0xd01a, 0xd4fc, 1] {
        rom.word(word);
    }
    rom.dbra(1, add);
    rom.word(0x4e75);
    let mut initialized = vec![0; 512];
    initialized[1] = 0x12;
    for group in 0..16 {
        initialized[5 + group * 26..5 + (group + 1) * 26]
            .copy_from_slice(&source[group * 32..group * 32 + 26]);
    }
    initialized[2] = checksum(&initialized);
    Program {
        bytes: rom.bytes,
        initialized,
        clear_end,
        copy_end,
        done,
    }
}
