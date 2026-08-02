use crate::hardware::bus::{Bus, DebugTraceEvent};
use crate::hardware::cartridge::{Cartridge, RomFooter};
use crate::hardware::constants::CYCLES_PER_FRAME;
use crate::hardware::cpu::{Cpu, CpuState, CpuTrap, FetchedInstruction};
use sha2::{Digest, Sha256};
use std::fmt;

pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;

pub struct Emulator {
    pub(crate) cpu: Cpu,
    pub(crate) bus: Bus,
    pub(crate) rom_hash: [u8; 32],
    pub(crate) rom_crc32: u32,
    pub(crate) frame_count: u64,
    sample_rate: u32,
    apu_sample_generation_enabled: bool,
}

impl Emulator {
    pub fn new(rom_data: &[u8], sample_rate: u32) -> anyhow::Result<Self> {
        let cartridge = Cartridge::load(rom_data)?;
        let rom_hash: [u8; 32] = Sha256::digest(rom_data).into();
        let rom_crc32 = crc32fast::hash(rom_data);
        let mut emu = Self {
            cpu: Cpu::new(),
            bus: Bus::new(cartridge),
            rom_hash,
            rom_crc32,
            frame_count: 0,
            sample_rate,
            apu_sample_generation_enabled: true,
        };
        emu.reset();
        Ok(emu)
    }

    pub fn from_rom_data(rom_data: &[u8]) -> anyhow::Result<Self> {
        Self::new(rom_data, DEFAULT_SAMPLE_RATE)
    }

    pub fn reset(&mut self) {
        self.bus.reset();
        self.cpu.reset();
        self.frame_count = 0;
    }

    pub fn step_frame(&mut self) {
        if self.cpu.is_suspended() {
            return;
        }
        self.clear_frame_ready();
        let guard = self
            .cpu
            .cycles
            .wrapping_add(u64::from(CYCLES_PER_FRAME) * 2);
        while !self.frame_ready() && self.cpu.cycles < guard {
            if self.step_instruction().is_none() && self.cpu.is_suspended() {
                break;
            }
        }
        self.finish_frame();
    }

    pub fn step_instruction(&mut self) -> Option<FetchedInstruction> {
        self.step_instruction_inner(false).0
    }

    pub fn step_instruction_with_bus_trace(
        &mut self,
    ) -> (Option<FetchedInstruction>, Vec<DebugTraceEvent>) {
        self.step_instruction_inner(true)
    }

    fn step_instruction_inner(
        &mut self,
        collect_bus_trace: bool,
    ) -> (Option<FetchedInstruction>, Vec<DebugTraceEvent>) {
        if self.cpu.is_suspended() {
            return (None, Vec::new());
        }
        self.bus.debug_trace_enabled = collect_bus_trace;
        if collect_bus_trace {
            self.bus.debug_trace_events.clear();
        }

        let fetched = self.cpu.step(&mut self.bus);
        let events = if collect_bus_trace {
            self.bus.debug_trace_enabled = false;
            self.bus.take_debug_trace_events()
        } else {
            Vec::new()
        };
        (fetched, events)
    }

    pub fn finish_frame(&mut self) {
        if !self.bus.ppu.frame_ready {
            self.bus.render_frame();
        }
        self.frame_count = self.frame_count.wrapping_add(1);
    }

    pub fn framebuffer(&self) -> &[u8] {
        self.bus.ppu.framebuffer()
    }

    pub fn framebuffer_dimensions(&self) -> (usize, usize) {
        self.bus.ppu.dimensions()
    }

    pub fn frame_ready(&self) -> bool {
        self.bus.ppu.frame_ready
    }

    pub fn clear_frame_ready(&mut self) {
        self.bus.ppu.frame_ready = false;
    }

    pub fn drain_audio_samples_into(&mut self, _buf: &mut Vec<f32>) {}

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.sample_rate = rate;
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.apu_sample_generation_enabled = enabled;
    }

    pub fn apu_sample_generation_enabled(&self) -> bool {
        self.apu_sample_generation_enabled
    }

    pub fn set_apu_channel_mutes(&mut self, _mutes: [bool; 1]) {}

    pub fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.bus
            .keypad
            .set_host_input(buttons_pressed, dpad_pressed);
    }

    pub fn has_battery(&self) -> bool {
        self.bus.cartridge.has_battery()
    }

    pub fn dump_battery_sram(&self) -> Option<Vec<u8>> {
        self.bus.cartridge.dump_battery_data()
    }

    pub fn load_battery_sram(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.bus.cartridge.load_battery_data(bytes)
    }

    pub fn encode_state(&self) -> anyhow::Result<Vec<u8>> {
        crate::save_state::encode_state(self)
    }

    pub fn load_state(&mut self, data: &[u8]) -> anyhow::Result<()> {
        crate::save_state::decode_state(self, data)
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

    pub fn footer(&self) -> &RomFooter {
        self.bus.cartridge.footer()
    }

    pub fn is_cpu_suspended(&self) -> bool {
        self.cpu.is_suspended()
    }

    pub fn cpu_state(&self) -> CpuState {
        self.cpu.state
    }

    pub fn cpu_pc(&self) -> u32 {
        self.cpu.pc()
    }

    pub fn cpu_registers(&self) -> [u16; 8] {
        self.cpu.regs
    }

    pub fn cpu_segments(&self) -> [u16; 4] {
        self.cpu.segments
    }

    pub fn cpu_ip(&self) -> u16 {
        self.cpu.ip
    }

    pub fn cpu_flags(&self) -> u16 {
        self.cpu.flags
    }

    pub fn cpu_cycles(&self) -> u64 {
        self.cpu.cycles
    }

    pub fn cpu_last_opcode(&self) -> u8 {
        self.cpu.last_opcode
    }

    pub fn last_fetch(&self) -> Option<FetchedInstruction> {
        self.cpu.last_fetch
    }

    pub fn last_trap(&self) -> Option<CpuTrap> {
        self.cpu.last_trap
    }

    pub fn cpu_peek8(&self, addr: u32) -> u8 {
        self.bus.peek8(addr)
    }

    pub fn cpu_peek16(&self, addr: u32) -> u16 {
        self.bus.peek16(addr)
    }

    pub fn cpu_write8(&mut self, addr: u32, value: u8) {
        self.bus.write8(addr, value);
    }

    pub fn io_peek8(&self, port: u16) -> u8 {
        self.bus.io_peek8(port)
    }

    pub fn io_write8(&mut self, port: u16, value: u8) {
        self.bus.io_write8(port, value);
    }

    pub fn ppu_debug_snapshot(&self) -> crate::hardware::ppu::PpuDebugSnapshot {
        self.bus.ppu_debug_snapshot()
    }
}

impl fmt::Debug for Emulator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Emulator")
            .field("pc", &format_args!("{:#07X}", self.cpu_pc()))
            .field("state", &self.cpu.state)
            .field("cycles", &self.cpu.cycles)
            .field("frame_count", &self.frame_count)
            .field("rom_crc32", &format_args!("{:#010X}", self.rom_crc32))
            .field("footer", &self.bus.cartridge.footer())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::cartridge::compute_footer_checksum;

    fn rom_with_reset_code(code: &[u8]) -> Vec<u8> {
        let mut rom = vec![0xFF; 0x10000];
        rom[..code.len()].copy_from_slice(code);
        let reset = rom.len() - 16;
        rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
        let footer = rom.len() - 10;
        rom[footer + 4] = 0x01;
        let checksum = compute_footer_checksum(&rom);
        rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
        rom
    }

    #[test]
    fn loads_and_steps_minimal_rom() {
        let rom = rom_with_reset_code(&[0x90, 0xF4]);
        let mut emu = Emulator::from_rom_data(&rom).unwrap();
        assert_eq!(emu.framebuffer_dimensions(), (224, 144));
        assert_eq!(emu.cpu_pc(), 0xFFFF0);
        assert_eq!(emu.step_instruction().unwrap().opcode, 0xEA);
        assert_eq!(emu.step_instruction().unwrap().opcode, 0x90);
        assert_eq!(emu.step_instruction().unwrap().opcode, 0xF4);
        assert_eq!(emu.cpu_state(), CpuState::Halted);
    }

    #[test]
    fn step_frame_produces_framebuffer() {
        let rom = rom_with_reset_code(&[0xF4]);
        let mut emu = Emulator::from_rom_data(&rom).unwrap();
        emu.step_frame();
        assert!(emu.frame_ready());
        assert_eq!(emu.framebuffer().len(), 224 * 144 * 4);
        assert_eq!(emu.frame_count, 1);
    }

    #[test]
    fn bus_trace_records_instruction_fetches_and_io() {
        let rom = rom_with_reset_code(&[0xB0, 0x04, 0xE6, 0xC2, 0xF4]);
        let mut emu = Emulator::from_rom_data(&rom).unwrap();
        emu.step_instruction();
        emu.step_instruction();
        let (_, trace) = emu.step_instruction_with_bus_trace();
        assert!(trace.iter().any(|event| {
            matches!(
                event,
                DebugTraceEvent::IoWrite {
                    port: 0x00C2,
                    new_value: 4,
                    ..
                }
            )
        }));
    }
}
