use super::apu::Apu;
use super::cartridge::Cartridge;
use super::constants::{EWRAM_SIZE, IO_SIZE, IWRAM_SIZE, OAM_SIZE, PALETTE_RAM_SIZE, VRAM_SIZE};
use super::dma::DmaController;
use super::keypad::Keypad;
use super::ppu::{Ppu, PpuDebugSnapshot};
use super::timer::Timers;
use std::cell::RefCell;

mod dma;
mod io;

const DISPSTAT: usize = 0x004;
const VCOUNT: usize = 0x006;
const SOUNDBIAS: usize = 0x088;
const IE: usize = 0x200;
const IF: usize = 0x202;
const IME: usize = 0x208;
const BIOS_IRQ_FLAGS: u32 = 0x0300_7FF8;

const INT_VBLANK: u16 = 1 << 0;
const INT_HBLANK: u16 = 1 << 1;
const INT_VCOUNT: u16 = 1 << 2;
const IRQ_DELAY_CYCLES: u32 = 7;
const IRQ_SAMPLE_LOOKAHEAD_CYCLES: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugTraceEvent {
    Read {
        addr: u32,
        value: u32,
        width: u8,
    },
    Write {
        addr: u32,
        old_value: u32,
        new_value: u32,
        width: u8,
    },
}

#[derive(Clone, Debug)]
pub struct Bus {
    pub cartridge: Cartridge,
    pub ppu: Ppu,
    pub apu: Apu,
    pub keypad: Keypad,
    pub timers: Timers,
    pub dma: DmaController,
    pub ewram: Vec<u8>,
    pub iwram: Vec<u8>,
    pub io: Vec<u8>,
    pub palette_ram: Vec<u8>,
    pub vram: Vec<u8>,
    pub oam: Vec<u8>,
    pending_dma_cycles: u32,
    irq_delay_cycles: Option<u32>,
    pub(crate) debug_trace_enabled: bool,
    pub(crate) debug_trace_reads: bool,
    pub(crate) debug_trace_writes: bool,
    pub(crate) debug_trace_events: RefCell<Vec<DebugTraceEvent>>,
}

impl Bus {
    pub fn new(cartridge: Cartridge, sample_rate: u32) -> Self {
        let mut bus = Self {
            cartridge,
            ppu: Ppu::new(),
            apu: Apu::new(sample_rate),
            keypad: Keypad::new(),
            timers: Timers::default(),
            dma: DmaController::default(),
            ewram: vec![0; EWRAM_SIZE],
            iwram: vec![0; IWRAM_SIZE],
            io: vec![0; IO_SIZE],
            palette_ram: vec![0; PALETTE_RAM_SIZE],
            vram: vec![0; VRAM_SIZE],
            oam: vec![0; OAM_SIZE],
            pending_dma_cycles: 0,
            irq_delay_cycles: None,
            debug_trace_enabled: false,
            debug_trace_reads: false,
            debug_trace_writes: false,
            debug_trace_events: RefCell::new(Vec::new()),
        };
        bus.write_io16_raw(SOUNDBIAS, 0x0200);
        bus
    }

    pub fn read8(&self, addr: u32) -> u8 {
        let value = self.read8_raw(addr);
        self.record_read(addr, u32::from(value), 1);
        value
    }

    fn read8_raw(&self, addr: u32) -> u8 {
        match addr {
            0x0000_0000..=0x0000_3FFF => bios_stub_read8(addr),
            0x0200_0000..=0x02FF_FFFF => self.ewram[(addr as usize) & (EWRAM_SIZE - 1)],
            0x0300_0000..=0x03FF_FFFF => self.iwram[(addr as usize) & (IWRAM_SIZE - 1)],
            0x0400_0000..=0x0400_03FF => self.io_read8(addr),
            0x0500_0000..=0x05FF_FFFF => self.palette_ram[(addr as usize) & (PALETTE_RAM_SIZE - 1)],
            0x0600_0000..=0x06FF_FFFF => self.vram[vram_index(addr)],
            0x0700_0000..=0x07FF_FFFF => self.oam[(addr as usize) & (OAM_SIZE - 1)],
            0x0800_0000..=0x0DFF_FFFF => self.cartridge.rom_read8(addr),
            0x0E00_0000..=0x0FFF_FFFF => self.cartridge.backup_read8(addr),
            _ => 0xFF,
        }
    }

    pub fn read16(&self, addr: u32) -> u16 {
        if is_backup_addr(addr) {
            let byte = self.cartridge.backup_read8(addr);
            let value = u16::from_le_bytes([byte, byte]);
            self.record_read(addr, u32::from(value), 2);
            return value;
        }
        let aligned = addr & !1;
        if self.cartridge.is_eeprom_access_addr(aligned) {
            let value = self.cartridge.eeprom_read16(aligned);
            self.record_read(aligned, u32::from(value), 2);
            return value;
        }
        let value = u16::from_le_bytes([self.read8_raw(aligned), self.read8_raw(aligned + 1)]);
        self.record_read(aligned, u32::from(value), 2);
        value
    }

    fn read16_raw(&self, addr: u32) -> u16 {
        if is_backup_addr(addr) {
            let byte = self.cartridge.backup_read8(addr);
            return u16::from_le_bytes([byte, byte]);
        }
        let aligned = addr & !1;
        u16::from_le_bytes([self.read8_raw(aligned), self.read8_raw(aligned + 1)])
    }

    pub fn read32(&self, addr: u32) -> u32 {
        if is_backup_addr(addr) {
            let byte = self.cartridge.backup_read8(addr);
            let value = u32::from_le_bytes([byte, byte, byte, byte]);
            self.record_read(addr, value, 4);
            return value;
        }
        let aligned = addr & !3;
        let value = u32::from_le_bytes([
            self.read8_raw(aligned),
            self.read8_raw(aligned + 1),
            self.read8_raw(aligned + 2),
            self.read8_raw(aligned + 3),
        ]);
        self.record_read(aligned, value, 4);
        value
    }

    fn read32_raw(&self, addr: u32) -> u32 {
        if is_backup_addr(addr) {
            let byte = self.cartridge.backup_read8(addr);
            return u32::from_le_bytes([byte, byte, byte, byte]);
        }
        let aligned = addr & !3;
        u32::from_le_bytes([
            self.read8_raw(aligned),
            self.read8_raw(aligned + 1),
            self.read8_raw(aligned + 2),
            self.read8_raw(aligned + 3),
        ])
    }

    pub fn write8(&mut self, addr: u32, value: u8) {
        let old_value = self.read8_raw(addr);
        self.write8_raw(addr, value);
        self.record_write(
            addr,
            u32::from(old_value),
            u32::from(self.read8_raw(addr)),
            1,
        );
    }

    fn write8_raw(&mut self, addr: u32, value: u8) {
        match addr {
            0x0200_0000..=0x02FF_FFFF => self.ewram[(addr as usize) & (EWRAM_SIZE - 1)] = value,
            0x0300_0000..=0x03FF_FFFF => self.iwram[(addr as usize) & (IWRAM_SIZE - 1)] = value,
            0x0400_0000..=0x0400_03FF => self.io_write8(addr, value),
            0x0500_0000..=0x05FF_FFFF => {
                write_repeated_video_byte(&mut self.palette_ram, addr as usize, value);
            }
            0x0600_0000..=0x06FF_FFFF => {
                let index = vram_index(addr);
                if vram_byte_write_hits_bg(index, read_io16(&self.io, 0)) {
                    write_repeated_video_byte(&mut self.vram, index, value);
                }
            }
            0x0700_0000..=0x07FF_FFFF => {}
            0x0E00_0000..=0x0FFF_FFFF => self.cartridge.backup_write8(addr, value),
            _ => {}
        }
    }

    pub fn write16(&mut self, addr: u32, value: u16) {
        if is_backup_addr(addr) {
            let old_value = u32::from(self.cartridge.backup_read8(addr));
            let byte = value.to_le_bytes()[(addr & 1) as usize];
            self.cartridge.backup_write8(addr, byte);
            self.record_write(
                addr,
                old_value,
                u32::from(self.cartridge.backup_read8(addr)),
                2,
            );
            return;
        }
        let aligned = addr & !1;
        if self.cartridge.is_eeprom_access_addr(aligned) {
            self.cartridge.eeprom_write16(aligned, value);
            self.record_write(aligned, 0xFFFF, u32::from(value & 1), 2);
            return;
        }
        let old_value = self.read16_raw(aligned);
        if matches!(aligned, 0x0400_0000..=0x0400_03FF) {
            self.io_write16(aligned, value);
            let new_value = if is_sound_fifo_register(aligned) {
                u32::from(value)
            } else {
                u32::from(self.read16_raw(aligned))
            };
            self.record_write(aligned, u32::from(old_value), new_value, 2);
            return;
        }
        let bytes = value.to_le_bytes();
        self.write16_raw(aligned, bytes);
        self.record_write(
            aligned,
            u32::from(old_value),
            u32::from(self.read16_raw(aligned)),
            2,
        );
    }

    pub fn write32(&mut self, addr: u32, value: u32) {
        if is_backup_addr(addr) {
            let old_value = u32::from(self.cartridge.backup_read8(addr));
            let byte = value.to_le_bytes()[(addr & 3) as usize];
            self.cartridge.backup_write8(addr, byte);
            self.record_write(
                addr,
                old_value,
                u32::from(self.cartridge.backup_read8(addr)),
                4,
            );
            return;
        }
        let aligned = addr & !3;
        let old_value = self.read32_raw(aligned);
        if matches!(aligned, 0x0400_0000..=0x0400_03FF) {
            self.io_write16(aligned, value as u16);
            self.io_write16(aligned + 2, (value >> 16) as u16);
            let new_value = if is_sound_fifo_register(aligned) {
                value
            } else {
                self.read32_raw(aligned)
            };
            self.record_write(aligned, old_value, new_value, 4);
            return;
        }
        let bytes = value.to_le_bytes();
        self.write32_raw(aligned, bytes);
        self.record_write(aligned, old_value, self.read32_raw(aligned), 4);
    }

    fn write16_raw(&mut self, addr: u32, bytes: [u8; 2]) {
        match addr {
            0x0200_0000..=0x02FF_FFFF => {
                let index = (addr as usize) & (EWRAM_SIZE - 1);
                self.ewram[index] = bytes[0];
                self.ewram[(index + 1) & (EWRAM_SIZE - 1)] = bytes[1];
            }
            0x0300_0000..=0x03FF_FFFF => {
                let index = (addr as usize) & (IWRAM_SIZE - 1);
                self.iwram[index] = bytes[0];
                self.iwram[(index + 1) & (IWRAM_SIZE - 1)] = bytes[1];
            }
            0x0500_0000..=0x05FF_FFFF => {
                let index = (addr as usize) & (PALETTE_RAM_SIZE - 1);
                self.palette_ram[index] = bytes[0];
                self.palette_ram[(index + 1) & (PALETTE_RAM_SIZE - 1)] = bytes[1];
            }
            0x0600_0000..=0x06FF_FFFF => {
                for (offset, byte) in bytes.into_iter().enumerate() {
                    self.vram[vram_index(addr + offset as u32)] = byte;
                }
            }
            0x0700_0000..=0x07FF_FFFF => {
                let index = (addr as usize) & (OAM_SIZE - 1);
                self.oam[index] = bytes[0];
                self.oam[(index + 1) & (OAM_SIZE - 1)] = bytes[1];
            }
            0x0E00_0000..=0x0FFF_FFFF => {
                self.cartridge.backup_write8(addr, bytes[0]);
                self.cartridge.backup_write8(addr.wrapping_add(1), bytes[1]);
            }
            _ => {}
        }
    }

    fn write32_raw(&mut self, addr: u32, bytes: [u8; 4]) {
        match addr {
            0x0200_0000..=0x02FF_FFFF => {
                let index = (addr as usize) & (EWRAM_SIZE - 1);
                for (offset, byte) in bytes.into_iter().enumerate() {
                    self.ewram[(index + offset) & (EWRAM_SIZE - 1)] = byte;
                }
            }
            0x0300_0000..=0x03FF_FFFF => {
                let index = (addr as usize) & (IWRAM_SIZE - 1);
                for (offset, byte) in bytes.into_iter().enumerate() {
                    self.iwram[(index + offset) & (IWRAM_SIZE - 1)] = byte;
                }
            }
            0x0500_0000..=0x05FF_FFFF => {
                let index = (addr as usize) & (PALETTE_RAM_SIZE - 1);
                for (offset, byte) in bytes.into_iter().enumerate() {
                    self.palette_ram[(index + offset) & (PALETTE_RAM_SIZE - 1)] = byte;
                }
            }
            0x0600_0000..=0x06FF_FFFF => {
                for (offset, byte) in bytes.into_iter().enumerate() {
                    self.vram[vram_index(addr + offset as u32)] = byte;
                }
            }
            0x0700_0000..=0x07FF_FFFF => {
                let index = (addr as usize) & (OAM_SIZE - 1);
                for (offset, byte) in bytes.into_iter().enumerate() {
                    self.oam[(index + offset) & (OAM_SIZE - 1)] = byte;
                }
            }
            0x0E00_0000..=0x0FFF_FFFF => {
                for (offset, byte) in bytes.into_iter().enumerate() {
                    self.cartridge
                        .backup_write8(addr.wrapping_add(offset as u32), byte);
                }
            }
            _ => {}
        }
    }

    pub fn step_cycles(&mut self, mut cycles: u32) {
        while cycles > 0 {
            let was_in_vblank = self.ppu.in_vblank();
            let was_in_hblank = self.ppu.in_hblank();
            let old_vcount = self.ppu.vcount();
            let soundcnt_h = read_io16(&self.io, 0x82);
            let mut step = cycles
                .min(self.ppu.cycles_until_next_status_event().max(1))
                .min(self.cycles_until_next_direct_sound_overflow(soundcnt_h))
                .min(self.cycles_until_irq_event());
            for timer in 0..4 {
                if let Some(next) = self.timers.cycles_until_overflow(timer) {
                    step = step.min(next.max(1));
                }
            }

            self.step_irq_event(step);
            self.ppu.step_cycles(step);
            self.cartridge.step_cycles(step);
            cycles -= step;

            if !was_in_hblank && self.ppu.in_hblank() && self.ppu.in_visible_scanline() {
                self.ppu.render_current_scanline(
                    &self.io,
                    &self.palette_ram,
                    &self.vram,
                    &self.oam,
                );
                self.run_dma_start_timing(2);
            }
            if !was_in_vblank && self.ppu.in_vblank() {
                self.ppu.mark_frame_ready();
                self.run_dma_start_timing(1);
            }
            self.update_lcd_interrupts(was_in_vblank, was_in_hblank, old_vcount);
            self.apu.step_output(
                step,
                soundcnt_h,
                read_io16(&self.io, 0x84),
                read_io16(&self.io, SOUNDBIAS),
            );
            let (timer_interrupts, timer_overflows, timer_irq_extra_delays) =
                self.timers.step_with_overflows(step);
            if timer_overflows.iter().any(|&count| count != 0) {
                self.service_sound_timer_overflows(timer_overflows, soundcnt_h);
            }
            if timer_interrupts != 0 {
                self.request_timer_interrupts(timer_interrupts, timer_irq_extra_delays);
            }
        }
    }

    pub fn take_pending_dma_cycles(&mut self) -> u32 {
        std::mem::take(&mut self.pending_dma_cycles)
    }

    pub fn cycles_until_next_halt_check(&self) -> u32 {
        let mut cycles = 64;
        cycles = cycles.min(self.ppu.cycles_until_next_status_event().max(1));
        cycles = cycles.min(self.cycles_until_irq_event());
        for timer in 0..4 {
            if let Some(next) = self.timers.cycles_until_overflow(timer) {
                cycles = cycles.min(next.max(1));
            }
        }
        cycles.max(1)
    }

    pub(crate) fn interrupt_ready(&self) -> bool {
        self.interrupt_pending()
            && self
                .irq_delay_cycles
                .is_some_and(|cycles| cycles <= IRQ_SAMPLE_LOOKAHEAD_CYCLES)
    }

    pub(crate) fn take_irq_sample_delay_cycles(&mut self) -> u32 {
        let Some(cycles) = self.irq_delay_cycles else {
            return 0;
        };
        if cycles > IRQ_SAMPLE_LOOKAHEAD_CYCLES {
            return 0;
        }
        self.irq_delay_cycles = Some(0);
        cycles
    }

    pub(crate) fn test_irq_signal(&mut self, cycles_late: u32) {
        self.test_irq_signal_with_extra_delay(cycles_late, 0);
    }

    pub(crate) fn test_irq_signal_with_extra_delay(&mut self, cycles_late: u32, extra_delay: u32) {
        if self.irq_line_asserted() && self.irq_delay_cycles.is_none() {
            self.irq_delay_cycles = Some(
                IRQ_DELAY_CYCLES
                    .saturating_add(extra_delay)
                    .saturating_sub(cycles_late),
            );
        }
    }

    fn cycles_until_irq_event(&self) -> u32 {
        self.irq_delay_cycles
            .filter(|&cycles| cycles > 0)
            .unwrap_or(u32::MAX)
    }

    fn step_irq_event(&mut self, cycles: u32) {
        if let Some(delay) = self.irq_delay_cycles {
            let next = delay.saturating_sub(cycles);
            self.irq_delay_cycles = if next == 0 && !self.irq_line_asserted() {
                None
            } else {
                Some(next)
            };
        }
    }

    fn irq_line_asserted(&self) -> bool {
        read_io16(&self.io, IE) & read_io16(&self.io, IF) & 0x3FFF != 0
    }

    pub fn render_frame(&mut self) {
        self.ppu
            .step_frame(&self.io, &self.palette_ram, &self.vram, &self.oam);
    }

    pub fn ppu_debug_snapshot(&self) -> PpuDebugSnapshot {
        self.ppu.debug_snapshot(&self.io)
    }

    pub fn set_ppu_debug_flags(&mut self, bg: bool, window: bool, sprites: bool) {
        self.ppu.set_debug_flags(bg, window, sprites);
    }

    pub fn set_ppu_debug_bg_layers(&mut self, layers: [bool; 4]) {
        self.ppu.set_debug_bg_layers(layers);
    }

    pub fn system_ram(&self) -> (&[u8], &[u8]) {
        (&self.ewram, &self.iwram)
    }

    pub fn register_ram_reset(&mut self, flags: u8) {
        if flags & (1 << 0) != 0 {
            self.ewram.fill(0);
        }
        if flags & (1 << 1) != 0 {
            let clear_len = IWRAM_SIZE.saturating_sub(0x200);
            self.iwram[..clear_len].fill(0);
        }
        if flags & (1 << 2) != 0 {
            self.palette_ram.fill(0);
        }
        if flags & (1 << 3) != 0 {
            self.vram.fill(0);
        }
        if flags & (1 << 4) != 0 {
            self.oam.fill(0);
        }

        if flags & (1 << 7) != 0 {
            let sample_rate = self.apu.sample_rate();
            self.io.fill(0);
            self.dma = DmaController::default();
            self.timers = Timers::default();
            self.apu = Apu::new(sample_rate);
            self.pending_dma_cycles = 0;
        } else {
            if flags & (1 << 5) != 0 {
                self.clear_io_range(0x120, 0x15F);
            }
            if flags & (1 << 6) != 0 {
                self.clear_io_range(0x060, 0x0A7);
            }
        }

        self.write_io16_raw(0, 0x0080);
    }

    fn record_write(&mut self, addr: u32, old_value: u32, new_value: u32, width: u8) {
        if self.debug_trace_enabled && self.debug_trace_writes {
            self.debug_trace_events
                .borrow_mut()
                .push(DebugTraceEvent::Write {
                    addr,
                    old_value,
                    new_value,
                    width,
                });
        }
    }

    fn record_read(&self, addr: u32, value: u32, width: u8) {
        if self.debug_trace_enabled && self.debug_trace_reads {
            self.debug_trace_events
                .borrow_mut()
                .push(DebugTraceEvent::Read { addr, value, width });
        }
    }
}

fn read_io16(io: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([
        io.get(offset).copied().unwrap_or(0),
        io.get(offset + 1).copied().unwrap_or(0),
    ])
}

fn bios_stub_read8(addr: u32) -> u8 {
    const IRQ_VECTOR: u32 = 0x18;
    const IRQ_HANDLER: u32 = 0x128;
    const IRQ_VECTOR_BRANCH: u32 = 0xEA00_0042;
    const IRQ_HANDLER_WORDS: [u32; 8] = [
        0xE92D_500F, // stmfd sp!, {r0-r3,r12,lr}
        0xE3A0_0404, // mov r0, #0x04000000
        0xE28F_E000, // add lr, pc, #0
        0xE510_F004, // ldr pc, [r0, #-4]
        0xE8BD_500F, // ldmfd sp!, {r0-r3,r12,lr}
        0xE25E_F004, // subs pc, lr, #4
        0xE92D_5800, // stmfd sp!, {r11,r12,lr}; prefetched after IRQ return
        0xE55E_C002, // ldrb r12, [lr, #-2]; protected-read latch after IRQ return
    ];

    let word = if (IRQ_VECTOR..IRQ_VECTOR + 4).contains(&addr) {
        Some(IRQ_VECTOR_BRANCH)
    } else if (IRQ_HANDLER..IRQ_HANDLER + IRQ_HANDLER_WORDS.len() as u32 * 4).contains(&addr) {
        let index = ((addr - IRQ_HANDLER) / 4) as usize;
        Some(IRQ_HANDLER_WORDS[index])
    } else {
        None
    };

    word.map(|value| value.to_le_bytes()[(addr & 3) as usize])
        .unwrap_or(0)
}

fn is_sound_fifo_register(addr: u32) -> bool {
    matches!(addr & 0x03FF, 0x0A0 | 0x0A2 | 0x0A4 | 0x0A6)
}

fn is_backup_addr(addr: u32) -> bool {
    matches!(addr, 0x0E00_0000..=0x0FFF_FFFF)
}

fn write_repeated_video_byte(memory: &mut [u8], addr: usize, value: u8) {
    let index = (addr % memory.len()) & !1;
    memory[index] = value;
    memory[index + 1] = value;
}

fn vram_byte_write_hits_bg(index: usize, dispcnt: u16) -> bool {
    let bitmap_mode = dispcnt & 0x7 >= 3;
    index < if bitmap_mode { 0x14000 } else { 0x10000 }
}

fn vram_index(addr: u32) -> usize {
    let mut offset = ((addr - 0x0600_0000) as usize) & 0x1FFFF;
    if offset >= VRAM_SIZE {
        offset -= 0x8000;
    }
    offset
}

#[cfg(test)]
#[path = "bus/tests.rs"]
mod tests;
