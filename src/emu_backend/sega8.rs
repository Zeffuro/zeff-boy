use std::path::{Path, PathBuf};

use zeff_emu_common::address::{Address, narrow_u16};
use zeff_emu_common::memory::{MemoryRegionDescriptor, MemoryRegionKind, resolve_memory_region};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_sega8_core::emulator::Emulator as Sega8Emulator;
use zeff_sega8_core::hardware::cartridge::{Sega8MapperKind, Sega8System, SystemHint};
use zeff_sega8_core::hardware::region::Sega8Region;
use zeff_sega8_core::hardware::timing::Sega8VideoStandard;

use crate::audio_recorder::MidiApuSnapshot;
use crate::emu_backend::ActiveSystem;
use crate::emu_backend::paths::BackendPaths;
use crate::emu_core_trait::{EmulatorCore, copy_slice_to_vec};

impl crate::emu_core_trait::DebuggableEmulator for Sega8Emulator {
    fn add_breakpoint(&mut self, addr: Address) {
        self.add_breakpoint(addr);
    }

    fn add_watchpoint(&mut self, addr: Address, wt: zeff_emu_common::debug::WatchType) {
        self.add_watchpoint(addr, wt);
    }

    fn remove_breakpoint(&mut self, addr: Address) {
        self.remove_breakpoint(addr);
    }

    fn toggle_breakpoint(&mut self, addr: Address) {
        self.toggle_breakpoint(addr);
    }

    fn cpu_peek8(&self, addr: Address) -> u8 {
        self.cpu_peek8(narrow_u16(addr))
    }

    fn cpu_write8(&mut self, addr: Address, val: u8) {
        self.cpu_write8(narrow_u16(addr), val);
    }
    fn is_cpu_suspended(&self) -> bool {
        self.is_cpu_suspended()
    }
    fn debug_continue(&mut self) {
        self.debug_continue()
    }
    fn debug_step(&mut self) {
        self.debug_step()
    }
    fn supports_opcode_history(&self) -> bool {
        true
    }
    fn set_opcode_log_enabled(&mut self, enabled: bool) {
        self.set_opcode_log_enabled(enabled)
    }
}

pub(crate) struct Sega8Backend {
    pub(crate) emu: Sega8Emulator,
    paths: BackendPaths,
}

impl Sega8Backend {
    pub(crate) fn new(emu: Sega8Emulator, rom_path: PathBuf) -> Self {
        Self {
            emu,
            paths: BackendPaths::new(rom_path),
        }
    }

    pub(crate) fn with_source_path(
        emu: Sega8Emulator,
        rom_path: PathBuf,
        source_path: PathBuf,
    ) -> Self {
        Self {
            emu,
            paths: BackendPaths::with_source_path(rom_path, source_path),
        }
    }

    pub(crate) fn source_path(&self) -> &Path {
        self.paths.source_path()
    }

    pub(crate) fn system(&self) -> ActiveSystem {
        active_system_for_sega8(self.emu.system())
    }
}

impl EmulatorCore for Sega8Backend {
    #[inline]
    fn step_frame(&mut self) {
        self.emu.step_frame();
    }

    #[inline]
    fn framebuffer(&self) -> &[u8] {
        self.emu.framebuffer()
    }

    #[inline]
    fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.emu.drain_audio_samples_into(buf);
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.emu.set_sample_rate(rate);
    }

    fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.emu.set_apu_sample_generation_enabled(enabled);
    }

    fn set_apu_channel_mutes(&mut self, mutes: &[bool]) {
        let arr: [bool; zeff_sega8_core::hardware::apu::PSG_CHANNEL_COUNT] =
            std::array::from_fn(|i| mutes.get(i).copied().unwrap_or(false));
        self.emu.set_apu_channel_mutes(arr);
    }

    #[inline]
    fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.emu.set_input(buttons_pressed, dpad_pressed);
    }

    #[inline]
    fn set_input_p2(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.emu.set_input_p2(buttons_pressed, dpad_pressed);
    }

    #[inline]
    fn is_suspended(&self) -> bool {
        self.emu.is_suspended()
    }

    fn flush_battery_sram(&mut self) -> anyhow::Result<Option<String>> {
        crate::save_paths::flush_battery_sram(self.paths.rom_path(), self.emu.dump_battery_sram())
    }

    fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        self.emu.encode_state()
    }

    fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.emu.load_state_from_bytes(bytes)
    }

    fn rom_path(&self) -> &Path {
        self.paths.rom_path()
    }

    fn rom_hash(&self) -> [u8; 32] {
        self.emu.rom_hash()
    }

    fn save_ram_kind(&self) -> SaveRamKind {
        self.emu.save_ram_kind()
    }

    fn system_ram_len(&self) -> usize {
        self.emu.system_ram().len()
    }

    fn video_ram_len(&self) -> usize {
        self.emu.video_ram_snapshot().len()
    }

    fn palette_ram_len(&self) -> usize {
        match self.emu.system() {
            Sega8System::MasterSystem | Sega8System::GameGear => {
                self.emu.palette_ram_snapshot().len()
            }
            Sega8System::Sg1000 => 0,
        }
    }

    fn supports_debugger(&self) -> bool {
        true
    }

    fn supports_opcode_history(&self) -> bool {
        true
    }

    fn apu_channel_snapshot(&self) -> Option<MidiApuSnapshot> {
        Some(MidiApuSnapshot::Sega8(
            self.emu.bus().apu().debug_snapshot(),
        ))
    }

    fn copy_memory_region(
        &mut self,
        id_or_alias: &str,
        out: &mut Vec<u8>,
    ) -> anyhow::Result<MemoryRegionDescriptor> {
        let regions = self.memory_regions();
        let region = resolve_memory_region(&regions, id_or_alias).ok_or_else(|| {
            anyhow::anyhow!("unknown memory region '{id_or_alias}' for Sega 8-bit")
        })?;

        match region.kind {
            MemoryRegionKind::SystemRam => copy_slice_to_vec(out, self.emu.system_ram()),
            MemoryRegionKind::VideoRam => copy_slice_to_vec(out, self.emu.video_ram_snapshot()),
            MemoryRegionKind::PaletteRam => copy_slice_to_vec(out, self.emu.palette_ram_snapshot()),
            MemoryRegionKind::SaveRam => {
                copy_slice_to_vec(out, self.emu.bus().cartridge_ram_visible())
            }
            MemoryRegionKind::Framebuffer => copy_slice_to_vec(out, self.emu.framebuffer()),
            MemoryRegionKind::CpuAddressSpace => {
                return Err(anyhow::anyhow!(
                    "Sega 8-bit CPU address space is not copyable as a finite memory region"
                ));
            }
            MemoryRegionKind::Oam
            | MemoryRegionKind::IoRegisters
            | MemoryRegionKind::ExternalWorkRam
            | MemoryRegionKind::InternalWorkRam => {
                return Err(anyhow::anyhow!(
                    "Sega 8-bit memory region '{}' is not exposed as a copyable region",
                    region.id
                ));
            }
        }

        Ok(region)
    }
}

pub(crate) fn hint_for_active_system(system: ActiveSystem) -> Option<SystemHint> {
    match system {
        ActiveSystem::MasterSystem => Some(SystemHint::MasterSystem),
        ActiveSystem::GameGear => Some(SystemHint::GameGear),
        ActiveSystem::Sg1000 => Some(SystemHint::Sg1000),
        _ => None,
    }
}

pub(crate) fn video_standard_from_paths(
    source_path: &Path,
    rom_path: &Path,
) -> Option<Sega8VideoStandard> {
    Sega8VideoStandard::from_path(rom_path).or_else(|| Sega8VideoStandard::from_path(source_path))
}

pub(crate) fn console_region_from_paths(
    source_path: &Path,
    rom_path: &Path,
) -> Option<Sega8Region> {
    Sega8Region::from_path(rom_path).or_else(|| Sega8Region::from_path(source_path))
}

pub(crate) fn mapper_kind_from_paths(
    source_path: &Path,
    rom_path: &Path,
) -> Option<Sega8MapperKind> {
    Sega8MapperKind::from_path(rom_path).or_else(|| Sega8MapperKind::from_path(source_path))
}

fn active_system_for_sega8(system: Sega8System) -> ActiveSystem {
    match system {
        Sega8System::MasterSystem => ActiveSystem::MasterSystem,
        Sega8System::GameGear => ActiveSystem::GameGear,
        Sega8System::Sg1000 => ActiveSystem::Sg1000,
    }
}

pub(crate) fn try_load_battery_sram(
    emu: &mut Sega8Emulator,
    rom_path: &Path,
) -> anyhow::Result<Option<String>> {
    crate::save_paths::try_load_battery_sram(rom_path, "Sega 8-bit", emu.has_battery(), |bytes| {
        emu.load_battery_sram(bytes)
    })
}
