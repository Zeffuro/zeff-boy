use super::fixture::Rom;
use zeff_md_core::Emulator;

pub const PORT: u32 = 0x200000;
pub const PRODUCT: &[u8; 14] = b"GM T-081276 00";
pub const DECLARATION: &[u8; 12] = b"RA\xe8\x40\x00\x20\x00\x00\x00\x20\x00\x01";

pub struct Guest {
    pub rom: Rom,
    pub cuts: Vec<u32>,
    scl: bool,
    sda: bool,
}

impl Guest {
    pub fn new() -> Self {
        let mut rom = Rom::new();
        rom.bytes.resize(0x300000, 0);
        rom.bytes[0x180..0x18e].copy_from_slice(PRODUCT);
        rom.bytes[0x1b0..0x1bc].copy_from_slice(DECLARATION);
        rom.bytes[0x200001] = 0x6b;
        Self {
            rom,
            cuts: Vec::new(),
            scl: true,
            sda: true,
        }
    }

    pub fn mark(&mut self) -> u32 {
        let pc = self.rom.pc as u32;
        self.cuts.push(pc);
        pc
    }

    pub fn mapping(&mut self, rom: bool) {
        self.rom.write(0x33fc, PORT, 0xa400 | u16::from(rom));
    }

    pub fn line(&mut self, value: u8) {
        let scl = value & 2 != 0;
        let sda = value & 1 != 0;
        if self.scl && !scl {
            self.rom.write(0x13fc, PORT, 0xa4);
        }
        if self.sda != sda {
            self.rom.write(0x13fc, PORT + 1, 0xa4 | u16::from(sda));
        }
        if !self.scl && scl {
            self.rom.write(0x13fc, PORT, 0xa5);
        }
        self.scl = scl;
        self.sda = sda;
    }

    pub fn start(&mut self) {
        self.line(1);
        self.line(3);
        self.line(2);
        self.line(0);
    }

    pub fn stop(&mut self) {
        self.line(0);
        self.line(2);
        self.line(3);
    }

    pub fn send(&mut self, byte: u8) -> usize {
        self.send_inner(byte, false)
    }

    pub(super) fn send_inner(&mut self, byte: u8, held: bool) -> usize {
        for bit in (0..8).rev() {
            let data = (byte >> bit) & 1;
            self.line(data);
            self.line(data | 2);
            self.line(data);
        }
        self.line(1);
        self.line(3);
        let ack = self.rom.sample();
        if held {
            self.mark();
            self.mapping(true);
            self.mark();
            self.mapping(false);
            self.mark();
        }
        self.line(1);
        ack
    }

    pub fn receive(&mut self, ack: bool, checkpoint: bool) -> usize {
        let first = self.rom.results;
        for bit in 0..8 {
            self.line(1);
            self.line(3);
            self.rom.sample();
            if checkpoint && bit == 3 {
                self.mark();
            }
            self.line(1);
        }
        let data = u8::from(!ack);
        self.line(data);
        self.line(data | 2);
        self.line(data);
        first
    }

    pub fn wait(&mut self) {
        self.rom.word(0x3e3c);
        self.rom.word(0x2fff);
        self.rom.word(0x51cf);
        self.rom.word(0xfffe);
    }
}

pub fn received(machine: &Emulator, first: usize) -> u8 {
    (0..8).fold(0, |value, bit| {
        let sample = super::fixture::sample(machine, first + bit);
        assert!(sample <= 1);
        (value << 1) | sample
    })
}
