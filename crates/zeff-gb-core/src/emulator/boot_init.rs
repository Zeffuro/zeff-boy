use super::{CGB_POST_BOOT_REGISTERS, DMG_POST_BOOT_REGISTERS, Emulator, RegisterSeed};
use crate::debug::{DebugController, OpcodeLog};
use crate::hardware::bus::Bus;
use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};
use sha2::{Digest, Sha256};

impl Emulator {
    fn post_boot_registers_for_mode(mode: HardwareMode) -> RegisterSeed {
        match mode {
            HardwareMode::CGBNormal | HardwareMode::CGBDouble => CGB_POST_BOOT_REGISTERS,
            HardwareMode::DMG | HardwareMode::SGB1 | HardwareMode::SGB2 => DMG_POST_BOOT_REGISTERS,
        }
    }

    pub fn new(rom: &[u8], sample_rate: u32) -> anyhow::Result<Self> {
        let mut emulator = Self::from_rom_data(rom, HardwareModePreference::Auto)?;
        emulator.set_sample_rate(sample_rate);
        Ok(emulator)
    }

    pub fn from_rom_data(
        rom: &[u8],
        mode_preference: HardwareModePreference,
    ) -> anyhow::Result<Self> {
        let rom_hash = Self::compute_rom_hash(rom);
        log::info!("ROM loaded: {} bytes", rom.len());

        let header = crate::hardware::rom_header::RomHeader::from_rom(rom)?;
        header.display_info(rom);
        let hardware_mode = mode_preference.resolve(
            header.is_cgb_compatible,
            header.is_sgb_supported,
            header.old_licensee_code,
        );
        if matches!(mode_preference, HardwareModePreference::ForceCgb) && !header.is_cgb_compatible
        {
            log::warn!("ForceCgb requested for DMG-only ROM; running in CGB mode anyway");
        }
        let bus = Box::new(Bus::new(rom.to_vec(), &header, hardware_mode)?);

        let emulator = Self {
            cpu: crate::hardware::cpu::Cpu::new(),
            bus,
            header,
            hardware_mode_preference: mode_preference,
            hardware_mode,
            cycle_count: 0,
            frame_count: 0,
            opcode_log: OpcodeLog::new(),
            call_stack: Vec::new(),
            last_opcode: 0,
            last_opcode_pc: 0,
            debug: DebugController::new(),
            rom_breakpoints: Vec::new(),
            hit_rom_breakpoint: None,
            rom_hash,
        };

        let mut emulator = emulator;
        emulator.apply_post_boot_state();
        Ok(emulator)
    }

    pub fn reset(&mut self) {
        let rom = self.bus.cartridge.rom_bytes().to_vec();
        let sample_rate = self.bus.apu_sample_rate();
        let mode_preference = self.hardware_mode_preference;

        match Self::from_rom_data(&rom, mode_preference) {
            Ok(mut emulator) => {
                emulator.set_sample_rate(sample_rate);
                *self = emulator;
            }
            Err(err) => {
                log::warn!("failed to reset GB emulator from loaded ROM bytes: {err}");
            }
        }
    }

    fn compute_rom_hash(rom: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(rom);
        hasher.finalize().into()
    }

    fn apply_post_boot_state(&mut self) {
        self.cpu.pc = 0x0100;
        self.cpu.sp = 0xFFFE;

        let (a, f, b, c, d, e, h, l) = Self::post_boot_registers_for_mode(self.hardware_mode);
        self.cpu.regs.a = a;
        self.cpu.regs.f = f;
        self.cpu.regs.b = b;
        self.cpu.regs.c = c;
        self.cpu.regs.d = d;
        self.cpu.regs.e = e;
        self.cpu.regs.h = h;
        self.cpu.regs.l = l;

        if matches!(
            self.hardware_mode,
            HardwareMode::DMG | HardwareMode::SGB1 | HardwareMode::SGB2
        ) {
            self.bus.apply_dmg_post_boot_io_state();
        }
    }
}
