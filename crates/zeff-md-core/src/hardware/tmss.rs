use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_m68k::Registers;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tmss {
    rom: Arc<[u8]>,
    digest: [u8; 32],
    lock: [u16; 2],
    cartridge: bool,
}

impl Tmss {
    pub fn new(rom: Vec<u8>) -> Result<Self> {
        ensure!(rom.len() == 2048, "TMSS requires a 2 KiB boot ROM");
        let pc = u32::from_be_bytes(rom[4..8].try_into().unwrap());
        ensure!(
            pc & 1 == 0 && pc <= crate::constants::CARTRIDGE_END,
            "invalid TMSS reset vector"
        );
        Ok(Self {
            digest: Sha256::digest(&rom).into(),
            rom: rom.into(),
            lock: [0; 2],
            cartridge: false,
        })
    }

    pub fn power_on(&mut self) {
        self.lock = [0; 2];
        self.cartridge = false;
    }

    pub fn initial(&self) -> Registers {
        let mut registers = Registers::new();
        registers.ssp = u32::from_be_bytes(self.rom[..4].try_into().unwrap());
        registers.pc = u32::from_be_bytes(self.rom[4..8].try_into().unwrap());
        registers
    }

    pub fn read_boot(&self, address: u32) -> Option<u8> {
        (!self.cartridge && address <= crate::constants::CARTRIDGE_END)
            .then(|| self.rom[address as usize & 2047])
    }

    pub fn boot_mapped(&self) -> bool {
        !self.cartridge
    }
    pub fn unlocked(&self) -> bool {
        self.lock == [0x5345, 0x4741]
    }
    pub fn lock_word(&self, index: usize) -> u16 {
        self.lock[index]
    }
    pub fn write_lock(&mut self, index: usize, value: u16) {
        self.lock[index] = value;
    }
    pub fn select_cartridge(&mut self, value: bool) {
        self.cartridge = value;
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.digest);
        for word in self.lock {
            writer.write_u16(word);
        }
        writer.write_bool(self.cartridge);
    }

    pub fn read_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        let mut digest = [0; 32];
        reader.read_exact(&mut digest)?;
        ensure!(digest == self.digest, "TMSS save-state firmware mismatch");
        self.lock = [reader.read_u16()?, reader.read_u16()?];
        self.cartridge = reader.read_bool()?;
        Ok(())
    }
}
