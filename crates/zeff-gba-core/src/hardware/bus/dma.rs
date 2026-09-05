use super::super::constants::{GAMEPAK_ROM_END, GAMEPAK0_START};
use super::super::dma::DmaChannel;
use super::super::timer::TimerOverflowCounts;
use super::super::timing::{AccessType, access_cycles_with_waitcnt};
use super::*;

const INT_DMA0: u16 = 1 << 8;

impl Bus {
    pub(super) fn try_run_immediate_dma(&mut self, channel: usize) {
        let mut ch = self.dma.channel(channel);
        if (ch.control >> 12) & 0x3 != 0 {
            return;
        }
        let dest_mode = (ch.control >> 5) & 0x3;
        self.run_dma(channel, &mut ch);
        self.finish_dma_transfer(channel, ch, dest_mode);
    }

    pub(super) fn run_dma_start_timing(&mut self, start_timing: u16) {
        for channel in 0..4 {
            let mut ch = self.dma.channel(channel);
            if ch.control & 0x8000 == 0 || (ch.control >> 12) & 0x3 != start_timing {
                continue;
            }
            let dest_mode = (ch.control >> 5) & 0x3;
            self.run_dma(channel, &mut ch);
            self.finish_dma_transfer(channel, ch, dest_mode);
        }
    }

    fn finish_dma_transfer(&mut self, channel: usize, mut ch: DmaChannel, dest_mode: u16) {
        let repeat = ch.control & (1 << 9) != 0;
        if ch.control & (1 << 14) != 0 {
            self.request_interrupt(INT_DMA0 << channel);
        }
        if repeat {
            ch.active_count = ch.count;
            if dest_mode == 3 {
                ch.active_destination = ch.destination;
            }
        } else {
            ch.control &= !0x8000;
        }
        self.dma.set_channel(channel, ch);
    }

    fn run_sound_fifo_dma(&mut self, fifo: usize) {
        let fifo_addr = if fifo == 0 { 0x0400_00A0 } else { 0x0400_00A4 };
        for channel in 1..=2 {
            let mut ch = self.dma.channel(channel);
            if ch.control & 0x8000 == 0 || (ch.control >> 12) & 0x3 != 3 {
                continue;
            }
            if ch.active_destination & !3 != fifo_addr {
                continue;
            }

            let src_mode = (ch.control >> 7) & 0x3;
            let mut src = ch.active_source;
            let mut transfer_cycles = 0u32;
            #[cfg(feature = "profiling")]
            {
                self.profiling.dma_starts[channel] =
                    self.profiling.dma_starts[channel].wrapping_add(1);
                self.profiling.dma_units[channel] =
                    self.profiling.dma_units[channel].wrapping_add(4);
            }
            let waitcnt = self.waitcnt();
            for index in 0..4 {
                let access_type = if index == 0 {
                    AccessType::NonSequential
                } else {
                    AccessType::Sequential
                };
                transfer_cycles = transfer_cycles.saturating_add(access_cycles_with_waitcnt(
                    src,
                    4,
                    access_type,
                    waitcnt,
                ));
                transfer_cycles = transfer_cycles.saturating_add(access_cycles_with_waitcnt(
                    fifo_addr,
                    4,
                    access_type,
                    waitcnt,
                ));
                let value = self.read32(src);
                self.write32(fifo_addr, value);
                src = step_dma_addr(src, src_mode, 4);
            }
            self.pending_dma_cycles = self
                .pending_dma_cycles
                .saturating_add(transfer_cycles.saturating_add(2));
            ch.active_source = src;
            ch.active_count = ch.count;
            if ch.control & (1 << 14) != 0 {
                self.request_interrupt(INT_DMA0 << channel);
            }
            if ch.control & (1 << 9) == 0 {
                ch.control &= !0x8000;
            }
            self.dma.set_channel(channel, ch);
            break;
        }
    }

    pub(super) fn service_sound_timer_overflows(
        &mut self,
        timer_overflows: TimerOverflowCounts,
        soundcnt_h: u16,
    ) {
        for (timer, count) in timer_overflows.into_iter().enumerate().take(2) {
            for _ in 0..count {
                let requests = self.apu.on_timer_overflow(timer, soundcnt_h);
                if requests.a {
                    self.run_sound_fifo_dma(0);
                }
                if requests.b {
                    self.run_sound_fifo_dma(1);
                }
            }
        }
    }

    fn run_dma(&mut self, channel: usize, ch: &mut DmaChannel) {
        let word = ch.control & (1 << 10) != 0;
        let unit = if word { 4 } else { 2 };
        let width = unit as u8;
        let mut count = u32::from(ch.count);
        if count == 0 {
            count = if channel == 3 { 0x1_0000 } else { 0x4000 };
        }
        #[cfg(feature = "profiling")]
        {
            self.profiling.dma_starts[channel] = self.profiling.dma_starts[channel].wrapping_add(1);
            self.profiling.dma_units[channel] =
                self.profiling.dma_units[channel].wrapping_add(u64::from(count));
        }

        let dest_mode = (ch.control >> 5) & 0x3;
        let configured_src_mode = (ch.control >> 7) & 0x3;
        let mut src = ch.active_source;
        let mut dst = ch.active_destination;
        let mut transfer_cycles = 0u32;
        if channel == 3 && !word && self.cartridge.is_eeprom_access_addr(dst) {
            let mut bits = Vec::with_capacity(count as usize);
            let waitcnt = self.waitcnt();
            for index in 0..count {
                let access_type = if index == 0 {
                    AccessType::NonSequential
                } else {
                    AccessType::Sequential
                };
                transfer_cycles = transfer_cycles.saturating_add(access_cycles_with_waitcnt(
                    src,
                    width,
                    access_type,
                    waitcnt,
                ));
                transfer_cycles = transfer_cycles.saturating_add(access_cycles_with_waitcnt(
                    dst,
                    width,
                    access_type,
                    waitcnt,
                ));

                let value = self.read16(src);
                bits.push((value & 1) as u8);
                self.record_write(dst, 0xFFFF, u32::from(value & 1), 2);
                src = step_dma_addr(src, configured_src_mode, unit);
                dst = step_dma_addr(dst, dest_mode, unit);
            }
            self.cartridge
                .eeprom_write_bits(ch.active_destination, &bits);
            self.pending_dma_cycles = self
                .pending_dma_cycles
                .saturating_add(transfer_cycles.saturating_add(2));
            ch.active_source = src;
            ch.active_destination = dst;
            ch.active_count = 0;
            return;
        }
        if channel == 3 && !word && self.cartridge.is_eeprom_access_addr(src) {
            let waitcnt = self.waitcnt();
            for index in 0..count {
                let access_type = if index == 0 {
                    AccessType::NonSequential
                } else {
                    AccessType::Sequential
                };
                transfer_cycles = transfer_cycles.saturating_add(access_cycles_with_waitcnt(
                    src,
                    width,
                    access_type,
                    waitcnt,
                ));
                transfer_cycles = transfer_cycles.saturating_add(access_cycles_with_waitcnt(
                    dst,
                    width,
                    access_type,
                    waitcnt,
                ));

                let value = self.cartridge.eeprom_read16(src);
                self.record_read(src, u32::from(value), 2);
                self.write16(dst, value);
                src = step_dma_addr(src, configured_src_mode, unit);
                dst = step_dma_addr(dst, dest_mode, unit);
            }
            self.pending_dma_cycles = self
                .pending_dma_cycles
                .saturating_add(transfer_cycles.saturating_add(2));
            ch.active_source = src;
            ch.active_destination = dst;
            ch.active_count = 0;
            return;
        }
        let waitcnt = self.waitcnt();
        for index in 0..count {
            let access_type = if index == 0 {
                AccessType::NonSequential
            } else {
                AccessType::Sequential
            };
            transfer_cycles = transfer_cycles.saturating_add(access_cycles_with_waitcnt(
                src,
                width,
                access_type,
                waitcnt,
            ));
            transfer_cycles = transfer_cycles.saturating_add(access_cycles_with_waitcnt(
                dst,
                width,
                access_type,
                waitcnt,
            ));

            let read_addr = dma_source_addr(channel, src) & !(unit - 1);
            let write_addr = dma_destination_addr(channel, dst) & !(unit - 1);
            if word {
                let source_uses_latch = dma_source_uses_latch(read_addr);
                let value = if source_uses_latch {
                    ch.data_latch
                } else {
                    self.read32(read_addr)
                };
                if !source_uses_latch {
                    ch.data_latch = value;
                }
                self.write32(write_addr, value);
            } else {
                let source_uses_latch = dma_source_uses_latch(read_addr);
                let value = if source_uses_latch {
                    ch.data_latch as u16
                } else {
                    self.read16(read_addr)
                };
                if !source_uses_latch {
                    ch.data_latch = u32::from(value) | (u32::from(value) << 16);
                }
                self.write16(write_addr, value);
            }
            src = step_dma_addr(
                src,
                effective_dma_source_step_mode(channel, src, configured_src_mode),
                unit,
            );
            dst = step_dma_addr(dst, dest_mode, unit);
        }
        self.pending_dma_cycles = self
            .pending_dma_cycles
            .saturating_add(transfer_cycles.saturating_add(2));
        ch.active_source = src;
        ch.active_destination = dst;
        ch.active_count = 0;
    }
}

fn dma_source_addr(channel: usize, addr: u32) -> u32 {
    if channel == 0 {
        addr & 0x07FF_FFFF
    } else {
        addr & 0x0FFF_FFFF
    }
}

fn dma_destination_addr(channel: usize, addr: u32) -> u32 {
    if channel == 3 {
        addr & 0x0FFF_FFFF
    } else {
        addr & 0x07FF_FFFF
    }
}

fn dma_source_uses_latch(addr: u32) -> bool {
    matches!(addr, 0x0000_0000..=0x01FF_FFFF | 0x1000_0000..=0xFFFF_FFFF)
}

fn effective_dma_source_step_mode(channel: usize, addr: u32, configured_mode: u16) -> u16 {
    if channel != 0 && matches!(addr, GAMEPAK0_START..=GAMEPAK_ROM_END) {
        0
    } else {
        configured_mode
    }
}

fn step_dma_addr(addr: u32, mode: u16, unit: u32) -> u32 {
    match mode {
        0 | 3 => addr.wrapping_add(unit),
        1 => addr.wrapping_sub(unit),
        2 => addr,
        _ => addr,
    }
}
