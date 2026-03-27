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

    pub fn from_rom_data(
        rom: &[u8],
        mode_preference: HardwareModePreference,
    ) -> Result<Self, Box<dyn std::error::Error>> {
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
            log::warn!(
                "ForceCgb requested for DMG-only ROM; falling back to DMG mode for compatibility"
            );
        }
        let bus = Bus::new(rom.to_vec(), &header, hardware_mode)?;

        let emulator = Self {
            cpu: crate::hardware::cpu::Cpu::new(),
            bus,
            header,
            hardware_mode_preference: mode_preference,
            hardware_mode,
            cycle_count: 0,
            opcode_log: OpcodeLog::new(32),
            last_opcode: 0,
            last_opcode_pc: 0,
            debug: DebugController::new(),
            rom_hash,
        };

        let mut emulator = emulator;
        emulator.apply_post_boot_state();
        Ok(emulator)
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
    }
}
