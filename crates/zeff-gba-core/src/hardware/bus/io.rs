use super::super::keypad::KEYINPUT;
use super::*;

const SOUNDBIAS_WRITABLE_MASK: u16 = 0xC3FE;
const BG_PRIORITY_LINE_CARRYOVER_CYCLES: u32 = 96;

impl Bus {
    pub(super) fn io_read8(&self, addr: u32) -> u8 {
        let offset = addr & 0x3FF;
        if offset == 0x84 {
            return self.apu.read_psg(0xFF26) & 0x8F;
        }
        if let Some(psg_addr) = gba_psg_addr(addr) {
            return self.apu.read_psg(psg_addr);
        }

        let aligned = (offset & !1) as usize;
        let value = self.io_read16_value(aligned);
        let shift = ((addr & 1) * 8) as u16;
        (value >> shift) as u8
    }

    pub(crate) fn cpu_read_io16(&mut self, addr: u32) -> u16 {
        self.cpu_read_io16_with_timer_late_cycles(addr, 0)
    }

    pub(crate) fn cpu_read_io16_with_timer_late_cycles(
        &mut self,
        addr: u32,
        timer_late_cycles: u32,
    ) -> u16 {
        let aligned_addr = addr & !1;
        let aligned = (aligned_addr & 0x3FF) as usize;
        let value = match aligned {
            0x100..=0x10F => {
                let timer = ((aligned - 0x100) / 4) as usize;
                let control = (aligned & 0x2) != 0;
                self.timers
                    .cpu_read16_with_late_cycles(timer, control, timer_late_cycles)
            }
            _ => self.io_read16_value(aligned),
        };
        self.record_read(aligned_addr, u32::from(value), 2);
        value
    }

    fn io_read16_value(&self, aligned: usize) -> u16 {
        match aligned {
            0x100..=0x10F => {
                let timer = ((aligned - 0x100) / 4) as usize;
                let control = (aligned & 0x2) != 0;
                self.timers.read16(timer, control)
            }
            0x0B0..=0x0DF => {
                let rel = aligned - 0x0B0;
                self.dma
                    .read16((rel / 12) as usize, ((rel % 12) / 2) as usize)
            }
            DISPSTAT => {
                let mut dispstat = read_io16(&self.io, DISPSTAT);
                if self.ppu.in_vblank() {
                    dispstat |= 1;
                } else {
                    dispstat &= !1;
                }
                if self.ppu.in_hblank() {
                    dispstat |= 1 << 1;
                } else {
                    dispstat &= !(1 << 1);
                }
                if self.ppu.vcount() == (dispstat >> 8) {
                    dispstat |= 1 << 2;
                } else {
                    dispstat &= !(1 << 2);
                }
                dispstat
            }
            VCOUNT => self.ppu.vcount(),
            0x130 => self.keypad.read_keyinput(),
            0x132 => self.keypad.read_keycnt(),
            _ => u16::from_le_bytes([
                self.io.get(aligned).copied().unwrap_or(0),
                self.io.get(aligned + 1).copied().unwrap_or(0),
            ]),
        }
    }

    pub(super) fn io_write8(&mut self, addr: u32, value: u8) {
        let aligned = (addr & !1) & 0x3FF;
        let existing = read_io16(&self.io, aligned as usize);
        let value16 = if addr & 1 == 0 {
            (existing & 0xFF00) | u16::from(value)
        } else {
            (existing & 0x00FF) | (u16::from(value) << 8)
        };
        self.io_write16(0x0400_0000 | aligned, value16);
    }

    pub(super) fn io_write16(&mut self, addr: u32, value: u16) {
        let offset = (addr & 0x3FF) as usize;

        match addr {
            0x0400_0008..=0x0400_000E if addr & 1 == 0 => {
                let old = read_io16(&self.io, offset);
                self.write_io16_raw(offset, value);
                if bg_priority_only_changed(old, value)
                    && self.ppu.in_visible_scanline()
                    && self.ppu.in_hblank()
                {
                    self.ppu.render_current_scanline(
                        &self.io,
                        &self.palette_ram,
                        &self.vram,
                        &self.oam,
                    );
                } else if bg_priority_raised(old, value)
                    && self.ppu.in_visible_scanline()
                    && self.ppu.vcount() > 0
                    && self.ppu.line_cycles() <= BG_PRIORITY_LINE_CARRYOVER_CYCLES
                {
                    self.ppu.render_scanline_index(
                        self.ppu.vcount() - 1,
                        &self.io,
                        &self.palette_ram,
                        &self.vram,
                        &self.oam,
                    );
                }
            }
            0x0400_0004 => {
                self.write_io16_raw(offset, value & 0xFF38);
            }
            0x0400_0100..=0x0400_010F => {
                self.write_io16_raw(offset, value);
                let offset = addr & 0xF;
                let timer = (offset / 4) as usize;
                let control = (offset & 0x2) != 0;
                let disable_rewind_cycles = u32::from(
                    control
                        && value & 0x0080 == 0
                        && !self.interrupt_pending(),
                );
                if std::env::var_os("ZEFF_GBA_TIMER_TRACE").is_some() && control {
                    eprintln!(
                        "TIMER WRITE timer={} value={:04X} clock={} phase64={}",
                        timer,
                        value,
                        self.timer_clock,
                        self.timer_clock % 64
                    );
                }
                self.timers.write16_at_cycle_with_disable_rewind_cycles(
                    timer,
                    control,
                    value,
                    disable_rewind_cycles,
                    self.timer_clock,
                );
            }
            KEYINPUT => {}
            0x0400_0132 => self.keypad.write_keycnt(value),
            0x0400_0200 => {
                self.write_io16_raw(offset, value & 0x3FFF);
                self.test_irq_signal(1);
            }
            0x0400_0202 => {
                let acknowledged = read_io16(&self.io, IF) & value & 0x3FFF;
                self.mirror_acknowledged_interrupts_to_bios_flags(acknowledged);
                let next = read_io16(&self.io, IF) & !value;
                self.write_io16_raw(offset, next & 0x3FFF);
                self.test_irq_signal(1);
            }
            0x0400_0208 => {
                self.write_io16_raw(offset, value & 0x0001);
                self.test_irq_signal(1);
            }
            0x0400_0060..=0x0400_0080 | 0x0400_0090..=0x0400_009E => {
                self.write_gba_psg16(addr, value);
            }
            0x0400_0082 => {
                if value & (1 << 11) != 0 {
                    self.apu.reset_fifo(0);
                }
                if value & (1 << 15) != 0 {
                    self.apu.reset_fifo(1);
                }
                self.write_io16_raw(offset, value & 0x770F);
            }
            0x0400_0084 => {
                self.write_io16_raw(offset, value & 0x0080);
                self.apu.write_psg(0xFF26, (value & 0x0080) as u8);
            }
            0x0400_0088 => {
                self.write_io16_raw(offset, value & SOUNDBIAS_WRITABLE_MASK);
            }
            0x0400_00A0 | 0x0400_00A2 => {
                self.apu.write_fifo_halfword(0, value);
            }
            0x0400_00A4 | 0x0400_00A6 => {
                self.apu.write_fifo_halfword(1, value);
            }
            0x0400_00B0..=0x0400_00DF => {
                self.write_io16_raw(offset, value);
                let rel = addr - 0x0400_00B0;
                let channel = (rel / 12) as usize;
                let reg = ((rel % 12) / 2) as usize;
                let old_control = self.dma.channel(channel).control;
                self.dma.write16(channel, reg, value);
                if reg == 5 && old_control & 0x8000 == 0 && value & 0x8000 != 0 {
                    self.dma.latch_channel(channel);
                    self.try_run_immediate_dma(channel);
                }
            }
            _ => self.write_io16_raw(offset, value),
        }
    }

    pub fn interrupt_pending(&self) -> bool {
        read_io16(&self.io, IME) & 1 != 0
            && (read_io16(&self.io, IE) & read_io16(&self.io, IF) & 0x3FFF) != 0
    }

    pub(crate) fn enabled_interrupt_flags(&self) -> u16 {
        read_io16(&self.io, IE) & read_io16(&self.io, IF) & 0x3FFF
    }

    pub(crate) fn enable_master_interrupts(&mut self) {
        self.write_io16_raw(IME, 1);
        self.test_irq_signal(1);
    }

    pub(crate) fn set_sound_bias_level(&mut self, high: bool) {
        let current = read_io16(&self.io, SOUNDBIAS);
        let level = if high { 0x0200 } else { 0x0000 };
        self.write_io16_raw(SOUNDBIAS, (current & 0xC000) | level);
    }

    pub(crate) fn bios_irq_flags(&self) -> u16 {
        self.read16_raw(BIOS_IRQ_FLAGS)
    }

    pub(crate) fn clear_bios_irq_flags(&mut self, flags: u16) {
        let next = self.bios_irq_flags() & !(flags & 0x3FFF);
        self.write16_raw(BIOS_IRQ_FLAGS, next.to_le_bytes());
    }

    pub fn irq_handler_installed(&self) -> bool {
        let raw_addr = self.read32(0x03FF_FFFC);
        let addr = raw_addr & !3;
        if !matches!(
            addr,
            0x0200_0000..=0x03FF_FFFF | 0x0800_0000..=0x0DFF_FFFF
        ) {
            return false;
        }
        !matches!(self.read32(addr), 0 | 0xFFFF_FFFF)
    }

    pub(crate) fn request_interrupt(&mut self, flags: u16) {
        let next = read_io16(&self.io, IF) | (flags & 0x3FFF);
        self.write_io16_raw(IF, next);
        self.test_irq_signal(0);
    }

    pub(crate) fn request_timer_interrupts(
        &mut self,
        flags: u16,
        extra_delays: [u32; 4],
        cycles_late: [u32; 4],
    ) {
        let timer_flags = flags & 0x0078;
        if timer_flags == 0 {
            self.request_interrupt(flags);
            return;
        }

        let next = read_io16(&self.io, IF) | (flags & 0x3FFF);
        self.write_io16_raw(IF, next);

        let mut min_delay = None;
        for (timer, (extra_delay, cycles_late)) in extra_delays
            .into_iter()
            .zip(cycles_late.into_iter())
            .enumerate()
        {
            if timer_flags & (1 << (3 + timer)) != 0 {
                let delay = IRQ_DELAY_CYCLES
                    .saturating_add(extra_delay)
                    .saturating_sub(cycles_late);
                min_delay = Some(min_delay.map_or(delay, |current: u32| current.min(delay)));
            }
        }
        if self.irq_line_asserted()
            && let Some(delay) = min_delay
            && self
                .irq_delay_cycles
                .is_none_or(|current| delay < current)
        {
            self.irq_delay_cycles = Some(delay);
        }
    }

    fn mirror_acknowledged_interrupts_to_bios_flags(&mut self, flags: u16) {
        if flags == 0 {
            return;
        }
        let next = self.bios_irq_flags() | (flags & 0x3FFF);
        self.write16_raw(BIOS_IRQ_FLAGS, next.to_le_bytes());
    }

    pub(super) fn update_lcd_interrupts(
        &mut self,
        was_in_vblank: bool,
        was_in_hblank: bool,
        old_vcount: u16,
    ) {
        let dispstat = read_io16(&self.io, DISPSTAT);
        if !was_in_vblank && self.ppu.in_vblank() && dispstat & (1 << 3) != 0 {
            self.request_interrupt(INT_VBLANK);
        }
        if !was_in_hblank && self.ppu.in_hblank() && dispstat & (1 << 4) != 0 {
            self.request_interrupt(INT_HBLANK);
        }

        let lyc = dispstat >> 8;
        if old_vcount != self.ppu.vcount() && self.ppu.vcount() == lyc && dispstat & (1 << 5) != 0 {
            self.request_interrupt(INT_VCOUNT);
        }
    }

    pub(super) fn write_io16_raw(&mut self, offset: usize, value: u16) {
        if offset < self.io.len() {
            let bytes = value.to_le_bytes();
            self.io[offset] = bytes[0];
            if offset + 1 < self.io.len() {
                self.io[offset + 1] = bytes[1];
            }
        }
    }

    fn write_gba_psg16(&mut self, addr: u32, value: u16) {
        let offset = (addr & 0x3FF) as usize;
        for (i, byte) in value.to_le_bytes().into_iter().enumerate() {
            let byte_addr = addr.wrapping_add(i as u32);
            if let Some(psg_addr) = gba_psg_addr(byte_addr) {
                self.apu.write_psg(psg_addr, byte);
            }
            if offset + i < self.io.len() {
                self.io[offset + i] = byte;
            }
        }
    }

    pub(super) fn clear_io_range(&mut self, start: usize, end: usize) {
        let end = end.min(self.io.len().saturating_sub(1));
        if start <= end {
            self.io[start..=end].fill(0);
        }
    }
}

fn bg_priority_only_changed(old: u16, new: u16) -> bool {
    old & 0x3 != new & 0x3 && old & !0x3 == new & !0x3
}

fn bg_priority_raised(old: u16, new: u16) -> bool {
    bg_priority_only_changed(old, new) && (new & 0x3) < (old & 0x3)
}

fn gba_psg_addr(addr: u32) -> Option<u16> {
    match addr & 0x3FF {
        0x060 => Some(0xFF10),
        0x062 => Some(0xFF11),
        0x063 => Some(0xFF12),
        0x064 => Some(0xFF13),
        0x065 => Some(0xFF14),
        0x068 => Some(0xFF16),
        0x069 => Some(0xFF17),
        0x06C => Some(0xFF18),
        0x06D => Some(0xFF19),
        0x070 => Some(0xFF1A),
        0x071 => Some(0xFF1B),
        0x072 => Some(0xFF1C),
        0x073 => Some(0xFF1D),
        0x074 => Some(0xFF1E),
        0x078 => Some(0xFF20),
        0x079 => Some(0xFF21),
        0x07C => Some(0xFF22),
        0x07D => Some(0xFF23),
        0x080 => Some(0xFF24),
        0x081 => Some(0xFF25),
        0x090..=0x09F => Some(0xFF30 + ((addr & 0xF) as u16)),
        _ => None,
    }
}
