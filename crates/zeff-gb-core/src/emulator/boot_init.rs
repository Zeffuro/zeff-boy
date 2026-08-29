use super::{CGB_POST_BOOT_REGISTERS, DMG_POST_BOOT_REGISTERS, Emulator, RegisterSeed};
use crate::debug::{DebugController, OpcodeLog};
use crate::hardware::bus::Bus;
use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};
use sha2::{Digest, Sha256};

impl Emulator {
    fn post_boot_registers_for_mode(mode: HardwareMode) -> RegisterSeed {
        match mode {
            HardwareMode::CGBNormal | HardwareMode::CGBDouble => CGB_POST_BOOT_REGISTERS,
            HardwareMode::DMG | HardwareMode::SGB1 | HardwareMode::SGB2 => DMG_POST_BOOT_REGISTERS,
        }
    }

    fn cgb_post_boot_divider_counter(header: &RomHeader) -> u16 {
        let new_licensee = header.new_licensee_code.as_deref().unwrap_or_default();
        let new_starts_with_zero = new_licensee.as_bytes().first() == Some(&b'0');
        let new_ends_with_one = new_licensee.as_bytes().get(1) == Some(&b'1');

        if header.cgb_flag & 0x80 != 0 {
            return match header.old_licensee_code {
                0x01 => 0x2FA8,
                0x33 if new_starts_with_zero && new_ends_with_one => 0x2FC8,
                0x33 if new_starts_with_zero => 0x1EC0,
                0x33 => 0x1E9C,
                _ => 0x1EA0,
            };
        }

        match header.old_licensee_code {
            0x01 => 0x3784,
            0x33 if new_starts_with_zero && new_ends_with_one => 0x37A4,
            0x33 if new_starts_with_zero => 0x269C,
            0x33 => 0x2678,
            _ => 0x267C,
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
        Self::from_rom_data_inner(rom, mode_preference, None)
    }

    pub fn from_rom_data_with_boot_rom(
        rom: &[u8],
        mode_preference: HardwareModePreference,
        boot_rom: &[u8],
    ) -> anyhow::Result<Self> {
        Self::from_rom_data_inner(rom, mode_preference, Some(boot_rom))
    }

    fn from_rom_data_inner(
        rom: &[u8],
        mode_preference: HardwareModePreference,
        boot_rom: Option<&[u8]>,
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
        if let Some(boot_rom) = boot_rom {
            let expected_len = if matches!(
                hardware_mode,
                HardwareMode::CGBNormal | HardwareMode::CGBDouble
            ) {
                0x900
            } else {
                0x100
            };
            anyhow::ensure!(
                boot_rom.len() == expected_len,
                "{} boot ROM must be {expected_len} bytes, got {}",
                if expected_len == 0x900 { "CGB" } else { "DMG" },
                boot_rom.len()
            );
        }
        let mut bus = Box::new(Bus::new(rom.to_vec(), &header, hardware_mode)?);
        bus.set_boot_rom(boot_rom.map(<[u8]>::to_vec), boot_rom.is_some());

        let emulator = Self {
            cpu: crate::hardware::cpu::Cpu::new(),
            bus,
            header,
            hardware_mode_preference: mode_preference,
            hardware_mode,
            cycle_count: 0,
            frame_count: 0,
            opcode_log: OpcodeLog::new(),
            instruction_trace: zeff_emu_common::debug::InstructionTraceStore::default(),
            call_stack: Vec::new(),
            last_opcode: 0,
            last_opcode_pc: 0,
            debug: DebugController::new(),
            rom_breakpoints: Vec::new(),
            hit_rom_breakpoint: None,
            rom_hash,
        };

        let mut emulator = emulator;
        if boot_rom.is_some() {
            emulator.apply_power_on_state();
        } else {
            emulator.apply_post_boot_state();
        }
        Ok(emulator)
    }

    pub fn reset(&mut self) {
        let rom = self.bus.cartridge.rom_bytes().to_vec();
        let sample_rate = self.bus.apu_sample_rate();
        let boot_rom = self.bus.boot_rom_bytes().map(<[u8]>::to_vec);
        let mode_preference = self.hardware_mode_preference;
        let serial_device = self.game_boy_serial_device();
        let ppu_debug_flags = self.bus.ppu_debug_flags();
        let trace_enabled = self.instruction_trace.is_enabled();
        let trace_capacity = self.instruction_trace.capacity();

        let reset = match boot_rom.as_deref() {
            Some(boot_rom) => Self::from_rom_data_with_boot_rom(&rom, mode_preference, boot_rom),
            None => Self::from_rom_data(&rom, mode_preference),
        };
        match reset {
            Ok(mut emulator) => {
                emulator.set_sample_rate(sample_rate);
                emulator.set_game_boy_serial_device(serial_device);
                emulator.set_ppu_debug_flags(
                    ppu_debug_flags.bg,
                    ppu_debug_flags.window,
                    ppu_debug_flags.sprites,
                );
                emulator.instruction_trace.set_capacity(trace_capacity);
                emulator.instruction_trace.set_enabled(trace_enabled);
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
        } else {
            let divider_counter = Self::cgb_post_boot_divider_counter(&self.header);
            self.bus.apply_cgb_post_boot_timer_state(divider_counter);
        }
    }

    fn apply_power_on_state(&mut self) {
        self.cpu = crate::hardware::cpu::Cpu::new();
        self.cpu.pc = 0;
        self.cpu.sp = 0;
        self.bus.apply_boot_rom_power_on_state();
    }
}
