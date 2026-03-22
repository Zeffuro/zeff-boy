use crate::hardware::cartridge::Cartridge;
use crate::hardware::io::IO;
use crate::hardware::rom_header::RomHeader;
use crate::hardware::sgb::SgbEvent;
use crate::hardware::types::constants::*;
use crate::hardware::types::hardware_mode::HardwareMode;
use crate::save_state::{StateReader, StateWriter, decode_hardware_mode};
use anyhow::Result;
use log::warn;

pub(crate) struct Bus {
    pub(crate) cartridge: Cartridge,
    pub(crate) hardware_mode: HardwareMode,
    pub(crate) vram: [u8; VRAM_SIZE * 2], // CGB has 2x8KB VRAM banks
    pub(crate) wram: [u8; WRAM_SIZE * 8], // CGB has 8x4KB WRAM banks
    pub(crate) vram_bank: u8,
    pub(crate) wram_bank: u8,
    pub(crate) key1: u8,
    pub(crate) hdma1: u8,
    pub(crate) hdma2: u8,
    pub(crate) hdma3: u8,
    pub(crate) hdma4: u8,
    pub(crate) hdma5: u8,
    pub(crate) hdma_active: bool,
    pub(crate) hdma_hblank: bool,
    pub(crate) hdma_blocks_left: u8,
    pub(crate) oam_dma_active: bool,
    oam_dma_source_base: u16,
    oam_dma_index: u16,
    oam_dma_t_cycle_accum: u64,
    pub(crate) oam: [u8; OAM_SIZE],    // 0xFE00..0xFE9F
    pub(crate) io_bank: [u8; IO_SIZE], // 0xFF00..0xFF7F
    pub(crate) hram: [u8; HRAM_SIZE],  // 0xFF80..0xFFFE
    pub(crate) ie: u8,                 // 0xFFFF
    pub(crate) if_reg: u8,             // 0xFF0F
    pub(crate) io: IO,
    pub(crate) trace_cpu_accesses: bool,
    cpu_read_trace: Vec<(u16, u8)>,
    cpu_write_trace: Vec<(u16, u8, u8)>,
}

pub(crate) enum CpuAccessTraceEvent {
    Read {
        addr: u16,
        value: u8,
    },
    Write {
        addr: u16,
        old_value: u8,
        new_value: u8,
    },
}

impl Bus {
    pub(crate) fn new(
        rom: Vec<u8>,
        header: &RomHeader,
        hardware_mode: HardwareMode,
    ) -> Result<Box<Self>> {
        let cartridge = Cartridge::new(rom, header);

        Ok(Box::new(Self {
            cartridge,
            hardware_mode,
            vram: [0; VRAM_SIZE * 2],
            wram: [0; WRAM_SIZE * 8],
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
            oam: [0; OAM_SIZE],
            io_bank: [0; IO_SIZE],
            hram: [0; HRAM_SIZE],
            ie: 0,
            if_reg: 0xE1,
            io: IO::new(),
            trace_cpu_accesses: false,
            cpu_read_trace: Vec::with_capacity(8),
            cpu_write_trace: Vec::with_capacity(4),
        }))
        .map(|mut bus| {
            bus.io.timer.mode = bus.hardware_mode;
            bus.io.serial.mode = bus.hardware_mode;
            bus.io.ppu.set_sgb_mode(matches!(
                bus.hardware_mode,
                HardwareMode::SGB1 | HardwareMode::SGB2
            ));
            bus.key1 = match bus.hardware_mode {
                HardwareMode::CGBDouble => 0xFE,
                _ => 0x7E,
            };
            bus
        })
    }

    fn is_cgb_mode(&self) -> bool {
        matches!(
            self.hardware_mode,
            HardwareMode::CGBNormal | HardwareMode::CGBDouble
        )
    }

    fn active_vram_offset(&self) -> usize {
        if self.is_cgb_mode() {
            (self.vram_bank as usize & 0x01) * VRAM_SIZE
        } else {
            0
        }
    }

    fn active_wram_bank(&self) -> usize {
        if self.is_cgb_mode() {
            let bank = (self.wram_bank & 0x07) as usize;
            if bank == 0 { 1 } else { bank }
        } else {
            1
        }
    }

    pub(crate) fn maybe_switch_cgb_speed(&mut self) -> bool {
        if !self.is_cgb_mode() || (self.key1 & 0x01) == 0 {
            return false;
        }

        self.hardware_mode = match self.hardware_mode {
            HardwareMode::CGBNormal => HardwareMode::CGBDouble,
            HardwareMode::CGBDouble => HardwareMode::CGBNormal,
            mode => mode,
        };

        self.io.timer.mode = self.hardware_mode;
        self.io.serial.mode = self.hardware_mode;
        self.key1 = match self.hardware_mode {
            HardwareMode::CGBDouble => 0xFE,
            _ => 0x7E,
        };
        true
    }

    pub(crate) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(encode_hardware_mode(self.hardware_mode));
        self.cartridge.write_state(writer);
        writer.write_bytes(&self.vram);
        writer.write_bytes(&self.wram);
        writer.write_u8(self.vram_bank);
        writer.write_u8(self.wram_bank);
        writer.write_u8(self.key1);
        writer.write_u8(self.hdma1);
        writer.write_u8(self.hdma2);
        writer.write_u8(self.hdma3);
        writer.write_u8(self.hdma4);
        writer.write_u8(self.hdma5);
        writer.write_bool(self.hdma_active);
        writer.write_bool(self.hdma_hblank);
        writer.write_u8(self.hdma_blocks_left);
        writer.write_bool(self.oam_dma_active);
        writer.write_u16(self.oam_dma_source_base);
        writer.write_u16(self.oam_dma_index);
        writer.write_u64(self.oam_dma_t_cycle_accum);
        writer.write_bytes(&self.oam);
        writer.write_bytes(&self.io_bank);
        writer.write_bytes(&self.hram);
        writer.write_u8(self.ie);
        writer.write_u8(self.if_reg);
        self.io.write_state(writer);
    }

    pub(crate) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let hardware_mode = decode_hardware_mode(reader.read_u8()?)?;
        let cartridge = Cartridge::read_state(reader)?;

        let mut bus = Self {
            cartridge,
            hardware_mode,
            vram: [0; VRAM_SIZE * 2],
            wram: [0; WRAM_SIZE * 8],
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
            oam: [0; OAM_SIZE],
            io_bank: [0; IO_SIZE],
            hram: [0; HRAM_SIZE],
            ie: 0,
            if_reg: 0,
            io: IO::new(),
            trace_cpu_accesses: false,
            cpu_read_trace: Vec::with_capacity(8),
            cpu_write_trace: Vec::with_capacity(4),
        };

        reader.read_exact(&mut bus.vram)?;
        reader.read_exact(&mut bus.wram)?;
        bus.vram_bank = reader.read_u8()?;
        bus.wram_bank = reader.read_u8()?;
        bus.key1 = reader.read_u8()?;
        bus.hdma1 = reader.read_u8()?;
        bus.hdma2 = reader.read_u8()?;
        bus.hdma3 = reader.read_u8()?;
        bus.hdma4 = reader.read_u8()?;
        bus.hdma5 = reader.read_u8()?;
        bus.hdma_active = reader.read_bool()?;
        bus.hdma_hblank = reader.read_bool()?;
        bus.hdma_blocks_left = reader.read_u8()?;
        bus.oam_dma_active = reader.read_bool()?;
        bus.oam_dma_source_base = reader.read_u16()?;
        bus.oam_dma_index = reader.read_u16()?;
        bus.oam_dma_t_cycle_accum = reader.read_u64()?;
        reader.read_exact(&mut bus.oam)?;
        reader.read_exact(&mut bus.io_bank)?;
        reader.read_exact(&mut bus.hram)?;
        bus.ie = reader.read_u8()?;
        bus.if_reg = reader.read_u8()?;
        bus.io = IO::read_state(reader)?;

        bus.trace_cpu_accesses = false;
        bus.cpu_read_trace.clear();
        bus.cpu_write_trace.clear();
        Ok(bus)
    }

    pub(crate) fn cpu_read_byte(&mut self, addr: u16) -> u8 {
        if self.oam_dma_active && !is_hram_addr(addr) {
            return 0xFF;
        }
        let value = self.read_byte(addr);
        if self.trace_cpu_accesses {
            self.cpu_read_trace.push((addr, value));
        }
        value
    }

    pub(crate) fn cpu_write_byte(&mut self, addr: u16, value: u8) -> u64 {
        if self.oam_dma_active && !is_hram_addr(addr) {
            return 0;
        }
        let old_value = if self.trace_cpu_accesses {
            self.read_byte(addr)
        } else {
            0
        };
        let extra_t_cycles = self.write_byte(addr, value);
        if self.trace_cpu_accesses {
            let new_value = self.read_byte(addr);
            self.cpu_write_trace.push((addr, old_value, new_value));
        }
        extra_t_cycles
    }

    pub(crate) fn begin_cpu_access_trace(&mut self) {
        self.cpu_read_trace.clear();
        self.cpu_write_trace.clear();
    }

    pub(crate) fn drain_cpu_access_trace(&mut self, mut on_event: impl FnMut(CpuAccessTraceEvent)) {
        for &(addr, value) in &self.cpu_read_trace {
            on_event(CpuAccessTraceEvent::Read { addr, value });
        }
        for &(addr, old_value, new_value) in &self.cpu_write_trace {
            on_event(CpuAccessTraceEvent::Write {
                addr,
                old_value,
                new_value,
            });
        }
        self.cpu_read_trace.clear();
        self.cpu_write_trace.clear();
    }

    pub(crate) fn step_oam_dma(&mut self, t_cycles: u64) {
        if !self.oam_dma_active {
            return;
        }

        self.oam_dma_t_cycle_accum = self.oam_dma_t_cycle_accum.wrapping_add(t_cycles);
        while self.oam_dma_index < 160 {
            let needed_cycles = if self.oam_dma_index == 0 { 8 } else { 4 };
            if self.oam_dma_t_cycle_accum < needed_cycles {
                break;
            }
            self.oam_dma_t_cycle_accum -= needed_cycles;

            let source_addr = self.oam_dma_source_base.wrapping_add(self.oam_dma_index);
            let value = self.read_byte(source_addr);
            self.oam[self.oam_dma_index as usize] = value;
            self.oam_dma_index += 1;
        }

        if self.oam_dma_index >= 160 {
            self.oam_dma_active = false;
            self.oam_dma_t_cycle_accum = 0;
        }
    }

    fn write_vram_dma(&mut self, addr: u16, value: u8) {
        if (VRAM_START..=VRAM_END).contains(&addr) {
            let local = (addr - VRAM_START) as usize;
            let index = self.active_vram_offset() + local;
            self.vram[index] = value;
        }
    }

    fn hdma_source_addr(&self) -> u16 {
        (u16::from(self.hdma1) << 8) | u16::from(self.hdma2 & 0xF0)
    }

    fn hdma_dest_addr(&self) -> u16 {
        0x8000 | ((u16::from(self.hdma3 & 0x1F) << 8) | u16::from(self.hdma4 & 0xF0))
    }

    fn transfer_one_hdma_block(&mut self) {
        if !self.hdma_active || self.hdma_blocks_left == 0 {
            return;
        }

        let source = self.hdma_source_addr();
        let dest = self.hdma_dest_addr();

        for i in 0..0x10u16 {
            let src = source.wrapping_add(i);
            let dst = dest.wrapping_add(i);
            let value = self.read_byte(src);
            self.write_vram_dma(dst, value);
        }

        let source_end = source.wrapping_add(0x10);
        let dest_end = dest.wrapping_add(0x10);
        self.hdma1 = (source_end >> 8) as u8;
        self.hdma2 = (source_end as u8) & 0xF0;
        self.hdma3 = ((dest_end >> 8) as u8) & 0x1F;
        self.hdma4 = (dest_end as u8) & 0xF0;
        self.hdma_blocks_left = self.hdma_blocks_left.saturating_sub(1);

        if self.hdma_blocks_left == 0 {
            self.hdma_active = false;
            self.hdma_hblank = false;
            self.hdma5 = 0xFF;
        } else {
            self.hdma5 = self.hdma_blocks_left.wrapping_sub(1) & 0x7F;
        }
    }

    fn execute_hdma_transfer(&mut self, control: u8) -> u64 {
        if self.hdma_active && self.hdma_hblank && (control & 0x80) == 0 {
            self.hdma_active = false;
            self.hdma_hblank = false;
            self.hdma5 = 0x80 | (self.hdma_blocks_left.saturating_sub(1) & 0x7F);
            return 0;
        }

        self.hdma_blocks_left = (control & 0x7F).wrapping_add(1);
        self.hdma_active = true;
        self.hdma_hblank = (control & 0x80) != 0;

        if self.hdma_hblank {
            self.hdma5 = self.hdma_blocks_left.wrapping_sub(1) & 0x7F;
            return 0;
        }

        let blocks = self.hdma_blocks_left as u64;
        let per_block_t_cycles = match self.hardware_mode {
            HardwareMode::CGBDouble => 64,
            _ => 32,
        };

        while self.hdma_active {
            self.transfer_one_hdma_block();
        }

        blocks * per_block_t_cycles
    }

    pub(crate) fn maybe_step_hblank_hdma(&mut self, previous_ppu_mode: u8, current_ppu_mode: u8) {
        if !self.is_cgb_mode() || !self.hdma_active || !self.hdma_hblank {
            return;
        }

        if self.io.ppu.lcdc & 0x80 == 0 || self.io.ppu.ly >= 144 {
            return;
        }

        if previous_ppu_mode != 0 && current_ppu_mode == 0 {
            self.transfer_one_hdma_block();
        }
    }

    #[allow(unreachable_patterns)]
    pub(crate) fn read_byte(&self, addr: u16) -> u8 {
        match addr {
            ROM_BANK_0_START..=ROM_BANK_N_END => self.cartridge.read_rom(addr),
            VRAM_START..=VRAM_END => {
                if !self.io.ppu.cpu_vram_accessible() {
                    return 0xFF;
                }
                let local = (addr - VRAM_START) as usize;
                self.vram[self.active_vram_offset() + local]
            }
            ERAM_START..=ERAM_END => self.cartridge.read_ram(addr),
            WRAM_0_START..=WRAM_0_END => self.wram[(addr - WRAM_0_START) as usize],
            WRAM_N_START..=WRAM_N_END => {
                let local = (addr - WRAM_N_START) as usize;
                self.wram[self.active_wram_bank() * WRAM_SIZE + local]
            }
            ECHO_RAM_START..=ECHO_RAM_END => {
                let mirror_addr = addr - ECHO_RAM_OFFSET;
                self.read_byte(mirror_addr)
            }
            OAM_START..=OAM_END => {
                if !self.io.ppu.cpu_oam_accessible() {
                    return 0xFF;
                }
                self.oam[(addr - OAM_START) as usize]
            }
            NOT_USABLE_START..=NOT_USABLE_END => 0xFF,
            SERIAL_SB => self.io.serial.sb,
            SERIAL_SC => self.io.serial.sc,
            INTERRUPT_IF => self.if_reg,
            IO_START..=IO_END => self.read_io(addr),
            HRAM_START..=HRAM_END => self.hram[(addr - HRAM_START) as usize],
            IE_ADDR => self.ie,
            _ => 0xFF,
        }
    }

    #[allow(unreachable_patterns)]
    pub(crate) fn write_byte(&mut self, addr: u16, value: u8) -> u64 {
        match addr {
            ROM_BANK_0_START..=ROM_BANK_N_END => {
                self.cartridge.write_rom(addr, value);
                0
            }
            VRAM_START..=VRAM_END => {
                if !self.io.ppu.cpu_vram_accessible() {
                    return 0;
                }
                let local = (addr - VRAM_START) as usize;
                let index = self.active_vram_offset() + local;
                self.vram[index] = value;
                0
            }
            ERAM_START..=ERAM_END => {
                self.cartridge.write_ram(addr, value);
                0
            }
            WRAM_0_START..=WRAM_0_END => {
                self.wram[(addr - WRAM_0_START) as usize] = value;
                0
            }
            WRAM_N_START..=WRAM_N_END => {
                let local = (addr - WRAM_N_START) as usize;
                let index = self.active_wram_bank() * WRAM_SIZE + local;
                self.wram[index] = value;
                0
            }
            ECHO_RAM_START..=ECHO_RAM_END => {
                let mirror_addr = addr - ECHO_RAM_OFFSET;
                self.write_byte(mirror_addr, value)
            }
            OAM_START..=OAM_END => {
                if !self.io.ppu.cpu_oam_accessible() {
                    return 0;
                }
                self.oam[(addr - OAM_START) as usize] = value;
                0
            }
            NOT_USABLE_START..=NOT_USABLE_END => {
                warn!("Attempted illegal write to ROM at address {:04X}", addr);
                0
            }
            IO_START..=IO_END => self.write_io(addr, value),
            HRAM_START..=HRAM_END => {
                self.hram[(addr - HRAM_START) as usize] = value;
                0
            }
            IE_ADDR => {
                self.ie = value;
                0
            }
            _ => {
                warn!("Attempted illegal write to UNKNOWN at address {:04X}", addr);
                0
            }
        }
    }

    fn read_io(&self, addr: u16) -> u8 {
        match addr {
            JOYP_P1 => self.io.joypad.read(),
            SERIAL_SB => self.io.serial.sb,
            SERIAL_SC => self.io.serial.sc,

            TIMER_DIV => self.io.timer.div,
            TIMER_TIMA => self.io.timer.tima,
            TIMER_TMA => self.io.timer.tma,
            TIMER_TAC => self.io.timer.tac,

            INTERRUPT_IF => self.if_reg | 0xE0,

            PPU_LCDC => self.io.ppu.lcdc,
            PPU_STAT => self.io.ppu.stat | 0x80,
            PPU_SCY => self.io.ppu.scy,
            PPU_SCX => self.io.ppu.scx,
            PPU_LY => self.io.ppu.ly,
            PPU_LYC => self.io.ppu.lyc,
            PPU_WY => self.io.ppu.wy,
            PPU_WX => self.io.ppu.wx,
            PPU_BGP => self.io.ppu.bgp,
            PPU_OBP0 => self.io.ppu.obp0,
            PPU_OBP1 => self.io.ppu.obp1,
            PPU_DMA => self.io_bank[(addr - IO_START) as usize],
            CGB_KEY1 => {
                if self.is_cgb_mode() {
                    self.key1
                } else {
                    0xFF
                }
            }
            CGB_BCPS => {
                if self.is_cgb_mode() {
                    self.io.ppu.read_bcps()
                } else {
                    0xFF
                }
            }
            CGB_BCPD => {
                if self.is_cgb_mode() {
                    self.io.ppu.read_bcpd()
                } else {
                    0xFF
                }
            }
            CGB_OCPS => {
                if self.is_cgb_mode() {
                    self.io.ppu.read_ocps()
                } else {
                    0xFF
                }
            }
            CGB_OCPD => {
                if self.is_cgb_mode() {
                    self.io.ppu.read_ocpd()
                } else {
                    0xFF
                }
            }
            CGB_HDMA1 => {
                if self.is_cgb_mode() {
                    self.hdma1
                } else {
                    0xFF
                }
            }
            CGB_HDMA2 => {
                if self.is_cgb_mode() {
                    self.hdma2
                } else {
                    0xFF
                }
            }
            CGB_HDMA3 => {
                if self.is_cgb_mode() {
                    self.hdma3
                } else {
                    0xFF
                }
            }
            CGB_HDMA4 => {
                if self.is_cgb_mode() {
                    self.hdma4
                } else {
                    0xFF
                }
            }
            CGB_HDMA5 => {
                if self.is_cgb_mode() {
                    self.hdma5
                } else {
                    0xFF
                }
            }
            PPU_VBK => {
                if self.is_cgb_mode() {
                    0xFE | (self.vram_bank & 0x01)
                } else {
                    0xFF
                }
            }
            CGB_SVBK => {
                if self.is_cgb_mode() {
                    0xF8 | (self.wram_bank & 0x07)
                } else {
                    0xFF
                }
            }
            NR10..=NR52 | WAVE_RAM_START..=WAVE_RAM_END => self.io.apu.read(addr),
            CGB_PCM12 | CGB_PCM34 => {
                if self.is_cgb_mode() {
                    self.io.apu.read(addr)
                } else {
                    0xFF
                }
            }

            _ => self.io_bank[(addr - IO_START) as usize],
        }
    }

    fn write_io(&mut self, addr: u16, value: u8) -> u64 {
        self.io_bank[(addr - IO_START) as usize] = value;

        match addr {
            JOYP_P1 => {
                self.io.joypad.write(value);
                if matches!(self.hardware_mode, HardwareMode::SGB1 | HardwareMode::SGB2) {
                    if let Some(event) = self.io.sgb.on_joyp_write(value) {
                        self.apply_sgb_event(event);
                    }
                }
            }
            SERIAL_SB => self.io.serial.sb = value,
            SERIAL_SC => self.io.serial.sc = value,

            TIMER_DIV => self.io.timer.reset_div(),
            TIMER_TIMA => self.io.timer.write_tima(value),
            TIMER_TMA => self.io.timer.tma = value,
            TIMER_TAC => self.io.timer.write_tac(value),

            INTERRUPT_IF => self.if_reg = value & 0x1F,

            PPU_LCDC => self.io.ppu.lcdc = value,
            PPU_STAT => self.io.ppu.stat = (self.io.ppu.stat & 0x07) | (value & 0xF8),
            PPU_SCY => self.io.ppu.scy = value,
            PPU_SCX => self.io.ppu.scx = value,
            PPU_LY => self.io.ppu.ly = 0,
            PPU_LYC => self.io.ppu.lyc = value,
            PPU_WY => self.io.ppu.wy = value,
            PPU_WX => self.io.ppu.wx = value,
            PPU_BGP => self.io.ppu.bgp = value,
            PPU_OBP0 => self.io.ppu.obp0 = value,
            PPU_OBP1 => self.io.ppu.obp1 = value,
            PPU_DMA => {
                self.oam_dma_source_base = (value as u16) << 8;
                self.oam_dma_index = 0;
                self.oam_dma_t_cycle_accum = 0;
                self.oam_dma_active = true;
            }
            CGB_KEY1 => {
                if self.is_cgb_mode() {
                    self.key1 = (self.key1 & 0x80) | (value & 0x01) | 0x7E;
                }
            }
            CGB_BCPS => {
                if self.is_cgb_mode() {
                    self.io.ppu.write_bcps(value);
                }
            }
            CGB_BCPD => {
                if self.is_cgb_mode() {
                    self.io.ppu.write_bcpd(value);
                }
            }
            CGB_OCPS => {
                if self.is_cgb_mode() {
                    self.io.ppu.write_ocps(value);
                }
            }
            CGB_OCPD => {
                if self.is_cgb_mode() {
                    self.io.ppu.write_ocpd(value);
                }
            }
            CGB_HDMA1 => {
                if self.is_cgb_mode() {
                    self.hdma1 = value;
                }
            }
            CGB_HDMA2 => {
                if self.is_cgb_mode() {
                    self.hdma2 = value;
                }
            }
            CGB_HDMA3 => {
                if self.is_cgb_mode() {
                    self.hdma3 = value;
                }
            }
            CGB_HDMA4 => {
                if self.is_cgb_mode() {
                    self.hdma4 = value;
                }
            }
            CGB_HDMA5 => {
                if self.is_cgb_mode() {
                    return self.execute_hdma_transfer(value);
                }
            }
            PPU_VBK => {
                if self.is_cgb_mode() {
                    self.vram_bank = value & 0x01;
                }
            }
            CGB_SVBK => {
                if self.is_cgb_mode() {
                    let bank = value & 0x07;
                    self.wram_bank = if bank == 0 { 1 } else { bank };
                }
            }
            NR10..=NR52 | WAVE_RAM_START..=WAVE_RAM_END => self.io.apu.write(addr, value),

            _ => {}
        }

        0
    }

    fn apply_sgb_event(&mut self, event: SgbEvent) {
        match event {
            SgbEvent::Pal01(p0, p1) => {
                self.io.ppu.set_sgb_palette(0, p0);
                self.io.ppu.set_sgb_palette(1, p1);
            }
            SgbEvent::Pal23(p2, p3) => {
                self.io.ppu.set_sgb_palette(2, p2);
                self.io.ppu.set_sgb_palette(3, p3);
            }
            SgbEvent::PalSet(index) => self.io.ppu.set_sgb_active_palette(index),
            SgbEvent::MaskEn(mode) => self.io.ppu.set_sgb_mask_mode(mode),
            SgbEvent::MltReq => {}
        }
    }
}

fn encode_hardware_mode(mode: HardwareMode) -> u8 {
    match mode {
        HardwareMode::DMG => 0,
        HardwareMode::SGB1 => 1,
        HardwareMode::SGB2 => 2,
        HardwareMode::CGBNormal => 3,
        HardwareMode::CGBDouble => 4,
    }
}

fn is_hram_addr(addr: u16) -> bool {
    (HRAM_START..=HRAM_END).contains(&addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_bus() -> Box<Bus> {
        let mut rom = vec![0u8; 0x8000];
        for (i, byte) in rom.iter_mut().take(0x100).enumerate() {
            *byte = i as u8;
        }
        let header = RomHeader::from_rom(&rom).expect("test ROM header should parse");
        Bus::new(rom, &header, HardwareMode::DMG).expect("test bus should initialize")
    }

    #[test]
    fn oam_dma_transfers_one_byte_per_m_cycle() {
        let mut bus = make_test_bus();
        bus.oam[0] = 0xAA;
        bus.oam[1] = 0xBB;
        bus.write_byte(PPU_DMA, 0x00);

        assert!(bus.oam_dma_active);
        assert_eq!(bus.oam[0], 0xAA);

        bus.step_oam_dma(4);
        assert_eq!(bus.oam[0], 0xAA);
        assert_eq!(bus.oam[1], 0xBB);

        bus.step_oam_dma(4);
        assert_eq!(bus.oam[0], 0x00);
        assert_eq!(bus.oam[1], 0xBB);

        bus.step_oam_dma(4);
        assert_eq!(bus.oam[1], 0x01);
    }

    #[test]
    fn oam_dma_completes_after_160_m_cycles() {
        let mut bus = make_test_bus();
        bus.write_byte(PPU_DMA, 0x00);

        bus.step_oam_dma(8 + (158 * 4));
        assert!(bus.oam_dma_active);

        bus.step_oam_dma(4);
        assert!(!bus.oam_dma_active);
    }

    #[test]
    fn oam_dma_restart_resets_progress_to_byte_zero() {
        let mut bus = make_test_bus();
        bus.write_byte(0xC000, 0x11);
        bus.write_byte(0xC001, 0x22);
        bus.write_byte(0xC100, 0xAA);
        bus.write_byte(0xC101, 0xBB);

        bus.write_byte(PPU_DMA, 0xC0);
        bus.step_oam_dma(8);
        bus.step_oam_dma(4);
        assert_eq!(bus.oam[0], 0x11);
        assert_eq!(bus.oam[1], 0x22);

        bus.write_byte(PPU_DMA, 0xC1);
        assert_eq!(bus.oam_dma_index, 0);

        bus.step_oam_dma(4);
        assert_eq!(bus.oam[0], 0x11);

        bus.step_oam_dma(4);
        assert_eq!(bus.oam[0], 0xAA);
        assert_eq!(bus.oam[1], 0x22);

        bus.step_oam_dma(4);
        assert_eq!(bus.oam[1], 0xBB);
    }

    #[test]
    fn oam_dma_source_reads_ff_from_vram_during_mode_3() {
        let mut bus = make_test_bus();
        bus.vram[0] = 0x5A;
        bus.io.ppu.lcdc |= 0x80;
        bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x03;

        bus.write_byte(PPU_DMA, 0x80);
        bus.step_oam_dma(8);

        assert_eq!(bus.oam[0], 0xFF);
    }

    #[test]
    fn oam_dma_blocks_cpu_access_except_hram() {
        let mut bus = make_test_bus();
        bus.write_byte(PPU_DMA, 0x00);

        assert_eq!(bus.cpu_read_byte(0x0001), 0xFF);
        bus.ie = 0x1F;
        assert_eq!(bus.cpu_read_byte(IE_ADDR), 0xFF);

        bus.cpu_write_byte(0xC000, 0x12);
        assert_ne!(bus.read_byte(0xC000), 0x12);

        bus.cpu_write_byte(IE_ADDR, 0x00);
        assert_eq!(bus.ie, 0x1F);

        bus.cpu_write_byte(HRAM_START, 0x34);
        assert_eq!(bus.cpu_read_byte(HRAM_START), 0x34);
    }
}
