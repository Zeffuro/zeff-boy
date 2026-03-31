use super::Bus;
use crate::hardware::sgb::SgbEvent;
use crate::hardware::types::constants::*;
use crate::hardware::types::hardware_mode::HardwareMode;

pub(super) fn read_io(bus: &Bus, addr: u16) -> u8 {
    match addr {
        JOYP_P1 => bus.joypad_p1(),
        SERIAL_SB => bus.io.serial.sb(),
        SERIAL_SC => bus.io.serial.sc(),

        TIMER_DIV => bus.io.timer.div(),
        TIMER_TIMA => bus.io.timer.tima(),
        TIMER_TMA => bus.io.timer.tma(),
        TIMER_TAC => bus.io.timer.tac(),

        INTERRUPT_IF => bus.if_reg | 0xE0,

        PPU_LCDC => bus.io.ppu.lcdc.bits(),
        PPU_STAT => bus.io.ppu.stat | 0x80,
        PPU_SCY => bus.io.ppu.scy,
        PPU_SCX => bus.io.ppu.scx,
        PPU_LY => bus.io.ppu.ly,
        PPU_LYC => bus.io.ppu.lyc,
        PPU_WY => bus.io.ppu.wy,
        PPU_WX => bus.io.ppu.wx,
        PPU_BGP => bus.io.ppu.bgp,
        PPU_OBP0 => bus.io.ppu.obp0,
        PPU_OBP1 => bus.io.ppu.obp1,
        PPU_DMA => bus.io_bank[(addr - IO_START) as usize],
        CGB_KEY1 => {
            if bus.is_cgb_mode() {
                bus.key1
            } else {
                0xFF
            }
        }
        CGB_BCPS => {
            if bus.is_cgb_mode() {
                bus.io.ppu.read_bcps()
            } else {
                0xFF
            }
        }
        CGB_BCPD => {
            if bus.is_cgb_mode() {
                bus.io.ppu.read_bcpd()
            } else {
                0xFF
            }
        }
        CGB_OCPS => {
            if bus.is_cgb_mode() {
                bus.io.ppu.read_ocps()
            } else {
                0xFF
            }
        }
        CGB_OCPD => {
            if bus.is_cgb_mode() {
                bus.io.ppu.read_ocpd()
            } else {
                0xFF
            }
        }
        CGB_HDMA1 => {
            if bus.is_cgb_mode() {
                bus.hdma1
            } else {
                0xFF
            }
        }
        CGB_HDMA2 => {
            if bus.is_cgb_mode() {
                bus.hdma2
            } else {
                0xFF
            }
        }
        CGB_HDMA3 => {
            if bus.is_cgb_mode() {
                bus.hdma3
            } else {
                0xFF
            }
        }
        CGB_HDMA4 => {
            if bus.is_cgb_mode() {
                bus.hdma4
            } else {
                0xFF
            }
        }
        CGB_HDMA5 => {
            if bus.is_cgb_mode() {
                bus.hdma5
            } else {
                0xFF
            }
        }
        PPU_VBK => {
            if bus.is_cgb_mode() {
                0xFE | (bus.vram_bank & 0x01)
            } else {
                0xFF
            }
        }
        CGB_SVBK => {
            if bus.is_cgb_mode() {
                0xF8 | (bus.wram_bank & 0x07)
            } else {
                0xFF
            }
        }
        NR10..=NR52 | WAVE_RAM_START..=WAVE_RAM_END => bus.io.apu.read(addr),
        CGB_PCM12 | CGB_PCM34 => {
            if bus.is_cgb_mode() {
                bus.io.apu.read(addr)
            } else {
                0xFF
            }
        }

        _ => bus.io_bank[(addr - IO_START) as usize],
    }
}

pub(super) fn write_io(bus: &mut Bus, addr: u16, value: u8) -> u64 {
    bus.io_bank[(addr - IO_START) as usize] = value;

    match addr {
        JOYP_P1 => {
            bus.write_joypad_p1(value);
            if matches!(bus.hardware_mode, HardwareMode::SGB1 | HardwareMode::SGB2)
                && let Some(event) = bus.io.sgb.on_joyp_write(value)
            {
                apply_sgb_event(bus, event);
            }
        }
        SERIAL_SB => bus.io.serial.write_sb(value),
        SERIAL_SC => bus.io.serial.write_sc(value),

        TIMER_DIV => bus.io.timer.reset_div(),
        TIMER_TIMA => bus.io.timer.write_tima(value),
        TIMER_TMA => bus.io.timer.write_tma(value),
        TIMER_TAC => bus.io.timer.write_tac(value),

        INTERRUPT_IF => bus.if_reg = value & 0x1F,

        PPU_LCDC => bus.io.ppu.lcdc = crate::hardware::ppu::Lcdc::from_bits_truncate(value),
        PPU_STAT => bus.io.ppu.stat = (bus.io.ppu.stat & 0x07) | (value & 0xF8),
        PPU_SCY => bus.io.ppu.scy = value,
        PPU_SCX => bus.io.ppu.scx = value,
        PPU_LY => bus.io.ppu.ly = 0,
        PPU_LYC => bus.io.ppu.lyc = value,
        PPU_WY => bus.io.ppu.wy = value,
        PPU_WX => bus.io.ppu.wx = value,
        PPU_BGP => bus.io.ppu.bgp = value,
        PPU_OBP0 => bus.io.ppu.obp0 = value,
        PPU_OBP1 => bus.io.ppu.obp1 = value,
        PPU_DMA => bus.start_oam_dma(value),
        CGB_KEY1 => {
            if bus.is_cgb_mode() {
                bus.key1 = (bus.key1 & 0x80) | (value & 0x01) | 0x7E;
            }
        }
        CGB_BCPS => {
            if bus.is_cgb_mode() {
                bus.io.ppu.write_bcps(value);
            }
        }
        CGB_BCPD => {
            if bus.is_cgb_mode() {
                bus.io.ppu.write_bcpd(value);
            }
        }
        CGB_OCPS => {
            if bus.is_cgb_mode() {
                bus.io.ppu.write_ocps(value);
            }
        }
        CGB_OCPD => {
            if bus.is_cgb_mode() {
                bus.io.ppu.write_ocpd(value);
            }
        }
        CGB_HDMA1 => {
            if bus.is_cgb_mode() {
                bus.hdma1 = value;
            }
        }
        CGB_HDMA2 => {
            if bus.is_cgb_mode() {
                bus.hdma2 = value;
            }
        }
        CGB_HDMA3 => {
            if bus.is_cgb_mode() {
                bus.hdma3 = value;
            }
        }
        CGB_HDMA4 => {
            if bus.is_cgb_mode() {
                bus.hdma4 = value;
            }
        }
        CGB_HDMA5 => {
            if bus.is_cgb_mode() {
                return bus.execute_hdma_transfer(value);
            }
        }
        PPU_VBK => {
            if bus.is_cgb_mode() {
                bus.vram_bank = value & 0x01;
            }
        }
        CGB_SVBK => {
            if bus.is_cgb_mode() {
                let bank = value & 0x07;
                bus.wram_bank = if bank == 0 { 1 } else { bank };
            }
        }
        NR10..=NR52 | WAVE_RAM_START..=WAVE_RAM_END => bus.io.apu.write(addr, value),

        _ => {}
    }

    0
}

fn apply_sgb_event(bus: &mut Bus, event: SgbEvent) {
    match event {
        SgbEvent::Pal01(p0, p1) => {
            bus.io.ppu.set_sgb_palette(0, p0);
            bus.io.ppu.set_sgb_palette(1, p1);
        }
        SgbEvent::Pal23(p2, p3) => {
            bus.io.ppu.set_sgb_palette(2, p2);
            bus.io.ppu.set_sgb_palette(3, p3);
        }
        SgbEvent::PalSet(index) => bus.io.ppu.set_sgb_active_palette(index),
        SgbEvent::MaskEn(mode) => bus.io.ppu.set_sgb_mask_mode(mode),
        SgbEvent::MltReq => {}
    }
}
