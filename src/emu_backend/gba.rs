use std::path::{Path, PathBuf};

use zeff_emu_common::address::Address;
use zeff_emu_common::memory::{MemoryRegionDescriptor, MemoryRegionKind, resolve_memory_region};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_gba_core::emulator::Emulator as GbaEmulator;

use crate::emu_backend::paths::BackendPaths;
use crate::emu_core_trait::{EmulatorCore, copy_optional_region_to_vec, copy_slice_to_vec};

impl crate::emu_core_trait::DebuggableEmulator for GbaEmulator {
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
        self.cpu_peek8(addr)
    }
    fn cpu_write8(&mut self, addr: Address, val: u8) {
        self.cpu_write8(addr, val);
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

pub(crate) struct GbaBackend {
    pub(crate) emu: GbaEmulator,
    paths: BackendPaths,
}

impl GbaBackend {
    pub(crate) fn new(emu: GbaEmulator, rom_path: PathBuf) -> Self {
        Self {
            emu,
            paths: BackendPaths::new(rom_path),
        }
    }

    pub(crate) fn with_source_path(
        emu: GbaEmulator,
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
}

impl EmulatorCore for GbaBackend {
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
        let arr: [bool; 6] = std::array::from_fn(|i| mutes.get(i).copied().unwrap_or(false));
        self.emu.set_apu_channel_mutes(arr);
    }

    #[inline]
    fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.emu.set_input(buttons_pressed, dpad_pressed);
    }

    #[inline]
    fn is_suspended(&self) -> bool {
        self.emu.is_cpu_suspended()
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
        let (ewram, iwram) = self.emu.system_ram();
        ewram.len() + iwram.len()
    }

    fn external_work_ram_len(&self) -> usize {
        self.emu.system_ram().0.len()
    }

    fn internal_work_ram_len(&self) -> usize {
        self.emu.system_ram().1.len()
    }

    fn video_ram_len(&self) -> usize {
        self.emu.video_ram_snapshot().len()
    }

    fn palette_ram_len(&self) -> usize {
        self.emu.palette_ram_snapshot().len()
    }

    fn oam_len(&self) -> usize {
        self.emu.oam_snapshot().len()
    }

    fn io_registers_len(&self) -> usize {
        self.emu.io_snapshot().len()
    }

    fn supports_debugger(&self) -> bool {
        true
    }

    fn supports_opcode_history(&self) -> bool {
        true
    }

    fn cpu_address_bits(&self) -> u8 {
        32
    }

    fn copy_memory_region(
        &mut self,
        id_or_alias: &str,
        out: &mut Vec<u8>,
    ) -> anyhow::Result<MemoryRegionDescriptor> {
        let regions = self.memory_regions();
        let region = resolve_memory_region(&regions, id_or_alias).ok_or_else(|| {
            anyhow::anyhow!("unknown memory region '{id_or_alias}' for Game Boy Advance")
        })?;

        match region.kind {
            MemoryRegionKind::SystemRam => {
                let (ewram, iwram) = self.emu.system_ram();
                out.clear();
                out.extend_from_slice(ewram);
                out.extend_from_slice(iwram);
            }
            MemoryRegionKind::ExternalWorkRam => copy_slice_to_vec(out, self.emu.system_ram().0),
            MemoryRegionKind::InternalWorkRam => copy_slice_to_vec(out, self.emu.system_ram().1),
            MemoryRegionKind::VideoRam => copy_slice_to_vec(out, self.emu.video_ram_snapshot()),
            MemoryRegionKind::PaletteRam => copy_slice_to_vec(out, self.emu.palette_ram_snapshot()),
            MemoryRegionKind::Oam => copy_slice_to_vec(out, self.emu.oam_snapshot()),
            MemoryRegionKind::IoRegisters => copy_slice_to_vec(out, self.emu.io_snapshot()),
            MemoryRegionKind::SaveRam => {
                copy_optional_region_to_vec(out, self.emu.dump_battery_sram(), region.id)?
            }
            MemoryRegionKind::Framebuffer => copy_slice_to_vec(out, self.emu.framebuffer()),
            MemoryRegionKind::CpuAddressSpace => {
                return Err(anyhow::anyhow!(
                    "GBA CPU address space is not copyable as a finite memory region"
                ));
            }
        }

        Ok(region)
    }
}

pub(crate) fn try_load_battery_sram(
    emu: &mut GbaEmulator,
    rom_path: &Path,
) -> anyhow::Result<Option<String>> {
    crate::save_paths::try_load_battery_sram(rom_path, "GBA", emu.has_battery(), |bytes| {
        emu.load_battery_sram(bytes)
    })
}
