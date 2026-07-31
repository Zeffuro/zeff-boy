use crate::hardware::bus::{Bus, DebugTraceEvent};
use crate::hardware::cartridge::{BackupKind, Cartridge, RomHeader};
use crate::hardware::constants::CYCLES_PER_FRAME;
use crate::hardware::cpu::{Cpu, CpuMode};
use sha2::{Digest, Sha256};
use std::fmt;
use zeff_emu_common::address::Address;
use zeff_emu_common::debug::{
    AddressDebugController, AddressWatchHit, AddressWatchpoint, WatchType,
};

pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;
const DEFAULT_IRQ_LOOKAHEAD_CYCLES: u32 = 3;
const TIMER_WORD_LOAD_IRQ_LOOKAHEAD_CYCLES: u32 = 6;
const LARGE_PRESCALED_TIMER_WORD_LOAD_IRQ_LOOKAHEAD_CYCLES: u32 = 7;
const LOOSE_PRESCALED_TIMER_WORD_LOAD_IRQ_LOOKAHEAD_CYCLES: u32 = 8;
const LARGE_PRESCALED_TIMER_FUTURE_IRQ_LOOKAHEAD_CYCLES: u32 = 9;
const PRESCALED_TIMER_WORD_LOAD_IRQ_LOOKAHEAD_CYCLES: u32 = 4;
const LOOSE_TIMER_WORD_READ_GAP_CYCLES: u64 = 256;
const PRESCALED_LOOSE_TIMER_WORD_READ_GAP_CYCLES: u64 = 16;

pub struct Emulator {
    pub(crate) cpu: Cpu,
    pub(crate) bus: Bus,
    pub(crate) rom_hash: [u8; 32],
    pub(crate) frame_count: u64,
    pub(crate) debug: AddressDebugController,
}

impl Emulator {
    pub fn new(rom_data: &[u8], sample_rate: u32) -> anyhow::Result<Self> {
        let cartridge = Cartridge::load(rom_data)?;
        let rom_hash: [u8; 32] = Sha256::digest(rom_data).into();
        let mut emu = Self {
            cpu: Cpu::new(),
            bus: Bus::new(cartridge, sample_rate),
            rom_hash,
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
        self.cpu.reset();
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

    pub fn step_instruction(&mut self) -> Option<crate::hardware::cpu::FetchedInstruction> {
        self.step_instruction_inner(false, false, false).0
    }

    pub fn step_instruction_with_bus_trace(
        &mut self,
        trace_reads: bool,
        trace_writes: bool,
    ) -> (
        Option<crate::hardware::cpu::FetchedInstruction>,
        Vec<DebugTraceEvent>,
    ) {
        self.step_instruction_inner(trace_reads || trace_writes, trace_reads, trace_writes)
    }

    fn step_instruction_inner(
        &mut self,
        collect_bus_trace: bool,
        trace_reads: bool,
        trace_writes: bool,
    ) -> (
        Option<crate::hardware::cpu::FetchedInstruction>,
        Vec<DebugTraceEvent>,
    ) {
        if self.cpu.is_suspended() {
            return (None, Vec::new());
        }
        let next_instruction_reads_timer_word =
            self.cpu.next_instruction_reads_timer_word(&self.bus);
        let irq_lookahead_cycles = if next_instruction_reads_timer_word
            && (self.bus.prescaled_loose_timer_interrupt_pending_after_services(3)
                || self.bus.prescaled_loose_timer_irq_enabled_after_services(3))
            && self.cpu.cycles_since_last_timer_word_read()
                >= PRESCALED_LOOSE_TIMER_WORD_READ_GAP_CYCLES
        {
            LOOSE_PRESCALED_TIMER_WORD_LOAD_IRQ_LOOKAHEAD_CYCLES
        } else if next_instruction_reads_timer_word
            && self
                .bus
                .large_prescaled_loose_timer_irq_enabled_at_service_count(0)
            && self.cpu.cycles_since_last_timer_word_read()
                >= PRESCALED_LOOSE_TIMER_WORD_READ_GAP_CYCLES
        {
            LOOSE_PRESCALED_TIMER_WORD_LOAD_IRQ_LOOKAHEAD_CYCLES
        } else if next_instruction_reads_timer_word
            && self.bus.large_prescaled_timer_interrupt_pending_after_services(1)
            && self.cpu.cycles_since_last_timer_word_read()
                < PRESCALED_LOOSE_TIMER_WORD_READ_GAP_CYCLES
        {
            LARGE_PRESCALED_TIMER_WORD_LOAD_IRQ_LOOKAHEAD_CYCLES
        } else if next_instruction_reads_timer_word
            && self
                .bus
                .prescaled_timer_interrupt_pending_after_services(1)
            && self.cpu.cycles_since_last_timer_word_read()
                < PRESCALED_LOOSE_TIMER_WORD_READ_GAP_CYCLES
        {
            TIMER_WORD_LOAD_IRQ_LOOKAHEAD_CYCLES
        } else if next_instruction_reads_timer_word
            && self.bus.large_prescaled_timer_interrupt_pending()
            && self.cpu.cycles_since_last_timer_word_read()
                < PRESCALED_LOOSE_TIMER_WORD_READ_GAP_CYCLES
        {
            TIMER_WORD_LOAD_IRQ_LOOKAHEAD_CYCLES
        } else if next_instruction_reads_timer_word && self.bus.prescaled_timer_interrupt_pending()
        {
            PRESCALED_TIMER_WORD_LOAD_IRQ_LOOKAHEAD_CYCLES
        } else if next_instruction_reads_timer_word
            && self.bus.long_period_timer_interrupt_pending_after_services(3)
            && self.cpu.cycles_since_last_timer_word_read() >= LOOSE_TIMER_WORD_READ_GAP_CYCLES
        {
            TIMER_WORD_LOAD_IRQ_LOOKAHEAD_CYCLES
        } else if self
            .bus
            .large_prescaled_timer_irq_enabled_at_service_count(1, 1)
        {
            LARGE_PRESCALED_TIMER_FUTURE_IRQ_LOOKAHEAD_CYCLES
        } else {
            DEFAULT_IRQ_LOOKAHEAD_CYCLES
        };
        self.try_service_ready_irq(irq_lookahead_cycles);
        if self.debug.should_break(self.cpu.pc()) {
            self.cpu.suspend();
            return (None, Vec::new());
        }
        if self.cpu.state == crate::hardware::cpu::CpuState::Halted {
            let cycles = self.bus.cycles_until_next_halt_check();
            self.cpu.cycles = self.cpu.cycles.wrapping_add(u64::from(cycles));
            self.bus.step_cycles(cycles);
            if self.bus.interrupt_ready() {
                self.cpu.resume();
            }
            return (None, Vec::new());
        }

        self.bus.debug_trace_enabled = collect_bus_trace;
        self.bus.debug_trace_reads = trace_reads;
        self.bus.debug_trace_writes = trace_writes;
        if collect_bus_trace {
            self.bus.debug_trace_events.borrow_mut().clear();
        }

        let before_cycles = self.cpu.cycles;
        let fetched = self.cpu.step(&mut self.bus);
        let elapsed = self
            .cpu
            .cycles
            .wrapping_sub(before_cycles)
            .min(u64::from(u32::MAX));
        let dma_cycles = self.bus.take_pending_dma_cycles();
        self.cpu.cycles = self.cpu.cycles.wrapping_add(u64::from(dma_cycles));
        self.bus
            .timers
            .begin_step_window((elapsed as u32).saturating_add(dma_cycles));
        self.bus
            .step_cycles((elapsed as u32).saturating_add(dma_cycles));

        let bus_trace_events = if collect_bus_trace {
            self.bus.debug_trace_enabled = false;
            self.bus.debug_trace_reads = false;
            self.bus.debug_trace_writes = false;
            std::mem::take(&mut *self.bus.debug_trace_events.borrow_mut())
        } else {
            Vec::new()
        };

        (fetched, bus_trace_events)
    }

    fn try_service_ready_irq(&mut self, lookahead_cycles: u32) {
        if !self.cpu.irq_enabled() || !self.bus.irq_handler_installed() {
            return;
        }

        let irq_delay_cycles =
            if self.bus.interrupt_ready_with_lookahead(lookahead_cycles) {
                self.bus
                    .take_irq_sample_delay_cycles_with_lookahead(lookahead_cycles)
            } else if let Some(cycles_until_ready) = self
                .bus
                .cycles_until_timer_irq_ready()
                .filter(|&cycles| cycles <= lookahead_cycles)
            {
                if std::env::var_os("ZEFF_GBA_TIMER_TRACE").is_some() {
                    let (reload, counter, control, irq_services) = self.bus.debug_timer0();
                    eprintln!(
                        "IRQ FUTURE cyc={} pc={:08X} look={} until={} gap={} r0={:08X} r2={:08X} t0={:04X}/{:04X}/{:04X} svc={}",
                        self.cpu.cycles,
                        self.cpu.pc(),
                        lookahead_cycles,
                        cycles_until_ready,
                        self.cpu.cycles_since_last_timer_word_read(),
                        self.cpu.regs[0],
                        self.cpu.regs[2],
                        reload,
                        counter,
                        control,
                        irq_services
                    );
                }
                self.cpu.cycles = self.cpu.cycles.wrapping_add(u64::from(cycles_until_ready));
                self.bus.step_cycles(cycles_until_ready);
                self.bus.take_irq_sample_delay_cycles_with_lookahead(0)
            } else {
                return;
            };
        if irq_delay_cycles != 0 {
            self.cpu.cycles = self.cpu.cycles.wrapping_add(u64::from(irq_delay_cycles));
            self.bus.step_cycles(irq_delay_cycles);
        }
        if self.cpu.try_service_irq(true) {
            self.bus.note_irq_service(
                irq_delay_cycles,
                self.cpu
                    .cycles_since_last_timer_word_read()
                    .min(u64::from(u32::MAX)) as u32,
            );
        }
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

    pub fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.bus.apu.drain_samples_into(buf);
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.bus.apu.set_sample_rate(rate);
    }

    pub fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.bus.apu.set_sample_generation_enabled(enabled);
    }

    pub fn set_apu_channel_mutes(&mut self, mutes: [bool; 6]) {
        self.bus.apu.set_channel_mutes(mutes);
    }

    pub fn apu_debug_snapshot(&self) -> crate::hardware::apu::ApuDebugSnapshot {
        self.bus.apu.debug_snapshot()
    }

    pub fn dma_channels_snapshot(&self) -> [crate::hardware::dma::DmaChannel; 4] {
        self.bus.dma.channels()
    }

    pub fn set_ppu_debug_flags(&mut self, bg: bool, window: bool, sprites: bool) {
        self.bus.set_ppu_debug_flags(bg, window, sprites);
    }

    pub fn set_ppu_debug_bg_layers(&mut self, layers: [bool; 4]) {
        self.bus.set_ppu_debug_bg_layers(layers);
    }

    pub fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.bus
            .keypad
            .set_host_input(buttons_pressed, dpad_pressed);
    }

    pub fn is_cpu_suspended(&self) -> bool {
        self.cpu.is_suspended()
    }

    pub fn cpu_state(&self) -> crate::hardware::cpu::CpuState {
        self.cpu.state
    }

    pub fn cpu_pc(&self) -> u32 {
        self.cpu.pc()
    }

    pub fn cpu_registers(&self) -> [u32; 16] {
        self.cpu.regs
    }

    pub fn cpu_cpsr(&self) -> u32 {
        self.cpu.cpsr
    }

    pub fn cpu_mode(&self) -> CpuMode {
        self.cpu.mode()
    }

    pub fn cpu_thumb_state(&self) -> bool {
        self.cpu.thumb_state()
    }

    pub fn cpu_visible_pc(&self) -> u32 {
        self.cpu.visible_pc()
    }

    pub fn last_fetch(&self) -> Option<crate::hardware::cpu::FetchedInstruction> {
        self.cpu.last_fetch
    }

    pub fn cpu_cycles(&self) -> u64 {
        self.cpu.cycles
    }

    pub fn cpu_write8(&mut self, addr: u32, value: u8) {
        let old = self.bus.read8(addr);
        self.bus.write8(addr, value);
        self.debug.check_watch_write(addr, old, value);
    }

    pub fn cpu_peek8(&self, addr: u32) -> u8 {
        self.bus.read8(addr)
    }

    pub fn cpu_read8_debuggable(&mut self, addr: u32) -> u8 {
        let value = self.bus.read8(addr);
        self.debug.check_watch_read(addr, value);
        value
    }

    pub fn cpu_write16(&mut self, addr: u32, value: u16) {
        self.bus.write16(addr, value);
    }

    pub fn cpu_peek16(&self, addr: u32) -> u16 {
        self.bus.read16(addr)
    }

    pub fn cpu_write32(&mut self, addr: u32, value: u32) {
        self.bus.write32(addr, value);
    }

    pub fn cpu_peek32(&self, addr: u32) -> u32 {
        self.bus.read32(addr)
    }

    pub fn cartridge_header(&self) -> &RomHeader {
        self.bus.cartridge.header()
    }

    pub fn cartridge_rom_bytes(&self) -> &[u8] {
        self.bus.cartridge.rom()
    }

    pub fn backup_kind(&self) -> BackupKind {
        self.bus.cartridge.backup_kind()
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

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    pub fn system_ram(&self) -> (&[u8], &[u8]) {
        self.bus.system_ram()
    }

    pub fn vram_snapshot(&self) -> &[u8] {
        &self.bus.vram
    }

    pub fn palette_ram_snapshot(&self) -> &[u8] {
        &self.bus.palette_ram
    }

    pub fn io_snapshot(&self) -> &[u8] {
        &self.bus.io
    }

    pub fn oam_snapshot(&self) -> &[u8] {
        &self.bus.oam
    }

    pub fn ppu_debug_snapshot(&self) -> crate::hardware::ppu::PpuDebugSnapshot {
        self.bus.ppu_debug_snapshot()
    }

    pub fn debug_continue(&mut self) {
        self.debug.clear_hits();
        self.debug.break_on_next = false;
        self.cpu.resume();
    }

    pub fn debug_step(&mut self) {
        self.debug.clear_hits();
        self.debug.break_on_next = false;
        self.cpu.request_debug_step();
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
        f.debug_struct("GBA Emulator")
            .field("cpu", &self.cpu)
            .field("debug", &self.debug)
            .field("title", &self.bus.cartridge.header().title)
            .field("backup_kind", &self.bus.cartridge.backup_kind())
            .field("frame_count", &self.frame_count)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_rom() -> Vec<u8> {
        let mut rom = vec![0; 0xC0];
        rom[0xA0..0xA4].copy_from_slice(b"TEST");
        rom[0xB2] = 0x96;
        rom
    }

    #[test]
    fn breakpoint_suspends_stub_cpu() {
        let rom = minimal_rom();
        let mut emu = Emulator::new(&rom, 48_000).unwrap();
        emu.add_breakpoint(0x0800_0000);
        emu.step_frame();
        assert!(emu.is_cpu_suspended());
        assert_eq!(emu.debug_hit_breakpoint(), Some(0x0800_0000));
    }

    #[test]
    fn watchpoint_hits_on_debug_write() {
        let rom = minimal_rom();
        let mut emu = Emulator::new(&rom, 48_000).unwrap();
        emu.add_watchpoint(0x0200_0000, WatchType::Write);
        emu.cpu_write8(0x0200_0000, 0x5A);
        let hit = emu.debug_hit_watchpoint().expect("watchpoint should hit");
        assert_eq!(hit.address, 0x0200_0000);
        assert_eq!(hit.new_value, 0x5A);
    }

    #[test]
    fn halted_cpu_wakes_on_exact_hblank_interrupt_cycle() {
        let rom = minimal_rom();
        let mut emu = Emulator::new(&rom, 48_000).unwrap();
        emu.cpu_write16(0x0400_0004, 1 << 4);
        emu.cpu_write16(0x0400_0200, 1 << 1);
        emu.cpu_write16(0x0400_0208, 1);
        emu.cpu.state = crate::hardware::cpu::CpuState::Halted;

        while emu.cpu.state == crate::hardware::cpu::CpuState::Halted {
            emu.step_instruction();
        }

        assert_eq!(emu.cpu_cycles(), 1013);
    }
}
