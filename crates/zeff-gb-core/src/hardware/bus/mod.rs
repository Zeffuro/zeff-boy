use crate::hardware::cartridge::Cartridge;
use crate::hardware::io::IO;
use crate::hardware::types::constants::*;
use crate::hardware::types::hardware_mode::HardwareMode;
use std::fmt;

mod dma;
mod io_bus;
mod lifecycle;
mod mem_map;
mod oam_corruption;
mod state;
mod trace;

pub use oam_corruption::OamCorruptionType;
pub use trace::CpuAccessTraceEvent;

pub struct Bus {
    pub cartridge: Cartridge,
    pub hardware_mode: HardwareMode,
    pub vram: [u8; VRAM_SIZE * 2],
    pub wram: [u8; WRAM_SIZE * 8],
    pub vram_bank: u8,
    pub wram_bank: u8,
    pub key1: u8,
    pub hdma1: u8,
    pub hdma2: u8,
    pub hdma3: u8,
    pub hdma4: u8,
    pub hdma5: u8,
    pub hdma_active: bool,
    pub hdma_hblank: bool,
    pub hdma_blocks_left: u8,
    pub oam_dma_active: bool,
    oam_dma_source_base: u16,
    oam_dma_index: u16,
    oam_dma_t_cycle_accum: u64,
    pub oam: [u8; OAM_SIZE],
    pub io_bank: [u8; IO_SIZE],
    pub hram: [u8; HRAM_SIZE],
    pub ie: u8,
    pub if_reg: u8,
    io: IO,
    pub trace_cpu_accesses: bool,
    cpu_read_trace: Vec<(u16, u8)>,
    cpu_write_trace: Vec<(u16, u8, u8)>,
    pub game_genie_patches: Vec<crate::cheats::CheatPatch>,
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

impl Bus {
    pub fn joypad_p1(&self) -> u8 {
        self.io.joypad.read()
    }

    pub fn write_joypad_p1(&mut self, value: u8) {
        self.io.joypad.write(value);
    }

    pub fn apply_joypad_pressed_masks(
        &mut self,
        buttons_pressed: u8,
        dpad_pressed: u8,
    ) -> bool {
        self.io
            .joypad
            .apply_pressed_masks(buttons_pressed, dpad_pressed)
    }

    pub fn apu_sample_rate(&self) -> u32 {
        self.io.apu.sample_rate
    }

    pub fn set_apu_sample_rate(&mut self, sample_rate: u32) {
        self.io.apu.set_sample_rate(sample_rate);
    }

    pub fn set_apu_debug_capture_enabled(&mut self, enabled: bool) {
        self.io.apu.debug_capture_enabled = enabled;
    }

    pub fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.io.apu.sample_generation_enabled = enabled;
    }

    pub fn set_apu_enabled(&mut self, enabled: bool) {
        self.io.apu.apu_enabled = enabled;
    }

    pub fn apu_drain_samples_into(&mut self, target: &mut Vec<f32>) {
        self.io.apu.drain_samples_into(target);
    }

    pub fn apu_drain_samples(&mut self) -> Vec<f32> {
        self.io.apu.drain_samples()
    }

    pub fn apu_channel_snapshot(&self) -> crate::hardware::apu::ApuChannelSnapshot {
        self.io.apu.channel_snapshot()
    }

    pub fn set_apu_channel_mutes(&mut self, mutes: [bool; 4]) {
        self.io.apu.set_channel_mutes(mutes);
    }

    pub fn apu_regs_snapshot(&self) -> [u8; 0x17] {
        self.io.apu.regs_snapshot()
    }

    pub fn apu_wave_ram_snapshot(&self) -> [u8; 0x10] {
        self.io.apu.wave_ram_snapshot()
    }

    pub fn apu_nr52_raw(&self) -> u8 {
        self.io.apu.nr52_raw()
    }

    pub fn apu_channel_debug_samples_ordered(&self, channel: usize) -> [f32; 512] {
        self.io.apu.channel_debug_samples_ordered(channel)
    }

    pub fn apu_master_debug_samples_ordered(&self) -> [f32; 512] {
        self.io.apu.master_debug_samples_ordered()
    }

    pub fn apu_channel_mutes(&self) -> [bool; 4] {
        self.io.apu.channel_mutes()
    }

    pub fn apply_bess_apu_io(&mut self, io_regs: &[u8]) {
        self.io.apu.apply_bess_io(io_regs);
    }

    pub(in crate::hardware) fn ppu_mode(&self) -> u8 {
        self.io.ppu.mode()
    }

    pub fn ppu_framebuffer(&self) -> &[u8] {
        &self.io.ppu.framebuffer
    }

    pub fn ppu_lcdc(&self) -> u8 {
        self.io.ppu.lcdc
    }

    pub fn ppu_stat(&self) -> u8 {
        self.io.ppu.stat
    }

    pub fn ppu_scy(&self) -> u8 {
        self.io.ppu.scy
    }

    pub fn ppu_scx(&self) -> u8 {
        self.io.ppu.scx
    }

    pub fn ppu_ly(&self) -> u8 {
        self.io.ppu.ly
    }

    pub fn ppu_lyc(&self) -> u8 {
        self.io.ppu.lyc
    }

    pub fn ppu_wy(&self) -> u8 {
        self.io.ppu.wy
    }

    pub fn ppu_wx(&self) -> u8 {
        self.io.ppu.wx
    }

    pub fn ppu_bgp(&self) -> u8 {
        self.io.ppu.bgp
    }

    pub fn ppu_obp0(&self) -> u8 {
        self.io.ppu.obp0
    }

    pub fn ppu_obp1(&self) -> u8 {
        self.io.ppu.obp1
    }

    pub fn ppu_cycles(&self) -> u64 {
        self.io.ppu.cycles
    }

    pub fn ppu_cgb_mode(&self) -> bool {
        self.io.ppu.cgb_mode
    }

    pub fn ppu_bg_palette_ram_snapshot(&self) -> [u8; 0x40] {
        self.io.ppu.bg_palette_ram
    }

    pub fn ppu_obj_palette_ram_snapshot(&self) -> [u8; 0x40] {
        self.io.ppu.obj_palette_ram
    }

    pub fn ppu_bg_palette_ram(&self) -> &[u8; 0x40] {
        &self.io.ppu.bg_palette_ram
    }

    pub fn ppu_obj_palette_ram(&self) -> &[u8; 0x40] {
        &self.io.ppu.obj_palette_ram
    }

    pub fn ppu_bg_palette_ram_mut(&mut self) -> &mut [u8; 0x40] {
        &mut self.io.ppu.bg_palette_ram
    }

    pub fn ppu_obj_palette_ram_mut(&mut self) -> &mut [u8; 0x40] {
        &mut self.io.ppu.obj_palette_ram
    }

    pub fn set_ppu_debug_flags(&mut self, bg: bool, window: bool, sprites: bool) {
        self.io.ppu.debug_flags.bg = bg;
        self.io.ppu.debug_flags.window = window;
        self.io.ppu.debug_flags.sprites = sprites;
    }

    pub fn set_ppu_sgb_mode(&mut self, enabled: bool) {
        self.io.ppu.set_sgb_mode(enabled);
    }

    pub fn apply_bess_ppu_registers(&mut self, io: &[u8], is_cgb: bool) {
        self.io.ppu.lcdc = io[0x40];
        self.io.ppu.stat = io[0x41];
        self.io.ppu.scy = io[0x42];
        self.io.ppu.scx = io[0x43];
        self.io.ppu.ly = io[0x44];
        self.io.ppu.lyc = io[0x45];
        self.io.ppu.bgp = io[0x47];
        self.io.ppu.obp0 = io[0x48];
        self.io.ppu.obp1 = io[0x49];
        self.io.ppu.wy = io[0x4A];
        self.io.ppu.wx = io[0x4B];

        if is_cgb {
            self.io.ppu.cgb_mode = io[0x4C] & 0x04 == 0;
            self.io.ppu.bcps = io[0x68];
            self.io.ppu.ocps = io[0x6A];
        }
    }

    #[inline]
    pub(in crate::hardware) fn step_ppu(&mut self, system_t_cycles: u64) -> u8 {
        let cgb_mode = matches!(
            self.hardware_mode,
            HardwareMode::CGBNormal | HardwareMode::CGBDouble
        );
        self.io
            .ppu
            .step(system_t_cycles, &self.vram, &self.oam, cgb_mode)
    }

    pub fn timer_div(&self) -> u8 {
        self.io.timer.div()
    }

    pub fn timer_tima(&self) -> u8 {
        self.io.timer.tima()
    }

    pub fn timer_tma(&self) -> u8 {
        self.io.timer.tma()
    }

    pub fn timer_tac(&self) -> u8 {
        self.io.timer.tac()
    }

    #[inline]
    pub(in crate::hardware) fn step_timer(&mut self, t_cycles: u64) {
        if self.io.timer.step(t_cycles) {
            self.if_reg |= 0x04;
        }
    }

    #[inline]
    pub(in crate::hardware) fn step_serial(&mut self, t_cycles: u64) {
        if self.io.serial.step(t_cycles) {
            self.if_reg |= 0x08;
        }
    }

    #[inline]
    pub(in crate::hardware) fn step_apu(&mut self, system_t_cycles: u64) {
        self.io.apu.step(system_t_cycles);
    }

    pub fn serial_output_bytes(&self) -> &[u8] {
        self.io.serial.output_bytes()
    }

    pub fn sync_timer_serial_mode(&mut self) {
        self.io.timer.set_mode(self.hardware_mode);
        self.io.serial.set_mode(self.hardware_mode);
    }

    pub fn apply_bess_timer_serial_registers(&mut self, io: &[u8], mode: HardwareMode) {
        self.io.serial.write_sb(io[0x01]);
        self.io.serial.write_sc(io[0x02] & !0x80);
        self.io.serial.set_mode(mode);
        self.io.serial.reset_cycles();

        self.io.timer.apply_bess_div(io[0x04]);
        self.io.timer.set_tima_raw(io[0x05]);
        self.io.timer.set_tma_raw(io[0x06]);
        self.io.timer.set_tac_raw(io[0x07]);
        self.io.timer.set_mode(mode);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cheats::CheatPatch;
    use crate::cheats::CheatValue;
    use crate::hardware::ppu::LCDC_LCD_ENABLE;
    use crate::hardware::rom_header::RomHeader;

    fn make_test_bus() -> Bus {
        let mut rom = vec![0u8; 0x8000];
        for (i, byte) in rom.iter_mut().take(0x100).enumerate() {
            *byte = i as u8;
        }
        let header = RomHeader::from_rom(&rom).expect("test ROM header should parse");
        Bus::new(rom, &header, HardwareMode::DMG).expect("test bus should initialize")
    }

    fn make_cgb_test_bus() -> Bus {
        let mut rom = vec![0u8; 0x8000];
        for (i, byte) in rom.iter_mut().take(0x100).enumerate() {
            *byte = i as u8;
        }
        rom[0x143] = 0x80; // CGB compatible flag
        let header = RomHeader::from_rom(&rom).expect("test ROM header should parse");
        Bus::new(rom, &header, HardwareMode::CGBNormal).expect("test bus should initialize")
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

    #[test]
    fn read_rom_bank_0() {
        let bus = make_test_bus();
        assert_eq!(bus.read_byte_raw(0x0000), 0x00);
        assert_eq!(bus.read_byte_raw(0x0010), 0x10);
        assert_eq!(bus.read_byte_raw(0x00FF), 0xFF);
    }

    #[test]
    fn read_wram_bank_0() {
        let mut bus = make_test_bus();
        bus.wram[0] = 0xAB;
        bus.wram[0x0FFF] = 0xCD;
        assert_eq!(bus.read_byte_raw(WRAM_0_START), 0xAB);
        assert_eq!(bus.read_byte_raw(WRAM_0_END), 0xCD);
    }

    #[test]
    fn read_wram_bank_n() {
        let mut bus = make_test_bus();
        bus.wram[WRAM_SIZE] = 0x11;
        bus.wram[WRAM_SIZE + 0x0FFF] = 0x22;
        assert_eq!(bus.read_byte_raw(WRAM_N_START), 0x11);
        assert_eq!(bus.read_byte_raw(WRAM_N_END), 0x22);
    }

    #[test]
    fn read_write_hram() {
        let mut bus = make_test_bus();
        bus.write_byte(HRAM_START, 0xDE);
        bus.write_byte(HRAM_END, 0xAD);
        assert_eq!(bus.read_byte_raw(HRAM_START), 0xDE);
        assert_eq!(bus.read_byte_raw(HRAM_END), 0xAD);
    }

    #[test]
    fn read_write_ie() {
        let mut bus = make_test_bus();
        assert_eq!(bus.read_byte_raw(IE_ADDR), 0x00);
        bus.write_byte(IE_ADDR, 0x1F);
        assert_eq!(bus.read_byte_raw(IE_ADDR), 0x1F);
    }

    #[test]
    fn read_not_usable_returns_ff() {
        let bus = make_test_bus();
        assert_eq!(bus.read_byte_raw(NOT_USABLE_START), 0xFF);
        assert_eq!(bus.read_byte_raw(NOT_USABLE_END), 0xFF);
    }

    #[test]
    fn read_write_oam() {
        let mut bus = make_test_bus();
        bus.write_byte(OAM_START, 0x42);
        bus.write_byte(OAM_START + 0x9F, 0x99);
        assert_eq!(bus.read_byte_raw(OAM_START), 0x42);
        assert_eq!(bus.read_byte_raw(OAM_START + 0x9F), 0x99);
    }

    #[test]
    fn read_write_vram() {
        let mut bus = make_test_bus();
        bus.write_byte(VRAM_START, 0xAA);
        bus.write_byte(VRAM_END, 0xBB);
        assert_eq!(bus.read_byte_raw(VRAM_START), 0xAA);
        assert_eq!(bus.read_byte_raw(VRAM_END), 0xBB);
    }

    #[test]
    fn echo_ram_read_mirrors_wram_bank_0() {
        let mut bus = make_test_bus();
        bus.wram[0] = 0x77;
        bus.wram[0x0FFF] = 0x88;
        assert_eq!(bus.read_byte_raw(ECHO_RAM_START), 0x77);
        assert_eq!(bus.read_byte_raw(0xEFFF), 0x88);
    }

    #[test]
    fn echo_ram_read_mirrors_wram_bank_n() {
        let mut bus = make_test_bus();
        bus.wram[WRAM_SIZE] = 0xAA;
        bus.wram[WRAM_SIZE + 0x0DFF] = 0xBB;
        assert_eq!(bus.read_byte_raw(0xF000), 0xAA);
        assert_eq!(bus.read_byte_raw(ECHO_RAM_END), 0xBB);
    }

    #[test]
    fn echo_ram_write_mirrors_to_wram() {
        let mut bus = make_test_bus();
        bus.write_byte(ECHO_RAM_START, 0x55);
        assert_eq!(bus.wram[0], 0x55);
        assert_eq!(bus.read_byte_raw(WRAM_0_START), 0x55);

        bus.write_byte(0xF000, 0x66);
        assert_eq!(bus.wram[WRAM_SIZE], 0x66);
        assert_eq!(bus.read_byte_raw(WRAM_N_START), 0x66);
    }

    #[test]
    fn echo_ram_boundary_bank_0_to_n() {
        let mut bus = make_test_bus();
        bus.wram[0x0FFF] = 0x12;
        assert_eq!(bus.read_byte_raw(0xEFFF), 0x12);
        bus.wram[WRAM_SIZE] = 0x34;
        assert_eq!(bus.read_byte_raw(0xF000), 0x34);
    }

    #[test]
    fn vram_read_returns_ff_during_mode_3() {
        let mut bus = make_test_bus();
        bus.vram[0] = 0x5A;
        bus.io.ppu.lcdc |= LCDC_LCD_ENABLE;
        bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x03;
        assert_eq!(bus.read_byte_raw(VRAM_START), 0xFF);
    }

    #[test]
    fn vram_write_blocked_during_mode_3() {
        let mut bus = make_test_bus();
        bus.io.ppu.lcdc |= LCDC_LCD_ENABLE;
        bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x03;
        bus.write_byte(VRAM_START, 0xAB);
        assert_eq!(bus.vram[0], 0x00);
    }

    #[test]
    fn vram_accessible_when_lcd_off() {
        let mut bus = make_test_bus();
        bus.io.ppu.lcdc &= !LCDC_LCD_ENABLE;
        bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x03;
        bus.write_byte(VRAM_START, 0xCC);
        assert_eq!(bus.read_byte_raw(VRAM_START), 0xCC);
    }

    #[test]
    fn oam_read_returns_ff_during_mode_2() {
        let mut bus = make_test_bus();
        bus.oam[0] = 0x42;
        bus.io.ppu.lcdc |= LCDC_LCD_ENABLE;
        bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x02;
        assert_eq!(bus.read_byte_raw(OAM_START), 0xFF);
    }

    #[test]
    fn oam_read_returns_ff_during_mode_3() {
        let mut bus = make_test_bus();
        bus.oam[0] = 0x42;
        bus.io.ppu.lcdc |= LCDC_LCD_ENABLE;
        bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x03;
        assert_eq!(bus.read_byte_raw(OAM_START), 0xFF);
    }

    #[test]
    fn oam_write_blocked_during_mode_2() {
        let mut bus = make_test_bus();
        bus.io.ppu.lcdc |= LCDC_LCD_ENABLE;
        bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x02;
        bus.write_byte(OAM_START, 0xEE);
        assert_eq!(bus.oam[0], 0x00);
    }

    #[test]
    fn oam_accessible_during_mode_0_and_1() {
        let mut bus = make_test_bus();
        bus.io.ppu.lcdc |= LCDC_LCD_ENABLE;
        bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x00;
        bus.write_byte(OAM_START, 0x11);
        assert_eq!(bus.read_byte_raw(OAM_START), 0x11);
        bus.io.ppu.stat = (bus.io.ppu.stat & !0x03) | 0x01;
        bus.write_byte(OAM_START, 0x22);
        assert_eq!(bus.read_byte_raw(OAM_START), 0x22);
    }

    #[test]
    fn cgb_wram_bank_switching() {
        let mut bus = make_cgb_test_bus();
        bus.wram_bank = 1;
        bus.write_byte(WRAM_N_START, 0xAA);
        assert_eq!(bus.read_byte_raw(WRAM_N_START), 0xAA);
        bus.wram_bank = 2;
        assert_eq!(bus.read_byte_raw(WRAM_N_START), 0x00);
        bus.write_byte(WRAM_N_START, 0xBB);
        bus.wram_bank = 1;
        assert_eq!(bus.read_byte_raw(WRAM_N_START), 0xAA);
        bus.wram_bank = 2;
        assert_eq!(bus.read_byte_raw(WRAM_N_START), 0xBB);
    }

    #[test]
    fn cgb_wram_bank_0_maps_to_bank_1() {
        let mut bus = make_cgb_test_bus();
        bus.wram_bank = 0;
        bus.write_byte(WRAM_N_START, 0xCC);

        bus.wram_bank = 1;
        assert_eq!(bus.read_byte_raw(WRAM_N_START), 0xCC);
    }

    #[test]
    fn cgb_vram_bank_switching() {
        let mut bus = make_cgb_test_bus();
        bus.vram_bank = 0;
        bus.write_byte(VRAM_START, 0x11);
        bus.vram_bank = 1;
        assert_eq!(bus.read_byte_raw(VRAM_START), 0x00);
        bus.write_byte(VRAM_START, 0x22);
        bus.vram_bank = 0;
        assert_eq!(bus.read_byte_raw(VRAM_START), 0x11);
        bus.vram_bank = 1;
        assert_eq!(bus.read_byte_raw(VRAM_START), 0x22);
    }

    #[test]
    fn cgb_echo_ram_uses_active_wram_bank() {
        let mut bus = make_cgb_test_bus();
        bus.wram_bank = 3;
        bus.write_byte(WRAM_N_START, 0x55);
        assert_eq!(bus.read_byte_raw(0xF000), 0x55);
        bus.wram_bank = 4;
        assert_eq!(bus.read_byte_raw(0xF000), 0x00);
        bus.write_byte(0xF000, 0x66);
        assert_eq!(bus.read_byte_raw(WRAM_N_START), 0x66);
    }

    #[test]
    fn gdma_transfers_all_blocks_immediately() {
        let mut bus = make_cgb_test_bus();
        bus.hdma1 = 0xC0;
        bus.hdma2 = 0x00;
        bus.hdma3 = 0x00;
        bus.hdma4 = 0x00;
        for i in 0..0x20u16 {
            bus.wram[i as usize] = (i + 1) as u8;
        }
        let t_cycles = bus.execute_hdma_transfer(0x01);
        assert!(!bus.hdma_active);
        assert_eq!(bus.hdma5, 0xFF);
        assert_eq!(t_cycles, 2 * 32);
        for i in 0..0x20u16 {
            assert_eq!(bus.vram[i as usize], (i + 1) as u8);
        }
    }

    #[test]
    fn gdma_advances_source_and_dest_pointers() {
        let mut bus = make_cgb_test_bus();
        bus.hdma1 = 0xC0;
        bus.hdma2 = 0x00;
        bus.hdma3 = 0x00;
        bus.hdma4 = 0x00;
        bus.execute_hdma_transfer(0x00);
        assert_eq!(bus.hdma1, 0xC0);
        assert_eq!(bus.hdma2, 0x10);
        assert_eq!(bus.hdma3, 0x00);
        assert_eq!(bus.hdma4, 0x10);
    }

    #[test]
    fn gdma_double_speed_uses_64_t_per_block() {
        let mut bus = make_cgb_test_bus();
        bus.hardware_mode = HardwareMode::CGBDouble;
        bus.hdma1 = 0xC0;
        bus.hdma2 = 0x00;
        bus.hdma3 = 0x00;
        bus.hdma4 = 0x00;

        let t_cycles = bus.execute_hdma_transfer(0x03);
        assert_eq!(t_cycles, 4 * 64);
    }

    #[test]
    fn hblank_hdma_setup_does_not_transfer_immediately() {
        let mut bus = make_cgb_test_bus();
        bus.hdma1 = 0xC0;
        bus.hdma2 = 0x00;
        bus.hdma3 = 0x00;
        bus.hdma4 = 0x00;
        let t_cycles = bus.execute_hdma_transfer(0x81);

        assert!(bus.hdma_active);
        assert!(bus.hdma_hblank);
        assert_eq!(bus.hdma_blocks_left, 2);
        assert_eq!(t_cycles, 0);
    }

    #[test]
    fn hblank_hdma_transfers_one_block_per_mode_0_transition() {
        let mut bus = make_cgb_test_bus();
        bus.io.ppu.lcdc |= LCDC_LCD_ENABLE;
        bus.io.ppu.ly = 0;
        bus.hdma1 = 0xC0;
        bus.hdma2 = 0x00;
        bus.hdma3 = 0x00;
        bus.hdma4 = 0x00;

        bus.wram[0] = 0xAA;
        bus.wram[0x10] = 0xBB;
        bus.execute_hdma_transfer(0x81);
        assert_eq!(bus.hdma_blocks_left, 2);
        bus.maybe_step_hblank_hdma(3, 0);
        assert_eq!(bus.hdma_blocks_left, 1);
        assert!(bus.hdma_active);
        assert_eq!(bus.vram[0], 0xAA);
        bus.maybe_step_hblank_hdma(3, 0);
        assert!(!bus.hdma_active);
        assert_eq!(bus.hdma5, 0xFF);
        assert_eq!(bus.vram[0x10], 0xBB);
    }

    #[test]
    fn hblank_hdma_ignores_non_mode_0_transitions() {
        let mut bus = make_cgb_test_bus();
        bus.io.ppu.lcdc |= LCDC_LCD_ENABLE;
        bus.io.ppu.ly = 0;
        bus.hdma1 = 0xC0;
        bus.hdma2 = 0x00;
        bus.hdma3 = 0x00;
        bus.hdma4 = 0x00;

        bus.execute_hdma_transfer(0x80);
        let blocks_before = bus.hdma_blocks_left;
        bus.maybe_step_hblank_hdma(0, 2);
        assert_eq!(bus.hdma_blocks_left, blocks_before);
        bus.maybe_step_hblank_hdma(2, 3);
        assert_eq!(bus.hdma_blocks_left, blocks_before);
    }

    #[test]
    fn hblank_hdma_skipped_during_vblank() {
        let mut bus = make_cgb_test_bus();
        bus.io.ppu.lcdc |= LCDC_LCD_ENABLE;
        bus.io.ppu.ly = 144;
        bus.hdma1 = 0xC0;
        bus.hdma2 = 0x00;
        bus.hdma3 = 0x00;
        bus.hdma4 = 0x00;

        bus.execute_hdma_transfer(0x80);
        let blocks_before = bus.hdma_blocks_left;

        bus.maybe_step_hblank_hdma(3, 0);
        assert_eq!(bus.hdma_blocks_left, blocks_before);
    }

    #[test]
    fn hblank_hdma_cancel() {
        let mut bus = make_cgb_test_bus();
        bus.hdma1 = 0xC0;
        bus.hdma2 = 0x00;
        bus.hdma3 = 0x00;
        bus.hdma4 = 0x00;
        bus.execute_hdma_transfer(0x83);
        assert!(bus.hdma_active);
        assert!(bus.hdma_hblank);
        let t_cycles = bus.execute_hdma_transfer(0x00);
        assert!(!bus.hdma_active);
        assert!(!bus.hdma_hblank);
        assert_eq!(t_cycles, 0);
        assert_ne!(bus.hdma5 & 0x80, 0);
    }

    #[test]
    fn game_genie_rom_write_overrides_read() {
        let mut bus = make_test_bus();
        assert_eq!(bus.read_byte(0x0010), 0x10);

        bus.game_genie_patches.push(CheatPatch::RomWrite {
            address: 0x0010,
            value: CheatValue::Constant(0xFF),
        });

        assert_eq!(bus.read_byte(0x0010), 0xFF);
        assert_eq!(bus.read_byte(0x0011), 0x11);
    }

    #[test]
    fn game_genie_rom_write_if_equals_conditional() {
        let mut bus = make_test_bus();
        bus.game_genie_patches.push(CheatPatch::RomWriteIfEquals {
            address: 0x0020,
            value: CheatValue::Constant(0xAA),
            compare: CheatValue::Constant(0x20),
        });

        assert_eq!(bus.read_byte(0x0020), 0xAA);
    }

    #[test]
    fn game_genie_rom_write_if_equals_no_match() {
        let mut bus = make_test_bus();
        bus.game_genie_patches.push(CheatPatch::RomWriteIfEquals {
            address: 0x0020,
            value: CheatValue::Constant(0xAA),
            compare: CheatValue::Constant(0x99),
        });
        assert_eq!(bus.read_byte(0x0020), 0x20);
    }

    #[test]
    fn game_genie_empty_patches_fast_path() {
        let bus = make_test_bus();
        assert!(bus.game_genie_patches.is_empty());
        assert_eq!(bus.read_byte(0x0010), 0x10);
    }

    #[test]
    fn game_genie_non_rom_reads_unaffected() {
        let mut bus = make_test_bus();
        bus.wram[0] = 0x42;
        bus.game_genie_patches.push(CheatPatch::RomWrite {
            address: 0xC000,
            value: CheatValue::Constant(0xFF),
        });
        assert_eq!(bus.read_byte(WRAM_0_START), 0x42);
    }

    #[test]
    fn game_genie_multiple_patches_first_match_wins() {
        let mut bus = make_test_bus();
        bus.game_genie_patches.push(CheatPatch::RomWrite {
            address: 0x0010,
            value: CheatValue::Constant(0xAA),
        });
        bus.game_genie_patches.push(CheatPatch::RomWrite {
            address: 0x0010,
            value: CheatValue::Constant(0xBB),
        });
        assert_eq!(bus.read_byte(0x0010), 0xAA);
    }

    #[test]
    fn read_byte_raw_bypasses_game_genie() {
        let mut bus = make_test_bus();
        bus.game_genie_patches.push(CheatPatch::RomWrite {
            address: 0x0010,
            value: CheatValue::Constant(0xFF),
        });
        assert_eq!(bus.read_byte(0x0010), 0xFF);
        assert_eq!(bus.read_byte_raw(0x0010), 0x10);
    }

    #[test]
    fn write_to_wram_stores_correctly() {
        let mut bus = make_test_bus();
        bus.write_byte(WRAM_0_START, 0x11);
        bus.write_byte(WRAM_0_START + 1, 0x22);
        bus.write_byte(WRAM_N_START, 0x33);
        assert_eq!(bus.wram[0], 0x11);
        assert_eq!(bus.wram[1], 0x22);
        assert_eq!(bus.wram[WRAM_SIZE], 0x33);
    }

    #[test]
    fn write_returns_zero_extra_cycles_for_simple_regions() {
        let mut bus = make_test_bus();
        assert_eq!(bus.write_byte(WRAM_0_START, 0x00), 0);
        assert_eq!(bus.write_byte(HRAM_START, 0x00), 0);
        assert_eq!(bus.write_byte(IE_ADDR, 0x00), 0);
        assert_eq!(bus.write_byte(OAM_START, 0x00), 0);
        assert_eq!(bus.write_byte(VRAM_START, 0x00), 0);
    }

    #[test]
    fn if_register_read_write() {
        let mut bus = make_test_bus();
        assert_eq!(bus.read_byte_raw(INTERRUPT_IF), 0xE1);
        bus.if_reg = 0x00;
        assert_eq!(bus.read_byte_raw(INTERRUPT_IF), 0x00);
    }
}
