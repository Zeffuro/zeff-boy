use std::path::{Path, PathBuf};

use zeff_emu_common::address::{Address, narrow_u16};
use zeff_emu_common::memory::{MemoryRegionDescriptor, MemoryRegionKind, resolve_memory_region};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_gb_core::emulator::Emulator as GbEmulator;

use crate::audio_tooling::{
    AudioChannelId, AudioSemanticFrame, AudioVoiceClass, AudioVoiceState, GB_TEMPO_US_PER_BEAT,
    level_from_u4,
};
use crate::emu_backend::paths::BackendPaths;
use crate::emu_core_trait::{EmulatorCore, copy_optional_region_to_vec, copy_slice_to_vec};

impl crate::emu_core_trait::DebuggableEmulator for GbEmulator {
    fn add_breakpoint(&mut self, addr: Address) {
        self.add_breakpoint(narrow_u16(addr))
    }
    fn add_watchpoint(&mut self, addr: Address, wt: zeff_emu_common::debug::WatchType) {
        self.add_watchpoint(narrow_u16(addr), wt)
    }
    fn remove_breakpoint(&mut self, addr: Address) {
        self.remove_breakpoint(narrow_u16(addr))
    }
    fn toggle_breakpoint(&mut self, addr: Address) {
        self.toggle_breakpoint(narrow_u16(addr))
    }
    fn cpu_peek8(&self, addr: Address) -> u8 {
        self.cpu_peek8(narrow_u16(addr))
    }
    fn cpu_write8(&mut self, addr: Address, val: u8) {
        self.cpu_write8(narrow_u16(addr), val)
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

pub(crate) struct GbBackend {
    pub(crate) emu: GbEmulator,
    paths: BackendPaths,
}

impl GbBackend {
    pub(crate) fn new(emu: GbEmulator, rom_path: PathBuf) -> Self {
        Self {
            emu,
            paths: BackendPaths::new(rom_path),
        }
    }

    pub(crate) fn with_source_path(
        emu: GbEmulator,
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

impl EmulatorCore for GbBackend {
    #[inline]
    fn step_frame(&mut self) {
        self.emu.step_frame();
    }

    #[inline]
    fn frame_count(&self) -> u64 {
        self.emu.frame_count()
    }

    #[inline]
    fn framebuffer(&self) -> &[u8] {
        self.emu.framebuffer()
    }

    #[inline]
    fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.emu.drain_audio_samples_into(buf);
    }

    #[inline]
    fn set_sample_rate(&mut self, rate: u32) {
        self.emu.set_sample_rate(rate);
    }

    fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.emu.set_apu_sample_generation_enabled(enabled);
    }

    fn set_apu_channel_mutes(&mut self, mutes: &[bool]) {
        let arr: [bool; 4] = std::array::from_fn(|i| mutes.get(i).copied().unwrap_or(false));
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
        self.emu.encode_state_bytes()
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

    fn supports_debugger(&self) -> bool {
        true
    }

    fn supports_opcode_history(&self) -> bool {
        true
    }

    fn copy_memory_region(
        &mut self,
        id_or_alias: &str,
        out: &mut Vec<u8>,
    ) -> anyhow::Result<MemoryRegionDescriptor> {
        let regions = self.memory_regions();
        let region = resolve_memory_region(&regions, id_or_alias)
            .ok_or_else(|| anyhow::anyhow!("unknown memory region '{id_or_alias}' for Game Boy"))?;

        match region.kind {
            MemoryRegionKind::SystemRam => copy_slice_to_vec(out, self.emu.system_ram()),
            MemoryRegionKind::VideoRam => copy_slice_to_vec(out, self.emu.video_ram_snapshot()),
            MemoryRegionKind::SaveRam => {
                copy_optional_region_to_vec(out, self.emu.dump_battery_sram(), region.id)?
            }
            MemoryRegionKind::Framebuffer => copy_slice_to_vec(out, self.emu.framebuffer()),
            MemoryRegionKind::CpuAddressSpace => {
                return Err(anyhow::anyhow!(
                    "Game Boy CPU address space is not copyable as a finite memory region"
                ));
            }
            MemoryRegionKind::PaletteRam
            | MemoryRegionKind::Oam
            | MemoryRegionKind::IoRegisters
            | MemoryRegionKind::ExternalWorkRam
            | MemoryRegionKind::InternalWorkRam => {
                return Err(anyhow::anyhow!(
                    "Game Boy memory region '{}' is not exposed as a copyable region",
                    region.id
                ));
            }
        }

        Ok(region)
    }

    #[inline]
    fn audio_semantic_frame(&self) -> Option<AudioSemanticFrame> {
        Some(gb_audio_semantic_frame(
            self.emu.frame_count(),
            self.emu.apu_channel_snapshot(),
        ))
    }

    #[inline]
    fn rumble_active(&self) -> bool {
        self.emu.rumble_active()
    }

    #[inline]
    fn is_mbc7(&self) -> bool {
        self.emu.is_mbc7_cartridge()
    }

    #[inline]
    fn is_pocket_camera(&self) -> bool {
        self.emu.is_pocket_camera_cartridge()
    }
}

fn gb_audio_semantic_frame(
    frame: u64,
    snap: zeff_gb_core::hardware::apu::ApuChannelSnapshot,
) -> AudioSemanticFrame {
    AudioSemanticFrame {
        frame,
        tempo_us_per_beat: GB_TEMPO_US_PER_BEAT,
        voices: vec![
            AudioVoiceState {
                channel: AudioChannelId(0),
                name: "GB CH1 (Square 1)",
                class: AudioVoiceClass::Pulse,
                active: snap.ch1_enabled,
                pitch_hz: Some(gb_square_freq_to_hz(snap.ch1_frequency)),
                level: Some(level_from_u4(snap.ch1_volume)),
            },
            AudioVoiceState {
                channel: AudioChannelId(1),
                name: "GB CH2 (Square 2)",
                class: AudioVoiceClass::Pulse,
                active: snap.ch2_enabled,
                pitch_hz: Some(gb_square_freq_to_hz(snap.ch2_frequency)),
                level: Some(level_from_u4(snap.ch2_volume)),
            },
            AudioVoiceState {
                channel: AudioChannelId(2),
                name: "GB CH3 (Wave)",
                class: AudioVoiceClass::Wavetable,
                active: snap.ch3_enabled,
                pitch_hz: Some(gb_wave_freq_to_hz(snap.ch3_frequency)),
                level: Some(gb_wave_level(snap.ch3_output_level)),
            },
            AudioVoiceState {
                channel: AudioChannelId(3),
                name: "GB CH4 (Noise)",
                class: AudioVoiceClass::Noise,
                active: snap.ch4_enabled,
                pitch_hz: None,
                level: Some(level_from_u4(snap.ch4_volume)),
            },
        ],
    }
}

fn gb_square_freq_to_hz(freq_reg: u16) -> f64 {
    let denom = 2048u32.saturating_sub(freq_reg as u32).max(1);
    131_072.0 / f64::from(denom)
}

fn gb_wave_freq_to_hz(freq_reg: u16) -> f64 {
    let denom = 2048u32.saturating_sub(freq_reg as u32).max(1);
    65_536.0 / f64::from(denom)
}

fn gb_wave_level(level: u8) -> f32 {
    match level {
        0 => 0.0,
        1 => 1.0,
        2 => 80.0 / 127.0,
        3 => 48.0 / 127.0,
        _ => 0.0,
    }
}

pub(crate) fn try_load_battery_sram(
    emu: &mut GbEmulator,
    rom_path: &Path,
) -> anyhow::Result<Option<String>> {
    crate::save_paths::try_load_battery_sram(rom_path, "GB", emu.has_battery(), |bytes| {
        emu.load_battery_sram(bytes)
    })
}
