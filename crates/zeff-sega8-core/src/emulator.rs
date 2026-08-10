use std::path::Path;

use sha2::{Digest, Sha256};
use zeff_emu_common::address::Address;
use zeff_emu_common::debug::{
    AddressDebugController, AddressWatchHit, AddressWatchpoint, OpcodeLog, WatchType,
};
use zeff_emu_common::save_ram::SaveRamKind;

use crate::hardware::apu::PSG_CHANNEL_COUNT;
use crate::hardware::bus::{Bus, CpuAccessTraceEvent};
use crate::hardware::cartridge::{Cartridge, Sega8System, SystemHint};
use crate::hardware::constants::{
    GG_SCREEN_H, GG_SCREEN_W, GG_VIEWPORT_X, GG_VIEWPORT_Y, RGBA_CHANNELS, SMS_SCREEN_H,
    SMS_SCREEN_W, SMS_Z80_CYCLES_PER_FRAME,
};
use crate::hardware::cpu::{Cpu, CpuTrap, FetchedInstruction};
use crate::hardware::input::ControllerPort;
use crate::hardware::vdp::{Mode4ColorMode, Mode4RenderArea};

pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;
const HOST_DPAD_RIGHT: u8 = 1 << 0;
const HOST_DPAD_LEFT: u8 = 1 << 1;
const HOST_DPAD_UP: u8 = 1 << 2;
const HOST_DPAD_DOWN: u8 = 1 << 3;
const HOST_BUTTON_1: u8 = 1 << 0;
const HOST_BUTTON_2: u8 = 1 << 1;
const SMS_PAD_UP: u8 = 1 << 0;
const SMS_PAD_DOWN: u8 = 1 << 1;
const SMS_PAD_LEFT: u8 = 1 << 2;
const SMS_PAD_RIGHT: u8 = 1 << 3;
const SMS_PAD_BUTTON_1: u8 = 1 << 4;
const SMS_PAD_BUTTON_2: u8 = 1 << 5;

#[derive(Debug)]
pub struct Emulator {
    pub(crate) cpu: Cpu,
    pub(crate) bus: Bus,
    pub(crate) rom_hash: [u8; 32],
    pub(crate) frame_count: u64,
    pub(crate) framebuffer: Vec<u8>,
    pub(crate) sample_rate: u32,
    pub(crate) debug: AddressDebugController,
    pub(crate) opcode_log: OpcodeLog<(u16, u8, u32)>,
}

impl Emulator {
    pub fn new(rom_data: &[u8], sample_rate: u32) -> anyhow::Result<Self> {
        Self::new_with_hint(rom_data, sample_rate, SystemHint::Auto)
    }

    pub fn new_with_path_hint(
        rom_data: &[u8],
        sample_rate: u32,
        path: &Path,
    ) -> anyhow::Result<Self> {
        let hint = SystemHint::from_path(path).unwrap_or(SystemHint::Auto);
        Self::new_with_hint(rom_data, sample_rate, hint)
    }

    pub fn new_with_hint(
        rom_data: &[u8],
        sample_rate: u32,
        hint: SystemHint,
    ) -> anyhow::Result<Self> {
        let sample_rate = if sample_rate == 0 {
            DEFAULT_SAMPLE_RATE
        } else {
            sample_rate
        };
        let cartridge = Cartridge::load_with_hint(rom_data, hint)?;
        let rom_hash: [u8; 32] = Sha256::digest(rom_data).into();
        let framebuffer = vec![0; framebuffer_len(cartridge.system())];
        Ok(Self {
            cpu: Cpu::new(),
            bus: Bus::new_with_sample_rate(cartridge, sample_rate),
            rom_hash,
            frame_count: 0,
            framebuffer,
            sample_rate,
            debug: AddressDebugController::new(),
            opcode_log: OpcodeLog::new(),
        })
    }

    pub fn from_rom_data(rom_data: &[u8]) -> anyhow::Result<Self> {
        Self::new(rom_data, DEFAULT_SAMPLE_RATE)
    }

    pub fn reset(&mut self) {
        self.cpu.reset();
        self.bus.reset();
        self.frame_count = 0;
        self.framebuffer.fill(0);
        self.debug.clear_hits();
        self.opcode_log.clear();
    }

    pub fn step_frame(&mut self) {
        if self.cpu.is_suspended() {
            return;
        }
        let target_cycles = self
            .cpu
            .cycles()
            .wrapping_add(u64::from(SMS_Z80_CYCLES_PER_FRAME));
        while self.cpu.cycles() < target_cycles {
            if self.step_instruction().is_none() || self.cpu.is_suspended() {
                return;
            }
        }
        self.finish_frame();
    }

    pub fn finish_frame(&mut self) {
        self.render_frame();
        self.frame_count = self.frame_count.wrapping_add(1);
    }

    pub fn step_instruction(&mut self) -> Option<FetchedInstruction> {
        self.step_instruction_inner(false, false).0
    }

    pub fn step_instruction_with_bus_trace(
        &mut self,
    ) -> (Option<FetchedInstruction>, Vec<CpuAccessTraceEvent>) {
        self.step_instruction_inner(false, true)
    }

    fn step_instruction_inner(
        &mut self,
        skip_breakpoint_check: bool,
        collect_bus_trace: bool,
    ) -> (Option<FetchedInstruction>, Vec<CpuAccessTraceEvent>) {
        if self.cpu.is_suspended() {
            return (None, Vec::new());
        }

        let pc = Address::from(self.cpu.regs().pc);
        if !skip_breakpoint_check && self.debug.should_break(pc) {
            self.cpu.suspend();
            return (None, Vec::new());
        }

        let watch_active = !self.debug.watchpoints.is_empty();
        let trace_active = watch_active || collect_bus_trace;
        if trace_active {
            self.bus.begin_cpu_access_trace();
        }

        let fetched = self.cpu.step(&mut self.bus);
        if let Some(instruction) = fetched {
            self.bus.step_cycles(instruction.cycles);
            self.opcode_log
                .push((instruction.pc, instruction.opcode, instruction.cycles));
        }

        let mut bus_trace_events = Vec::new();
        if trace_active {
            let events = self.bus.drain_cpu_access_trace();
            if collect_bus_trace {
                bus_trace_events = events.clone();
            }
            if watch_active {
                self.apply_cpu_access_trace_watchpoints(events);
            }
            if self.debug.hit_watchpoint.is_some() {
                self.cpu.suspend();
            }
        }

        (fetched, bus_trace_events)
    }

    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    pub fn framebuffer_dimensions(&self) -> (usize, usize) {
        dimensions_for_system(self.system())
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    pub fn system(&self) -> Sega8System {
        self.bus.cartridge.system()
    }

    pub fn save_ram_kind(&self) -> SaveRamKind {
        self.bus.cartridge.save_ram_kind()
    }

    pub fn bus(&self) -> &Bus {
        &self.bus
    }

    pub fn bus_mut(&mut self) -> &mut Bus {
        &mut self.bus
    }

    pub fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    pub fn cpu_trap(&self) -> Option<CpuTrap> {
        self.cpu.trap()
    }

    pub fn rom_hash(&self) -> [u8; 32] {
        self.rom_hash
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = if sample_rate == 0 {
            DEFAULT_SAMPLE_RATE
        } else {
            sample_rate
        };
        self.bus.set_apu_sample_rate(self.sample_rate);
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.bus.drain_audio_samples_into(buf);
    }

    pub fn drain_audio_samples(&mut self) -> Vec<f32> {
        let mut buf = Vec::new();
        self.drain_audio_samples_into(&mut buf);
        buf
    }

    pub fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.bus.set_apu_sample_generation_enabled(enabled);
    }

    pub fn set_apu_channel_mutes(&mut self, mutes: [bool; PSG_CHANNEL_COUNT]) {
        self.bus.set_apu_channel_mutes(mutes);
    }

    pub fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        let mut raw = 0xFF;
        if dpad_pressed & HOST_DPAD_UP != 0 {
            raw &= !SMS_PAD_UP;
        }
        if dpad_pressed & HOST_DPAD_DOWN != 0 {
            raw &= !SMS_PAD_DOWN;
        }
        if dpad_pressed & HOST_DPAD_LEFT != 0 {
            raw &= !SMS_PAD_LEFT;
        }
        if dpad_pressed & HOST_DPAD_RIGHT != 0 {
            raw &= !SMS_PAD_RIGHT;
        }
        if buttons_pressed & HOST_BUTTON_1 != 0 {
            raw &= !SMS_PAD_BUTTON_1;
        }
        if buttons_pressed & HOST_BUTTON_2 != 0 {
            raw &= !SMS_PAD_BUTTON_2;
        }
        self.bus
            .input_mut()
            .set_controller_raw(ControllerPort::One, raw);
    }

    pub fn has_battery(&self) -> bool {
        self.save_ram_kind().is_battery_backed()
    }

    pub fn dump_battery_sram(&self) -> Option<Vec<u8>> {
        self.has_battery()
            .then(|| self.bus.cartridge_ram().to_vec())
    }

    pub fn load_battery_sram(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        if !self.has_battery() {
            anyhow::bail!(
                "Sega 8-bit ROM does not declare known battery-backed SRAM; classified as {:?}",
                self.save_ram_kind()
            );
        }
        self.bus.load_cartridge_ram(bytes)
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

    pub fn is_suspended(&self) -> bool {
        self.cpu.is_suspended()
    }

    pub fn suspend(&mut self) {
        self.cpu.suspend();
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

    pub fn debug_write(&mut self, addr: Address, val: u8) {
        let addr16 = addr as u16;
        let old_value = self.bus.cpu_read(addr16);
        self.bus.cpu_write(addr16, val);
        self.debug
            .check_watch_write(Address::from(addr16), old_value, val);
        if self.debug.hit_watchpoint.is_some() {
            self.cpu.suspend();
        }
    }

    pub fn set_opcode_log_enabled(&mut self, enabled: bool) {
        self.opcode_log.set_enabled(enabled);
    }

    pub fn recent_opcodes(&self, n: usize) -> Vec<(u16, u8, u32)> {
        self.opcode_log.recent(n)
    }

    fn apply_cpu_access_trace_watchpoints(&mut self, events: Vec<CpuAccessTraceEvent>) {
        for event in events {
            match event {
                CpuAccessTraceEvent::Read { addr, value } => {
                    self.debug.check_watch_read(Address::from(addr), value);
                }
                CpuAccessTraceEvent::Write {
                    addr,
                    old_value,
                    new_value,
                } => {
                    self.debug
                        .check_watch_write(Address::from(addr), old_value, new_value);
                }
                CpuAccessTraceEvent::IoRead { .. } | CpuAccessTraceEvent::IoWrite { .. } => {}
            }
            if self.debug.hit_watchpoint.is_some() {
                break;
            }
        }
    }

    fn render_frame(&mut self) {
        match self.system() {
            Sega8System::MasterSystem => {
                self.bus.vdp().render_mode4_frame_rgba(
                    &mut self.framebuffer,
                    Mode4RenderArea::new(SMS_SCREEN_W, SMS_SCREEN_H, 0, 0),
                    Mode4ColorMode::Sms,
                );
            }
            Sega8System::GameGear => {
                self.bus.vdp().render_mode4_frame_rgba(
                    &mut self.framebuffer,
                    Mode4RenderArea::new(GG_SCREEN_W, GG_SCREEN_H, GG_VIEWPORT_X, GG_VIEWPORT_Y),
                    Mode4ColorMode::GameGear,
                );
            }
            Sega8System::Sg1000 => {
                self.bus.vdp().render_tms9918_frame_rgba(
                    &mut self.framebuffer,
                    SMS_SCREEN_W,
                    SMS_SCREEN_H,
                );
            }
        }
    }
}

fn dimensions_for_system(system: Sega8System) -> (usize, usize) {
    match system {
        Sega8System::GameGear => (GG_SCREEN_W, GG_SCREEN_H),
        Sega8System::MasterSystem | Sega8System::Sg1000 => (SMS_SCREEN_W, SMS_SCREEN_H),
    }
}

fn framebuffer_len(system: Sega8System) -> usize {
    let (w, h) = dimensions_for_system(system);
    w * h * RGBA_CHANNELS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::cartridge::HeaderLocation;
    use crate::hardware::constants::{
        IO_PORT_VDP_CONTROL, IO_PORT_VDP_DATA, SEGA_HEADER_MAGIC, SEGA_HEADER_SIZE,
        SMS_MODE4_TILE_BYTES, VDP_CONTROL_REGISTER_WRITE_VALUE,
    };

    fn rom_with_header(location: HeaderLocation, region_size: u8) -> Vec<u8> {
        let mut rom = vec![0xFF; location.offset() + SEGA_HEADER_SIZE];
        let offset = location.offset();
        rom[offset..offset + SEGA_HEADER_MAGIC.len()].copy_from_slice(SEGA_HEADER_MAGIC);
        rom[offset + 0x0F] = region_size;
        rom
    }

    fn set_vdp_write_address(emu: &mut Emulator, addr: u16) {
        emu.bus_mut().io_write(IO_PORT_VDP_CONTROL, addr as u8);
        emu.bus_mut()
            .io_write(IO_PORT_VDP_CONTROL, 0x40 | ((addr >> 8) as u8 & 0x3F));
    }

    fn set_vdp_cram_write_address(emu: &mut Emulator, addr: u8) {
        emu.bus_mut().io_write(IO_PORT_VDP_CONTROL, addr);
        emu.bus_mut().io_write(IO_PORT_VDP_CONTROL, 0xC0);
    }

    fn set_vdp_register(emu: &mut Emulator, register: u8, value: u8) {
        emu.bus_mut().io_write(IO_PORT_VDP_CONTROL, value);
        emu.bus_mut().io_write(
            IO_PORT_VDP_CONTROL,
            VDP_CONTROL_REGISTER_WRITE_VALUE | register,
        );
    }

    #[test]
    fn creates_master_system_emulator_from_auto_header() {
        let emu = Emulator::new(&rom_with_header(HeaderLocation::Offset0x7ff0, 0x4C), 44_100)
            .expect("SMS emulator should initialize");

        assert_eq!(emu.system(), Sega8System::MasterSystem);
        assert_eq!(emu.framebuffer_dimensions(), (SMS_SCREEN_W, SMS_SCREEN_H));
        assert_eq!(
            emu.framebuffer().len(),
            SMS_SCREEN_W * SMS_SCREEN_H * RGBA_CHANNELS
        );
        assert_eq!(emu.sample_rate(), 44_100);
    }

    #[test]
    fn creates_game_gear_emulator_from_auto_header() {
        let emu = Emulator::new(&rom_with_header(HeaderLocation::Offset0x3ff0, 0x7A), 48_000)
            .expect("GG emulator should initialize");

        assert_eq!(emu.system(), Sega8System::GameGear);
        assert_eq!(emu.framebuffer_dimensions(), (GG_SCREEN_W, GG_SCREEN_H));
        assert_eq!(
            emu.framebuffer().len(),
            GG_SCREEN_W * GG_SCREEN_H * RGBA_CHANNELS
        );
    }

    #[test]
    fn path_hint_selects_sg1000_for_headerless_sg_rom() {
        let emu = Emulator::new_with_path_hint(&[0x00, 0x76], 48_000, std::path::Path::new("a.sg"))
            .expect("SG emulator should initialize from path hint");

        assert_eq!(emu.system(), Sega8System::Sg1000);
        assert_eq!(emu.framebuffer_dimensions(), (SMS_SCREEN_W, SMS_SCREEN_H));
    }

    #[test]
    fn step_frame_renders_sg1000_tms9918_background_from_vdp_state() {
        let mut emu = Emulator::new_with_hint(&[0x00, 0x76], 48_000, SystemHint::Sg1000)
            .expect("SG emulator should initialize");

        assert_eq!(emu.frame_count(), 0);
        assert!(emu.framebuffer().iter().all(|&byte| byte == 0));

        set_vdp_register(&mut emu, 1, 0x40);
        set_vdp_register(&mut emu, 2, 0x0E);
        set_vdp_register(&mut emu, 3, 0x20);
        set_vdp_register(&mut emu, 4, 0x00);
        set_vdp_register(&mut emu, 7, 0x01);
        set_vdp_write_address(&mut emu, 8);
        emu.bus_mut().io_write(IO_PORT_VDP_DATA, 0x80);
        set_vdp_write_address(&mut emu, 0x0800);
        emu.bus_mut().io_write(IO_PORT_VDP_DATA, 0x60);
        set_vdp_write_address(&mut emu, 0x3800);
        emu.bus_mut().io_write(IO_PORT_VDP_DATA, 1);

        emu.step_frame();

        assert_eq!(emu.frame_count(), 1);
        assert_eq!(
            &emu.framebuffer()[..RGBA_CHANNELS],
            &[0xD4, 0x52, 0x4D, 0xFF]
        );
    }

    #[test]
    fn step_frame_renders_sms_mode4_background_from_vdp_state() {
        let mut emu = Emulator::new_with_hint(&[0x76], 48_000, SystemHint::MasterSystem)
            .expect("SMS emulator should initialize");

        set_vdp_register(&mut emu, 2, 0x0E);
        set_vdp_register(&mut emu, 1, 0x40);
        set_vdp_write_address(&mut emu, SMS_MODE4_TILE_BYTES as u16);
        for value in [0x80, 0x80, 0x00, 0x00] {
            emu.bus_mut().io_write(IO_PORT_VDP_DATA, value);
        }
        set_vdp_write_address(&mut emu, 0x3800);
        emu.bus_mut().io_write(IO_PORT_VDP_DATA, 1);
        emu.bus_mut().io_write(IO_PORT_VDP_DATA, 0);
        set_vdp_cram_write_address(&mut emu, 3);
        emu.bus_mut().io_write(IO_PORT_VDP_DATA, 0x03);

        emu.step_frame();

        assert_eq!(
            &emu.framebuffer()[0..RGBA_CHANNELS],
            &[0xFF, 0x00, 0x00, 0xFF]
        );
    }

    #[test]
    fn set_input_maps_host_masks_to_active_low_sms_controller_bits() {
        let mut emu = Emulator::new_with_hint(&[0x00], 48_000, SystemHint::MasterSystem)
            .expect("emulator should initialize");

        emu.set_input(HOST_BUTTON_1, HOST_DPAD_RIGHT | HOST_DPAD_UP);

        let raw = emu
            .bus()
            .input()
            .read_controller(crate::hardware::input::ControllerPort::One);
        assert_eq!(raw & SMS_PAD_BUTTON_1, 0);
        assert_eq!(raw & SMS_PAD_RIGHT, 0);
        assert_eq!(raw & SMS_PAD_UP, 0);
        assert_ne!(raw & SMS_PAD_BUTTON_2, 0);
        assert_ne!(raw & SMS_PAD_LEFT, 0);
        assert_ne!(raw & SMS_PAD_DOWN, 0);
    }

    #[test]
    fn suspended_emulator_does_not_advance() {
        let mut emu = Emulator::new_with_hint(&[0x00], 48_000, SystemHint::MasterSystem)
            .expect("emulator should initialize");

        emu.suspend();
        emu.step_frame();

        assert_eq!(emu.frame_count(), 0);
        assert!(emu.is_suspended());
    }

    #[test]
    fn standard_mapper_ram_is_not_blindly_exposed_as_battery_sram() {
        let mut emu = Emulator::new_with_hint(&[0x00], 48_000, SystemHint::MasterSystem)
            .expect("emulator should initialize");

        assert_eq!(
            emu.save_ram_kind(),
            SaveRamKind::mapper_ram_unknown(crate::hardware::constants::SMS_CARTRIDGE_RAM_SIZE)
        );
        assert!(!emu.has_battery());
        assert_eq!(emu.dump_battery_sram(), None);
        assert!(emu.load_battery_sram(&[0; 1]).is_err());

        emu.bus_mut().cpu_write(
            crate::hardware::constants::MAPPER_FRAME_CONTROL,
            crate::hardware::constants::MAPPER_FRAME_CONTROL_CART_RAM_ENABLE,
        );
        emu.bus_mut().cpu_write(0x8000, 0x5A);

        assert_eq!(emu.bus().cartridge_ram()[0], 0x5A);
        assert_eq!(emu.dump_battery_sram(), None);
    }

    #[test]
    fn breakpoint_suspends_before_instruction_executes() {
        let mut emu = Emulator::new_with_hint(&[0x00, 0x76], 48_000, SystemHint::MasterSystem)
            .expect("emulator should initialize");

        emu.add_breakpoint(0);
        let fetched = emu.step_instruction();

        assert_eq!(fetched, None);
        assert!(emu.is_suspended());
        assert_eq!(emu.cpu().cycles(), 0);
        assert_eq!(emu.cpu().regs().pc, 0);
        assert_eq!(emu.debug_hit_breakpoint(), Some(0));
    }

    #[test]
    fn debug_step_executes_one_instruction_while_suspended_on_breakpoint() {
        let mut emu = Emulator::new_with_hint(&[0x00, 0x76], 48_000, SystemHint::MasterSystem)
            .expect("emulator should initialize");

        emu.add_breakpoint(0);
        assert_eq!(emu.step_instruction(), None);
        emu.debug_step();

        assert!(emu.is_suspended());
        assert_eq!(emu.debug_hit_breakpoint(), None);
        assert_eq!(emu.cpu().regs().pc, 1);
        assert_eq!(emu.cpu().cycles(), 4);
    }

    #[test]
    fn write_watchpoint_suspends_after_cpu_write() {
        let mut emu = Emulator::new_with_hint(
            &[0x3E, 0x5A, 0x32, 0x00, 0xC0, 0x76],
            48_000,
            SystemHint::Sg1000,
        )
        .expect("emulator should initialize");

        emu.add_watchpoint(0xC000, WatchType::Write);
        emu.step_instruction();
        assert!(!emu.is_suspended());
        assert!(emu.debug_hit_watchpoint().is_none());

        emu.step_instruction();
        let hit = emu
            .debug_hit_watchpoint()
            .expect("write watchpoint should hit");

        assert!(emu.is_suspended());
        assert_eq!(emu.bus().cpu_read(0xC000), 0x5A);
        assert_eq!(hit.address, 0xC000);
        assert_eq!(hit.old_value, 0);
        assert_eq!(hit.new_value, 0x5A);
        assert_eq!(hit.watch_type, WatchType::Write);
    }

    #[test]
    fn read_watchpoint_suspends_after_cpu_read() {
        let mut emu =
            Emulator::new_with_hint(&[0x3A, 0x00, 0xC0, 0x76], 48_000, SystemHint::MasterSystem)
                .expect("emulator should initialize");
        emu.bus_mut().cpu_write(0xC000, 0xA5);

        emu.add_watchpoint(0xC000, WatchType::Read);
        emu.step_instruction();
        let hit = emu
            .debug_hit_watchpoint()
            .expect("read watchpoint should hit");

        assert!(emu.is_suspended());
        assert_eq!(emu.cpu().regs().a, 0xA5);
        assert_eq!(hit.address, 0xC000);
        assert_eq!(hit.old_value, 0xA5);
        assert_eq!(hit.new_value, 0xA5);
        assert_eq!(hit.watch_type, WatchType::Read);
    }

    #[test]
    fn debug_write_triggers_write_watchpoint() {
        let mut emu = Emulator::new_with_hint(&[0x76], 48_000, SystemHint::MasterSystem)
            .expect("emulator should initialize");

        emu.add_watchpoint(0xC000, WatchType::Write);
        emu.debug_write(0xC000, 0x5A);

        let hit = emu
            .debug_hit_watchpoint()
            .expect("debug write should hit watchpoint");
        assert!(emu.is_suspended());
        assert_eq!(emu.bus().cpu_read(0xC000), 0x5A);
        assert_eq!(hit.address, 0xC000);
        assert_eq!(hit.old_value, 0);
        assert_eq!(hit.new_value, 0x5A);
    }

    #[test]
    fn bus_trace_records_cpu_reads_and_writes_for_instruction() {
        let mut emu = Emulator::new_with_hint(
            &[0x3E, 0x5A, 0x32, 0x00, 0xC0, 0x76],
            48_000,
            SystemHint::MasterSystem,
        )
        .expect("emulator should initialize");

        let (_, load_trace) = emu.step_instruction_with_bus_trace();
        assert!(matches!(
            load_trace.as_slice(),
            [
                CpuAccessTraceEvent::Read {
                    addr: 0x0000,
                    value: 0x3E
                },
                CpuAccessTraceEvent::Read {
                    addr: 0x0001,
                    value: 0x5A
                },
            ]
        ));

        let (_, store_trace) = emu.step_instruction_with_bus_trace();
        assert!(store_trace.iter().any(|event| matches!(
            event,
            CpuAccessTraceEvent::Write {
                addr: 0xC000,
                old_value: 0x00,
                new_value: 0x5A
            }
        )));
    }

    #[test]
    fn bus_trace_records_io_writes_for_out_instruction() {
        let mut emu = Emulator::new_with_hint(
            &[0x3E, 0x90, 0xD3, 0x7F, 0x76],
            48_000,
            SystemHint::MasterSystem,
        )
        .expect("emulator should initialize");

        emu.step_instruction();
        let (fetched, trace) = emu.step_instruction_with_bus_trace();

        assert_eq!(
            fetched.expect("OUT instruction should execute").opcode,
            0xD3
        );
        assert!(trace.iter().any(|event| matches!(
            event,
            CpuAccessTraceEvent::IoWrite {
                port: 0x7F,
                value: 0x90
            }
        )));
    }

    #[test]
    fn step_instruction_runs_minimal_z80_program() {
        let mut emu = Emulator::new_with_hint(
            &[0x3E, 0x5A, 0x32, 0x00, 0xC0, 0x76],
            48_000,
            SystemHint::Sg1000,
        )
        .expect("emulator should initialize");

        while !emu.cpu().is_halted() {
            emu.step_instruction();
        }

        assert_eq!(emu.bus().cpu_read(0xC000), 0x5A);
        assert_eq!(emu.cpu().cycles(), 24);
    }

    #[test]
    fn step_instruction_clocks_vdp_timing() {
        let mut emu = Emulator::new_with_hint(&[0x00], 48_000, SystemHint::MasterSystem)
            .expect("emulator should initialize");

        for _ in 0..57 {
            emu.step_instruction();
        }

        assert_eq!(emu.bus().vdp().scanline(), 1);
    }

    #[test]
    fn step_frame_generates_psg_audio_samples() {
        let mut emu = Emulator::new_with_hint(&[0x76], 48_000, SystemHint::MasterSystem)
            .expect("emulator should initialize");

        emu.bus_mut()
            .io_write(crate::hardware::constants::IO_PORT_PSG, 0x80);
        emu.bus_mut()
            .io_write(crate::hardware::constants::IO_PORT_PSG, 0x04);
        emu.bus_mut()
            .io_write(crate::hardware::constants::IO_PORT_PSG, 0x90);

        emu.step_frame();
        let samples = emu.drain_audio_samples();

        assert!(
            (1598..=1602).contains(&samples.len()),
            "expected about 800 stereo pairs per frame, got {} samples",
            samples.len()
        );
        assert!(samples.iter().any(|&sample| sample != 0.0));
        assert!(emu.drain_audio_samples().is_empty());
    }
}
