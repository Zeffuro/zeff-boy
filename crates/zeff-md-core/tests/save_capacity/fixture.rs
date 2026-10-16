use zeff_md_core::{Emulator, LoadConfig, Region};

pub struct Rom {
    pub bytes: Vec<u8>,
    pub pc: usize,
}

impl Rom {
    pub fn new(size: usize) -> Self {
        let mut bytes = vec![0; 0x800];
        bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
        bytes[0x1b0..0x1ba].copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20]);
        bytes[0x1ba..0x1bc].copy_from_slice(&((size * 2 - 1) as u16).to_be_bytes());
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

    pub fn write_long(&mut self, address: u32, value: u32) {
        self.word(0x23fc);
        self.long(value);
        self.long(address);
    }

    pub fn read_byte(&mut self, address: u32, destination: u32) {
        self.word(0x1039);
        self.long(address);
        self.word(0x13c0);
        self.long(destination);
    }

    pub fn enable(&mut self) {
        self.byte(0xa130f1, 1);
    }

    pub fn phase(&mut self, value: u16) {
        self.word(0x3c3c);
        self.word(value);
    }

    pub fn psg(&mut self) {
        for value in [0x85, 0x06, 0x90] {
            self.byte(0xc00011, value);
        }
    }

    fn dbra(&mut self, target: usize) {
        self.word(0x51cf);
        self.word((target as isize - self.pc as isize) as i16 as u16);
    }

    pub fn finish(mut self) -> Vec<u8> {
        self.phase(3);
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

pub fn run_to_phase(machine: &mut Emulator, phase: u32) {
    for _ in 0..2_000_000 {
        if machine.registers().d[6] == phase {
            return;
        }
        machine.step_instruction().unwrap();
    }
    panic!(
        "save ROM phase {phase} not reached, pc={:06x}",
        machine.registers().pc
    );
}

pub fn expected(size: usize, seed: u8) -> Vec<u8> {
    (0..size)
        .map(|i| ((i.wrapping_mul(37) as u8) ^ (i >> 8) as u8).wrapping_add(seed))
        .collect()
}

pub fn all_cells_rom(size: usize) -> Vec<u8> {
    let mut rom = Rom::new(size);
    rom.enable();
    rom.psg();
    for (phase, seed) in [(1, 0x19), (2, 0xe6)] {
        rom.word(0x207c);
        rom.long(0x200001);
        rom.word(0x7000);
        rom.word(0x3e3c);
        rom.word((size - 1) as u16);
        let fill = rom.pc;
        // Fold the high index bits so aliases beyond each old capacity are visible.
        for word in [
            0x3200, 0xc2fc, 37, 0x3400, 0xe04a, 0xb501, 0x0601, seed, 0x1081, 0x5488, 0x5240,
        ] {
            rom.word(word);
        }
        rom.dbra(fill);
        rom.word(0x207c);
        rom.long(0x200001);
        rom.word(0x227c);
        rom.long(0xff0000);
        rom.word(0x3e3c);
        rom.word((size - 1) as u16);
        let read = rom.pc;
        rom.word(0x1290);
        rom.word(0x5289);
        rom.word(0x5488);
        rom.dbra(read);
        rom.phase(phase);
    }
    rom.finish()
}
