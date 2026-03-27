use crate::hardware::bus::Bus;
use crate::hardware::cartridge::Cartridge;
use crate::hardware::cpu::{Cpu, CpuState};
use sha2::{Digest, Sha256};
use std::fmt;

mod runtime;

pub use crate::hardware::constants::CPU_CYCLES_PER_FRAME;

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
        let bus = Bus::new(cartridge, sample_rate);

        let rom_hash: [u8; 32] = Sha256::digest(rom_data).into();
        let rom_crc32 = crc32fast::hash(rom_data);

        let mut emu = Self {
            cpu: Cpu::new(),
            bus,
            rom_hash,
            rom_crc32,
            opcode_log: crate::debug::OpcodeLog::new(),
            debug: crate::debug::DebugController::new(),
        };
        emu.reset();
        Ok(emu)
    }

    pub fn reset(&mut self) {
        self.cpu = Cpu::new();
        self.cpu.reset(&mut self.bus);
        self.opcode_log.clear();
        self.debug.clear_hits();
    }

    pub fn framebuffer(&self) -> &[u8] {
        &self.bus.ppu.framebuffer[..]
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

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.bus.apu.output_sample_rate = rate as f64;
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

    pub fn apu_channel_snapshot(&self) -> crate::hardware::apu::ApuChannelSnapshot {
        self.bus.apu.channel_snapshot()
    }

    pub fn set_input_p1(&mut self, buttons: u8) {
        self.bus.controller1.set_buttons(buttons);
    }

    pub fn set_input_p2(&mut self, buttons: u8) {
        self.bus.controller2.set_buttons(buttons);
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

    pub fn cpu_write(&mut self, addr: u16, value: u8) {
        self.bus.cpu_write(addr, value);
    }

    pub fn cpu_peek(&self, addr: u16) -> u8 {
        self.bus.cpu_peek(addr)
    }

    pub fn cartridge_header(&self) -> &crate::hardware::cartridge::RomHeader {
        self.bus.cartridge.header()
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
