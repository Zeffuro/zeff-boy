use crate::hardware::cartridge::Cartridge;
use crate::hardware::io::IO;
use crate::hardware::types::constants::*;
use crate::hardware::types::hardware_mode::HardwareMode;
use std::fmt;

mod dma;
mod io_bus;
mod lifecycle;
mod mem_map;
mod state;
mod trace;

pub(crate) use trace::CpuAccessTraceEvent;

pub(crate) struct Bus {
    pub(crate) cartridge: Cartridge,
    pub(crate) hardware_mode: HardwareMode,
    pub(crate) vram: [u8; VRAM_SIZE * 2],
    pub(crate) wram: [u8; WRAM_SIZE * 8],
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
    pub(crate) oam: [u8; OAM_SIZE],
    pub(crate) io_bank: [u8; IO_SIZE],
    pub(crate) hram: [u8; HRAM_SIZE],
    pub(crate) ie: u8,
    pub(crate) if_reg: u8,
    pub(crate) io: IO,
    pub(crate) trace_cpu_accesses: bool,
    cpu_read_trace: Vec<(u16, u8)>,
    cpu_write_trace: Vec<(u16, u8, u8)>,
    pub(crate) game_genie_patches: Vec<crate::cheats::CheatPatch>,
}

impl fmt::Debug for Bus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bus")
            .field("hardware_mode", &self.hardware_mode)
            .field("vram_bank", &self.vram_bank)
            .field("wram_bank", &self.wram_bank)
            .field("key1", &format_args!("{:#04X}", self.key1))
            .field("ie", &format_args!("{:#04X}", self.ie))
            .field("if_reg", &format_args!("{:#04X}", self.if_reg))
            .field("oam_dma_active", &self.oam_dma_active)
            .field("hdma_active", &self.hdma_active)
            .field("hdma_hblank", &self.hdma_hblank)
            .field("game_genie_patches", &self.game_genie_patches.len())
            .field("io", &self.io)
            .finish_non_exhaustive()
    }
}

impl Bus {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::rom_header::RomHeader;

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
