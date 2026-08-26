use super::Bus;
use crate::hardware::cartridge::Cartridge;
use crate::hardware::io::IO;
use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::constants::{HRAM_SIZE, IO_SIZE, OAM_SIZE, VRAM_SIZE, WRAM_SIZE};
use crate::hardware::types::hardware_mode::HardwareMode;
use anyhow::Result;

impl Bus {
    pub fn new(rom: Vec<u8>, header: &RomHeader, hardware_mode: HardwareMode) -> Result<Self> {
        let cartridge = Cartridge::new(rom, header);

        let mut bus = Self {
            cartridge,
            hardware_mode,
            boot_rom: None,
            boot_rom_enabled: false,
            cgb_dmg_compat: matches!(
                hardware_mode,
                HardwareMode::CGBNormal | HardwareMode::CGBDouble
            ) && !header.is_cgb_compatible,
            vram: vec![0u8; VRAM_SIZE * 2].into_boxed_slice(),
            wram: vec![0u8; WRAM_SIZE * 8].into_boxed_slice(),
            vram_bank: 0,
            wram_bank: 1,
            key1: 0x7E,
            hdma1: 0xFF,
            hdma2: 0xFF,
            hdma3: 0xFF,
            hdma4: 0xFF,
            hdma5: 0xFF,
            hdma_active: false,
            hdma_hblank: false,
            hdma_blocks_left: 0,
            oam_dma_active: false,
            oam_dma_source_base: 0,
            oam_dma_index: 0,
            oam_dma_t_cycle_accum: 0,
            oam_dma_pending_source_base: None,
            oam: [0; OAM_SIZE],
            io_bank: [0; IO_SIZE],
            hram: [0; HRAM_SIZE],
            ie: 0,
            if_reg: 0xE1,
            cpu_interrupt_pending_before_if: 0,
            io: IO::new(),
            trace_cpu_accesses: false,
            trace_cpu_writes: false,
            cpu_access_trace_origin: zeff_emu_common::time::MasterTicks::ZERO,
            cpu_access_trace: Vec::with_capacity(12),
            game_genie_patches: Vec::new(),
        };

        bus.sync_timer_serial_mode();
        bus.io.ppu.set_sgb_mode(matches!(
            bus.hardware_mode,
            HardwareMode::SGB1 | HardwareMode::SGB2
        ));
        bus.key1 = match bus.hardware_mode {
            HardwareMode::CGBDouble => 0xFE,
            _ => 0x7E,
        };

        Ok(bus)
    }

    pub(super) fn is_cgb_hardware(&self) -> bool {
        matches!(
            self.hardware_mode,
            HardwareMode::CGBNormal | HardwareMode::CGBDouble
        )
    }

    pub(super) fn is_cgb_mode(&self) -> bool {
        self.is_cgb_hardware() && !self.cgb_dmg_compat
    }

    pub(super) fn active_vram_offset(&self) -> usize {
        if self.is_cgb_mode() {
            (self.vram_bank as usize & 0x01) * VRAM_SIZE
        } else {
            0
        }
    }

    pub(super) fn active_wram_bank(&self) -> usize {
        if self.is_cgb_mode() {
            let bank = (self.wram_bank & 0x07) as usize;
            if bank == 0 { 1 } else { bank }
        } else {
            1
        }
    }

    pub fn maybe_switch_cgb_speed(&mut self) -> bool {
        if !self.is_cgb_mode() || (self.key1 & 0x01) == 0 {
            return false;
        }

        self.hardware_mode = match self.hardware_mode {
            HardwareMode::CGBNormal => HardwareMode::CGBDouble,
            HardwareMode::CGBDouble => HardwareMode::CGBNormal,
            mode => mode,
        };

        self.sync_timer_serial_mode();
        self.key1 = match self.hardware_mode {
            HardwareMode::CGBDouble => 0xFE,
            _ => 0x7E,
        };
        true
    }

    #[inline]
    pub(in crate::hardware) fn advance_cpu_t_cycles(&mut self, cpu_t_cycles: u64) -> u64 {
        let system_t_cycles = if self.hardware_mode == HardwareMode::CGBDouble {
            cpu_t_cycles / 2
        } else {
            cpu_t_cycles
        };

        self.step_timer(cpu_t_cycles);
        self.step_serial(cpu_t_cycles);
        self.step_apu(system_t_cycles);

        let previous_ppu_mode = self.ppu_mode();
        let (ppu_interrupt, current_ppu_mode) = self.step_ppu(system_t_cycles);
        self.if_reg |= ppu_interrupt;
        self.maybe_step_hblank_hdma(previous_ppu_mode, current_ppu_mode);

        self.step_oam_dma(cpu_t_cycles);
        self.cartridge.step(system_t_cycles);

        system_t_cycles
    }

    pub(in crate::hardware) fn enter_stop_mode(&mut self) {
        if self.io.timer.reset_div() {
            self.if_reg |= 0x04;
        }
        self.clock_apu_div_events();
    }

    pub(in crate::hardware) fn advance_stopped_t_cycles(&mut self, cpu_t_cycles: u64) -> u64 {
        let system_t_cycles = if self.hardware_mode == HardwareMode::CGBDouble {
            cpu_t_cycles / 2
        } else {
            cpu_t_cycles
        };

        if self.is_cgb_hardware() {
            self.step_apu(system_t_cycles);
            let previous_ppu_mode = self.ppu_mode();
            let (ppu_interrupt, current_ppu_mode) = self.step_ppu(system_t_cycles);
            self.if_reg |= ppu_interrupt;
            self.maybe_step_hblank_hdma(previous_ppu_mode, current_ppu_mode);
        }
        self.cartridge.step(system_t_cycles);

        system_t_cycles
    }

    pub(in crate::hardware) fn advance_cgb_speed_switch_delay(&mut self) -> (u64, u64) {
        let double_speed = self.hardware_mode == HardwareMode::CGBDouble;
        let system_t_cycles = if double_speed { 65_544 } else { 65_538 };

        if self.io.timer.reset_div() {
            self.if_reg |= 0x04;
        }
        self.clock_apu_div_events();

        self.step_apu(system_t_cycles);

        let previous_ppu_mode = self.ppu_mode();
        let (ppu_interrupt, current_ppu_mode) = self.step_ppu(system_t_cycles);
        self.if_reg |= ppu_interrupt;
        self.maybe_step_hblank_hdma(previous_ppu_mode, current_ppu_mode);

        let cpu_t_cycles = if double_speed {
            system_t_cycles * 2
        } else {
            system_t_cycles
        };
        (cpu_t_cycles, system_t_cycles)
    }
}
