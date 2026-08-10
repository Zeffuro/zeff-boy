use crate::hardware::bus::{Bus, DebugTraceEvent};
use crate::hardware::cartridge::{Cartridge, MinimumSystem, RomFooter, RomOrientation};
use crate::hardware::constants::CYCLES_PER_FRAME;
use crate::hardware::cpu::{Cpu, CpuState, CpuTrap, FetchedInstruction};
use sha2::{Digest, Sha256};
use std::fmt;
use zeff_emu_common::address::Address;
use zeff_emu_common::debug::{
    AddressDebugController, AddressWatchHit, AddressWatchpoint, WatchType,
};
use zeff_emu_common::save_ram::SaveRamKind;

pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;

pub struct Emulator {
    pub(crate) cpu: Cpu,
    pub(crate) bus: Bus,
    pub(crate) rom_hash: [u8; 32],
    pub(crate) rom_crc32: u32,
    pub(crate) frame_count: u64,
    pub(crate) debug: AddressDebugController,
}

impl Emulator {
    pub fn new(rom_data: &[u8], sample_rate: u32) -> anyhow::Result<Self> {
        let cartridge = Cartridge::load(rom_data)?;
        let rom_hash: [u8; 32] = Sha256::digest(rom_data).into();
        let rom_crc32 = crc32fast::hash(rom_data);
        let mut bus = Bus::new(cartridge);
        bus.apu.set_sample_rate(sample_rate);
        let mut emu = Self {
            cpu: Cpu::new(),
            bus,
            rom_hash,
            rom_crc32,
            frame_count: 0,
            debug: AddressDebugController::new(),
        };
        emu.reset();
        Ok(emu)
    }

    pub fn from_rom_data(rom_data: &[u8]) -> anyhow::Result<Self> {
        Self::new(rom_data, DEFAULT_SAMPLE_RATE)
    }

    pub fn reset(&mut self) {
        self.bus.reset();
        self.cpu.apply_cartridge_start_state(
            self.bus.cartridge.minimum_system() != MinimumSystem::WonderSwan,
        );
        self.frame_count = 0;
        self.debug.clear_hits();
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
        self.step_instruction_inner(false, false).0
    }

    pub fn step_instruction_with_bus_trace(
        &mut self,
    ) -> (Option<FetchedInstruction>, Vec<DebugTraceEvent>) {
        self.step_instruction_inner(false, true)
    }

    fn step_instruction_inner(
        &mut self,
        skip_breakpoint_check: bool,
        collect_bus_trace: bool,
    ) -> (Option<FetchedInstruction>, Vec<DebugTraceEvent>) {
        if self.cpu.is_suspended() {
            return (None, Vec::new());
        }

        let pc = Address::from(self.cpu.pc());
        if !skip_breakpoint_check && self.debug.should_break(pc) {
            self.cpu.suspend();
            return (None, Vec::new());
        }

        let watch_active = !self.debug.watchpoints.is_empty();
        let trace_active = watch_active || collect_bus_trace;
        self.bus.debug_trace_enabled = trace_active;
        if trace_active {
            self.bus.debug_trace_events.clear();
        }

        let fetched = self.cpu.step(&mut self.bus);
        if fetched.is_some() {
            self.bus.retire_instruction();
        }
        let events = if trace_active {
            self.bus.debug_trace_enabled = false;
            self.bus.take_debug_trace_events()
        } else {
            Vec::new()
        };

        if watch_active {
            for event in &events {
                match *event {
                    DebugTraceEvent::Read { addr, value } => {
                        self.debug.check_watch_read(Address::from(addr), value);
                    }
                    DebugTraceEvent::Write {
                        addr,
                        old_value,
                        new_value,
                    } => {
                        self.debug
                            .check_watch_write(Address::from(addr), old_value, new_value);
                    }
                    DebugTraceEvent::IoRead { .. } | DebugTraceEvent::IoWrite { .. } => {}
                }
            }
            if self.debug.hit_watchpoint.is_some() {
                self.cpu.suspend();
            }
        }

        let bus_trace_events = if collect_bus_trace {
            events
        } else {
            Vec::new()
        };
        (fetched, bus_trace_events)
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

    pub fn system_ram(&self) -> &[u8] {
        &self.bus.ram
    }

    pub fn vram_snapshot(&self) -> &[u8] {
        &self.bus.ram
    }

    pub fn video_ram_snapshot(&self) -> &[u8] {
        self.vram_snapshot()
    }

    pub fn frame_ready(&self) -> bool {
        self.bus.ppu.frame_ready
    }

    pub fn clear_frame_ready(&mut self) {
        self.bus.ppu.frame_ready = false;
    }

    pub fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.bus.apu.drain_audio_samples_into(buf);
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.bus.apu.set_sample_rate(rate);
    }

    pub fn sample_rate(&self) -> u32 {
        self.bus.apu.sample_rate()
    }

    pub fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.bus.apu.set_sample_generation_enabled(enabled);
    }

    pub fn apu_sample_generation_enabled(&self) -> bool {
        self.bus.apu.sample_generation_enabled()
    }

    pub fn set_apu_channel_mutes(&mut self, mutes: [bool; 4]) {
        self.bus.apu.set_channel_mutes(mutes);
    }

    pub fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        if self
            .bus
            .keypad
            .set_host_input(buttons_pressed, dpad_pressed)
        {
            self.bus.raise_keypad_interrupt();
        }
    }

    pub fn has_battery(&self) -> bool {
        self.bus.cartridge.has_battery()
    }

    pub fn save_ram_kind(&self) -> SaveRamKind {
        self.bus.cartridge.save_ram_kind()
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

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    pub fn rom_crc32(&self) -> u32 {
        self.rom_crc32
    }

    pub fn cartridge_rom_bytes(&self) -> &[u8] {
        self.bus.cartridge.rom()
    }

    pub fn footer(&self) -> &RomFooter {
        self.bus.cartridge.footer()
    }

    pub fn preferred_orientation(&self) -> RomOrientation {
        self.footer().orientation()
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

    pub fn cpu_read8_debuggable(&mut self, addr: u32) -> u8 {
        let value = self.bus.peek8(addr);
        self.debug.check_watch_read(Address::from(addr), value);
        value
    }

    pub fn cpu_peek16(&self, addr: u32) -> u16 {
        self.bus.peek16(addr)
    }

    pub fn cpu_write8(&mut self, addr: u32, value: u8) {
        let old = self.bus.peek8(addr);
        self.bus.write8(addr, value);
        self.debug
            .check_watch_write(Address::from(addr), old, value);
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

    pub fn apu_debug_snapshot(&self) -> crate::hardware::apu::ApuDebugSnapshot {
        self.bus.apu_debug_snapshot()
    }

    pub fn debug_continue(&mut self) {
        self.debug.clear_hits();
        self.debug.break_on_next = false;
        self.cpu.resume();
    }

    pub fn debug_step(&mut self) {
        self.debug.clear_hits();
        self.debug.break_on_next = false;
        self.cpu.resume();
        let _ = self.step_instruction_inner(true, false);
        self.cpu.suspend();
    }

    pub fn add_breakpoint(&mut self, addr: Address) {
        self.debug.add_breakpoint(addr);
    }

    pub fn remove_breakpoint(&mut self, addr: Address) {
        self.debug.remove_breakpoint(addr);
    }

    pub fn toggle_breakpoint(&mut self, addr: Address) {
        self.debug.toggle_breakpoint(addr);
    }

    pub fn add_watchpoint(&mut self, addr: Address, watch_type: WatchType) {
        self.debug.add_watchpoint(addr, watch_type);
    }

    pub fn iter_breakpoints(&self) -> impl Iterator<Item = Address> + '_ {
        self.debug.iter_breakpoints()
    }

    pub fn debug_watchpoints(&self) -> &[AddressWatchpoint] {
        &self.debug.watchpoints
    }

    pub fn debug_hit_breakpoint(&self) -> Option<Address> {
        self.debug.hit_breakpoint
    }

    pub fn debug_hit_watchpoint(&self) -> Option<&AddressWatchHit> {
        self.debug.hit_watchpoint.as_ref()
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
            .field("debug", &self.debug)
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
        assert_eq!(
            emu.framebuffer().len(),
            crate::hardware::constants::FRAMEBUFFER_LEN
        );
        assert_eq!(emu.system_ram().len(), emu.video_ram_snapshot().len());
        assert_eq!(emu.save_ram_kind(), SaveRamKind::none());
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

    #[test]
    fn breakpoints_suspend_and_debug_step_executes_one_instruction() {
        let rom = rom_with_reset_code(&[0xF4]);
        let mut emu = Emulator::from_rom_data(&rom).unwrap();
        let start_pc = emu.cpu_pc();

        emu.add_breakpoint(start_pc);

        assert_eq!(emu.step_instruction(), None);
        assert!(emu.is_cpu_suspended());
        assert_eq!(emu.debug_hit_breakpoint(), Some(start_pc));

        emu.debug_step();

        assert!(emu.is_cpu_suspended());
        assert_eq!(emu.debug_hit_breakpoint(), None);
        assert_ne!(emu.cpu_pc(), start_pc);

        emu.debug_continue();

        assert!(!emu.is_cpu_suspended());
    }

    #[test]
    fn watchpoints_record_debuggable_reads_and_writes() {
        let rom = rom_with_reset_code(&[0xF4]);
        let mut emu = Emulator::from_rom_data(&rom).unwrap();

        emu.add_watchpoint(0x0000, WatchType::Write);
        assert_eq!(emu.debug_watchpoints().len(), 1);

        emu.cpu_write8(0x0000, 0x5A);

        let hit = emu
            .debug_hit_watchpoint()
            .expect("write watchpoint should hit");
        assert_eq!(hit.address, 0x0000);
        assert_eq!(hit.new_value, 0x5A);
        assert_eq!(hit.watch_type, WatchType::Write);

        emu.debug_continue();
        emu.add_watchpoint(0x0000, WatchType::Read);
        assert_eq!(emu.cpu_read8_debuggable(0x0000), 0x5A);

        let hit = emu
            .debug_hit_watchpoint()
            .expect("read watchpoint should hit");
        assert_eq!(hit.address, 0x0000);
        assert_eq!(hit.new_value, 0x5A);
        assert_eq!(hit.watch_type, WatchType::Read);
    }

    #[test]
    fn input_press_raises_enabled_keypad_interrupt() {
        let rom = rom_with_reset_code(&[0xF4]);
        let mut emu = Emulator::from_rom_data(&rom).unwrap();
        emu.io_write8(0xB0, 0x20);
        emu.io_write8(0xB2, 0x02);

        emu.set_input(0x01, 0x00);

        assert_eq!(emu.io_peek8(0xB4) & 0x02, 0x02);
    }
}
