use super::super::Emulator;
use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};
use crate::hardware::types::{CpuState, ImeState};

impl Emulator {
    pub fn rom_hash(&self) -> [u8; 32] {
        self.rom_hash
    }

    pub fn header(&self) -> &RomHeader {
        &self.header
    }

    pub fn cartridge_rom_bytes(&self) -> &[u8] {
        self.bus.cartridge.rom_bytes()
    }

    pub fn hardware_mode(&self) -> HardwareMode {
        self.hardware_mode
    }

    pub fn hardware_mode_preference(&self) -> HardwareModePreference {
        self.hardware_mode_preference
    }

    pub fn is_cgb_mode(&self) -> bool {
        matches!(
            self.hardware_mode,
            HardwareMode::CGBNormal | HardwareMode::CGBDouble
        )
    }

    pub fn cpu_pc(&self) -> u16 {
        self.cpu.pc
    }

    pub fn cpu_sp(&self) -> u16 {
        self.cpu.sp
    }

    pub fn cpu_cycles(&self) -> u64 {
        self.cpu.cycles
    }

    pub fn cpu_a(&self) -> u8 {
        self.cpu.regs.a
    }

    pub fn cpu_f(&self) -> u8 {
        self.cpu.regs.f
    }

    pub fn cpu_ime(&self) -> ImeState {
        self.cpu.ime
    }

    pub fn cpu_running(&self) -> CpuState {
        self.cpu.running
    }

    pub fn is_cpu_suspended(&self) -> bool {
        self.cpu.running == CpuState::Suspended
    }

    pub fn if_reg(&self) -> u8 {
        self.bus.if_reg
    }

    pub fn ie_reg(&self) -> u8 {
        self.bus.ie
    }

    pub fn timer_div(&self) -> u8 {
        self.bus.timer_div()
    }

    pub fn timer_tima(&self) -> u8 {
        self.bus.timer_tima()
    }

    pub fn timer_tac(&self) -> u8 {
        self.bus.timer_tac()
    }

    pub fn serial_output_bytes(&self) -> &[u8] {
        self.bus.serial_output_bytes()
    }

    pub fn peek_byte(&self, addr: u16) -> u8 {
        self.bus.read_byte(addr)
    }

    pub fn peek_byte_raw(&self, addr: u16) -> u8 {
        self.bus.read_byte_raw(addr)
    }

    pub fn rumble_active(&self) -> bool {
        self.bus.cartridge.rumble_active()
    }
}

