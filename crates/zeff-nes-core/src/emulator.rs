use crate::hardware::bus::Bus;
use crate::hardware::cartridge::Cartridge;
use crate::hardware::cpu::{Cpu, CpuState};
use sha2::{Digest, Sha256};
use std::fmt;

mod runtime;

pub use crate::hardware::constants::CPU_CYCLES_PER_FRAME;

pub const DEFAULT_SAMPLE_RATE: f64 = 48000.0;

pub struct Emulator {
    pub(crate) cpu: Cpu,
    pub(crate) bus: Bus,
    pub(crate) rom_hash: [u8; 32],
    pub(crate) rom_crc32: u32,
    pub(crate) opcode_log: crate::debug::OpcodeLog,
    pub(crate) debug: crate::debug::DebugController,
}

impl Emulator {
    pub fn new(rom_data: &[u8], sample_rate: f64) -> anyhow::Result<Self> {
        let cartridge = Cartridge::load(rom_data)?;
        let rom_hash: [u8; 32] = Sha256::digest(rom_data).into();
        let rom_crc32 = crc32fast::hash(rom_data);
        let bus = Bus::new(cartridge, sample_rate);

        let mut emu = Self {
            cpu: Cpu::new(),
            bus,
            rom_hash,
            rom_crc32,
            opcode_log: crate::debug::OpcodeLog::new(),
            debug: crate::debug::DebugController::new(),
        };
        emu.cpu.power_on(&mut emu.bus);
        Ok(emu)
    }

    pub fn from_rom_data(rom_data: &[u8]) -> anyhow::Result<Self> {
        Self::new(rom_data, DEFAULT_SAMPLE_RATE)
    }

    pub fn reset(&mut self) {
        self.bus.reset();
        self.cpu.reset(&mut self.bus);
        self.opcode_log.clear();
        self.debug.clear_hits();
    }

    pub fn framebuffer(&self) -> &[u8] {
        &self.bus.ppu.framebuffer[..]
    }

    pub fn framebuffer_dimensions(&self) -> (usize, usize) {
        (
            crate::hardware::ppu::SCREEN_W,
            crate::hardware::ppu::SCREEN_H,
        )
    }

    pub fn has_battery(&self) -> bool {
        self.bus.cartridge.header().has_battery
    }

    pub fn dump_battery_sram(&self) -> Option<Vec<u8>> {
        if !self.bus.cartridge.header().has_battery {
            return None;
        }
        self.bus.cartridge.dump_battery_data()
    }

    pub fn load_battery_sram(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.bus.cartridge.load_battery_data(bytes)
    }

    pub fn frame_ready(&self) -> bool {
        self.bus.ppu.frame_ready
    }

    pub fn clear_frame_ready(&mut self) {
        self.bus.ppu.frame_ready = false;
    }

    pub fn drain_audio_samples(&mut self) -> Vec<f32> {
        self.bus.apu.drain_samples()
    }

    pub fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.drain_audio_into_stereo(buf);
    }

    pub fn encode_state(&self) -> anyhow::Result<Vec<u8>> {
        crate::save_state::encode_state(self)
    }

    pub fn load_state(&mut self, data: &[u8]) -> anyhow::Result<()> {
        crate::save_state::decode_state(self, data)?;
        self.opcode_log.clear();
        Ok(())
    }

    pub fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.load_state(&bytes)
    }

    pub fn rom_hash(&self) -> [u8; 32] {
        self.rom_hash
    }

    pub fn rom_crc32(&self) -> u32 {
        self.rom_crc32
    }

    pub fn frame_count(&self) -> u64 {
        self.bus.ppu.frame_count + u64::from(self.bus.ppu.frame_ready)
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.bus.apu.set_output_sample_rate(rate as f64);
    }

    pub fn drain_audio_into_stereo(&mut self, buf: &mut Vec<f32>) {
        self.bus.apu.drain_samples_into_stereo(buf);
    }

    pub fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.bus.apu.set_sample_generation_enabled(enabled);
    }

    pub fn set_apu_channel_mutes(&mut self, mutes: [bool; 5]) {
        self.bus.apu.set_channel_mutes(mutes);
    }

    pub fn set_apu_debug_collection_enabled(&mut self, enabled: bool) {
        self.bus.apu.set_debug_collection_enabled(enabled);
    }

    pub fn set_palette_mode(&mut self, mode: crate::hardware::ppu::NesPaletteMode) {
        self.bus.set_palette_mode(mode);
    }

    pub fn set_custom_palette(&mut self, palette: Option<crate::hardware::ppu::NesPalette>) {
        self.bus.set_custom_palette(palette);
    }

    pub fn palette_mode(&self) -> crate::hardware::ppu::NesPaletteMode {
        self.bus.palette_mode()
    }

    pub fn palette_color_rgba(&self, index: u8) -> [u8; 4] {
        self.bus.palette_color_rgba(index)
    }

    pub fn palette_lut(&self) -> [[u8; 4]; 64] {
        self.bus.palette_lut()
    }

    pub fn apu_channel_snapshot(&self) -> crate::hardware::apu::ApuChannelSnapshot {
        self.bus.apu.channel_snapshot()
    }

    pub fn set_input_p1(&mut self, buttons: u8) {
        self.bus.set_vs_system_credit_input(buttons & 0x04 != 0);
        self.bus.controller1.set_buttons(buttons);
    }

    pub fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.set_input_p1(map_host_to_nes_byte(buttons_pressed, dpad_pressed));
    }

    pub fn set_input_p2(&mut self, buttons: u8) {
        self.bus.controller2.set_buttons(buttons);
    }

    pub fn set_zapper_state(
        &mut self,
        enabled: bool,
        trigger: bool,
        hit: bool,
        screen_pos: Option<(u16, u16)>,
    ) {
        use crate::hardware::cartridge::NesMapper;
        use crate::hardware::controller::ControllerType;
        self.bus.set_zapper_light_sensor(screen_pos, hit);
        if !enabled {
            self.bus.controller1.set_type(ControllerType::Standard);
            self.bus.controller2.set_type(ControllerType::Standard);
            return;
        }

        match self.bus.cartridge.header().mapper_kind() {
            NesMapper::VsSystem | NesMapper::Vrc1VsSystem | NesMapper::LegacyVsVrc1 => {
                self.bus
                    .controller1
                    .set_type(ControllerType::VsZapper { trigger, hit });
                self.bus.controller2.set_type(ControllerType::Standard);
            }
            _ => {
                self.bus
                    .controller2
                    .set_type(ControllerType::Zapper { trigger, hit });
            }
        }
    }

    pub fn set_opcode_log_enabled(&mut self, enabled: bool) {
        self.opcode_log.set_enabled(enabled);
    }

    pub fn cpu_pc(&self) -> u16 {
        self.cpu.pc
    }

    pub fn cpu_cycles(&self) -> u64 {
        self.cpu.cycles
    }

    pub fn cpu_a(&self) -> u8 {
        self.cpu.regs.a
    }

    pub fn cpu_x(&self) -> u8 {
        self.cpu.regs.x
    }

    pub fn cpu_y(&self) -> u8 {
        self.cpu.regs.y
    }

    pub fn cpu_sp(&self) -> u8 {
        self.cpu.sp
    }

    pub fn cpu_status(&self) -> u8 {
        self.cpu.regs.p.bits()
    }

    pub fn cpu_last_opcode(&self) -> u8 {
        self.cpu.last_opcode
    }

    pub fn cpu_last_step_cycles(&self) -> u64 {
        self.cpu.last_step_cycles
    }

    pub fn cpu_nmi_pending(&self) -> bool {
        self.cpu.nmi_pending
    }

    pub fn cpu_irq_line(&self) -> bool {
        self.cpu.irq_line
    }

    pub fn cpu_nmi_count(&self) -> u64 {
        self.cpu.nmi_count
    }

    pub fn cpu_irq_count(&self) -> u64 {
        self.cpu.irq_count
    }

    pub fn is_cpu_suspended(&self) -> bool {
        self.cpu.state == CpuState::Suspended
    }

    pub fn debug_continue(&mut self) {
        self.debug.clear_hits();
        self.debug.break_on_next = false;
        self.cpu.state = CpuState::Running;
    }

    pub fn debug_step(&mut self) {
        self.debug.clear_hits();
        self.debug.break_on_next = true;
        self.cpu.state = CpuState::Running;
    }

    pub fn add_breakpoint(&mut self, addr: u16) {
        self.debug.add_breakpoint(addr);
    }

    pub fn remove_breakpoint(&mut self, addr: u16) {
        self.debug.remove_breakpoint(addr);
    }

    pub fn toggle_breakpoint(&mut self, addr: u16) {
        self.debug.toggle_breakpoint(addr);
    }

    pub fn add_watchpoint(&mut self, addr: u16, watch_type: crate::debug::WatchType) {
        self.debug.add_watchpoint(addr, watch_type);
    }

    pub fn iter_breakpoints(&self) -> impl Iterator<Item = u16> + '_ {
        self.debug.iter_breakpoints()
    }

    pub fn debug_watchpoints(&self) -> &[crate::debug::Watchpoint] {
        &self.debug.watchpoints
    }

    pub fn debug_hit_breakpoint(&self) -> Option<u16> {
        self.debug.hit_breakpoint
    }

    pub fn debug_hit_watchpoint(&self) -> Option<&crate::debug::WatchHit> {
        self.debug.hit_watchpoint.as_ref()
    }

    pub fn bus(&self) -> &Bus {
        &self.bus
    }

    pub fn bus_mut(&mut self) -> &mut Bus {
        &mut self.bus
    }

    pub fn cpu_write(&mut self, addr: u16, value: u8) {
        self.bus.cpu_write(addr, value);
    }

    pub fn cpu_peek(&self, addr: u16) -> u8 {
        self.bus.cpu_peek(addr)
    }

    pub fn cartridge_header(&self) -> &crate::hardware::cartridge::RomHeader {
        self.bus.cartridge.header()
    }

    pub fn cartridge_effective_mapper_label(&self) -> String {
        self.bus.cartridge.effective_mapper_label()
    }

    pub fn clear_game_genie(&mut self) {
        self.bus.game_genie.clear();
    }

    pub fn add_game_genie_patch(&mut self, patch: crate::cheats::NesGameGeniePatch) {
        self.bus.game_genie.patches.push(patch);
    }

    pub fn set_cpu_pc(&mut self, pc: u16) {
        self.cpu.pc = pc;
    }

    pub fn last_opcode_pc(&self) -> u16 {
        self.cpu.last_opcode_pc
    }

    pub fn ppu_palette_ram(&self) -> &[u8; 32] {
        &self.bus.ppu.palette_ram
    }

    pub fn ppu_oam(&self) -> &[u8; 256] {
        &self.bus.ppu.oam
    }

    pub fn ppu_nametable_ram(&self) -> &[u8; 0x1000] {
        &self.bus.ppu.nametable_ram
    }

    pub fn ppu_ctrl(&self) -> u8 {
        self.bus.ppu.regs.ctrl
    }

    pub fn ppu_mask(&self) -> u8 {
        self.bus.ppu.regs.mask
    }

    pub fn ppu_status(&self) -> u8 {
        self.bus.ppu.regs.status
    }

    pub fn ppu_scanline(&self) -> u16 {
        self.bus.ppu.scanline
    }

    pub fn ppu_dot(&self) -> u16 {
        self.bus.ppu.dot
    }

    pub fn ppu_frame_count(&self) -> u64 {
        self.bus.ppu.frame_count
    }

    pub fn ppu_in_vblank(&self) -> bool {
        self.bus.ppu.in_vblank
    }

    pub fn ppu_frame_ready(&self) -> bool {
        self.bus.ppu.frame_ready
    }

    pub fn ppu_scroll_v(&self) -> u16 {
        self.bus.ppu.v
    }

    pub fn ppu_scroll_t(&self) -> u16 {
        self.bus.ppu.t
    }

    pub fn ppu_fine_x(&self) -> u8 {
        self.bus.ppu.fine_x
    }

    pub fn ppu_tall_sprites(&self) -> bool {
        self.bus.ppu.regs.tall_sprites()
    }

    pub fn system_ram(&self) -> &[u8] {
        &self.bus.ram
    }

    pub fn chr_ram_snapshot(&mut self) -> Vec<u8> {
        let mut buf = vec![0u8; 0x2000];
        for addr in 0..0x2000u16 {
            buf[addr as usize] = self.bus.cartridge.chr_read(addr);
        }
        buf
    }
}

fn map_host_to_nes_byte(buttons_pressed: u8, dpad_pressed: u8) -> u8 {
    (buttons_pressed & 0x0F)
        | ((dpad_pressed & 0x04) << 2)
        | ((dpad_pressed & 0x08) << 2)
        | ((dpad_pressed & 0x02) << 5)
        | ((dpad_pressed & 0x01) << 7)
}

impl fmt::Debug for Emulator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NES Emulator")
            .field("cpu", &self.cpu)
            .field("bus", &self.bus)
            .field("opcode_log", &self.opcode_log)
            .field("debug", &self.debug)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::bus::DebugTraceEvent;
    use crate::hardware::constants::{APU_STATUS, FRAME_STEP_4};
    use crate::hardware::cpu::StatusFlags;

    fn build_test_rom_with_program(program: &[u8]) -> Vec<u8> {
        let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[5] = 1;
        let prg = 16;
        rom[prg..prg + program.len()].copy_from_slice(program);
        rom[prg + 0x3FFC] = 0x00;
        rom[prg + 0x3FFD] = 0x80;
        rom
    }

    fn build_test_rom() -> Vec<u8> {
        build_test_rom_with_program(&[0xEA])
    }

    fn build_vs_system_test_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 16 + 0x8000 + 0x4000];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 2;
        rom[5] = 2;
        rom[6] = 0x30;
        rom[7] = 0x60;
        let prg = 16;
        rom[prg] = 0xEA;
        rom[prg + 0x7FFC] = 0x00;
        rom[prg + 0x7FFD] = 0x80;
        rom
    }

    #[test]
    fn new_uses_power_on_reset_without_stack_adjust() {
        let emu = Emulator::new(&build_test_rom(), DEFAULT_SAMPLE_RATE).expect("test ROM");

        assert_eq!(emu.cpu.pc, 0x8000);
        assert_eq!(emu.cpu.sp, 0xFD);
        assert_eq!(emu.cpu.regs.a, 0);
        assert_eq!(emu.cpu.regs.x, 0);
        assert_eq!(emu.cpu.regs.y, 0);
        assert_eq!(emu.cpu.regs.p.bits(), 0x24);
    }

    #[test]
    fn public_api_parity_wrappers_load_step_and_roundtrip_state() {
        let rom = build_test_rom_with_program(&[0x4C, 0x00, 0x80]);
        let mut emu = Emulator::from_rom_data(&rom).expect("test ROM");

        assert_eq!(emu.framebuffer_dimensions(), (256, 240));
        assert_eq!(emu.framebuffer().len(), 256 * 240 * 4);
        assert_eq!(emu.frame_count(), 0);

        emu.set_input(0x01, 0x01);
        emu.step_frame();

        assert!(emu.frame_count() > 0);

        let mut audio = Vec::new();
        emu.drain_audio_samples_into(&mut audio);

        let state = emu
            .encode_state()
            .expect("NES emulator should encode state");
        emu.load_state(&state)
            .expect("NES emulator should load state");
    }

    #[test]
    fn reset_preserves_cpu_registers_and_decrements_stack() {
        let mut emu = Emulator::new(&build_test_rom(), DEFAULT_SAMPLE_RATE).expect("test ROM");
        emu.cpu.regs.a = 0x34;
        emu.cpu.regs.x = 0x56;
        emu.cpu.regs.y = 0x78;
        emu.cpu.regs.p = StatusFlags::from_bits_truncate(0xFB);
        emu.cpu.sp = 0x12;
        emu.bus.ram[0x110] = 0xBC;
        emu.bus.ram[0x111] = 0x9A;
        emu.bus.ram[0x112] = 0xFB;

        emu.reset();

        assert_eq!(emu.cpu.pc, 0x8000);
        assert_eq!(emu.cpu.regs.a, 0x34);
        assert_eq!(emu.cpu.regs.x, 0x56);
        assert_eq!(emu.cpu.regs.y, 0x78);
        assert_eq!(emu.cpu.regs.p.bits(), 0xFF);
        assert_eq!(emu.cpu.sp, 0x0F);
        assert_eq!(emu.bus.ram[0x110], 0xBC);
        assert_eq!(emu.bus.ram[0x111], 0x9A);
        assert_eq!(emu.bus.ram[0x112], 0xFB);
    }

    #[test]
    fn mapper_99_zapper_uses_vs_serial_protocol_on_4016() {
        let mut emu =
            Emulator::new(&build_vs_system_test_rom(), DEFAULT_SAMPLE_RATE).expect("test ROM");
        emu.set_zapper_state(true, true, true, None);

        emu.cpu_write(0x4016, 1);
        emu.cpu_write(0x4016, 0);

        let port_1_bits: Vec<u8> = (0..8)
            .map(|_| emu.bus_mut().cpu_read(0x4016) & 0x01)
            .collect();

        assert_eq!(port_1_bits, [0, 0, 0, 0, 1, 0, 1, 1]);
    }

    #[test]
    fn mapper_99_select_exposes_one_vs_coin_pulse() {
        let mut emu =
            Emulator::new(&build_vs_system_test_rom(), DEFAULT_SAMPLE_RATE).expect("test ROM");

        emu.set_input_p1(0x04);
        assert_eq!(emu.bus_mut().cpu_read(0x4016) & 0x24, 0x20);

        emu.set_input_p1(0);
        assert_eq!(emu.bus_mut().cpu_read(0x4016) & 0x04, 0);
        assert_eq!(emu.bus_mut().cpu_read(0x4016) & 0x20, 0x20);

        for _ in 0..4 {
            emu.bus.finish_vs_system_input_frame();
        }
        assert_eq!(emu.bus_mut().cpu_read(0x4016) & 0x20, 0);
    }

    #[test]
    fn nrom_select_does_not_expose_vs_credit_bits() {
        let mut emu = Emulator::new(&build_test_rom(), DEFAULT_SAMPLE_RATE).expect("test ROM");

        emu.set_input_p1(0x04);

        assert_eq!(emu.bus_mut().cpu_read(0x4016) & 0x24, 0);
    }

    #[test]
    fn indexed_store_dummy_read_can_ack_frame_irq_edge() {
        let rom = build_test_rom_with_program(&[
            0xA2, 0x15, // LDX #$15
            0xA9, 0x00, // LDA #$00
            0x9D, 0x00, 0x40, // STA $4000,X
            0xEA, // NOP
        ]);
        let mut emu = Emulator::new(&rom, DEFAULT_SAMPLE_RATE).expect("test ROM");

        emu.step_instruction();
        emu.step_instruction();

        emu.bus.apu.five_step_mode = false;
        emu.bus.apu.irq_inhibit = false;
        emu.bus.apu.frame_irq = false;
        emu.bus.apu.frame_cycle = FRAME_STEP_4 - 3;
        emu.bus.apu.frame_reset_delay = 0;

        let (_, _, _, events) = emu.step_instruction_with_bus_trace();

        let status_read = events.iter().find_map(|event| match event {
            DebugTraceEvent::Read { addr, value, .. } if *addr == APU_STATUS => Some(*value),
            _ => None,
        });

        assert_eq!(status_read.map(|value| value & 0x40), Some(0x40));
        assert!(!emu.bus.apu.irq_pending());
        assert!(!emu.cpu.irq_line);
    }
}
