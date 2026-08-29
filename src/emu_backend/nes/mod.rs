use std::path::{Path, PathBuf};

use zeff_emu_common::address::{Address, narrow_u16};
use zeff_emu_common::memory::{MemoryRegionDescriptor, MemoryRegionKind, resolve_memory_region};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::time::{FrameLifecycle, MachineTiming, Reset, TimingSnapshot};
use zeff_nes_core::emulator::Emulator as NesEmulator;

use crate::audio_tooling::{
    AudioChannelDescriptor, AudioChannelId, AudioSemanticCaps, AudioSemanticFrame, AudioTopology,
    AudioVoiceClass, AudioVoiceState, NTSC_60_TEMPO_US_PER_BEAT, level_from_u4,
};
use crate::cheats::CheatPatch;
use crate::emu_backend::paths::BackendPaths;
use crate::emu_core_trait::{EmulatorCore, copy_optional_region_to_vec, copy_slice_to_vec};

const NES_AUDIO_CHANNELS: &[AudioChannelDescriptor] = &[
    AudioChannelDescriptor {
        id: AudioChannelId(0),
        name: "NES Pulse 1",
        group: "2A03 APU",
        class: AudioVoiceClass::Pulse,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(1),
        name: "NES Pulse 2",
        group: "2A03 APU",
        class: AudioVoiceClass::Pulse,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(2),
        name: "NES Triangle",
        group: "2A03 APU",
        class: AudioVoiceClass::Triangle,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(3),
        name: "NES Noise",
        group: "2A03 APU",
        class: AudioVoiceClass::Noise,
        caps: AudioSemanticCaps::GATE_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(4),
        name: "NES DMC",
        group: "2A03 APU",
        class: AudioVoiceClass::Pcm,
        caps: AudioSemanticCaps::GATE_LEVEL,
        muteable: true,
    },
];

impl crate::emu_core_trait::DebuggableEmulator for NesEmulator {
    fn add_breakpoint(&mut self, addr: Address) {
        self.add_breakpoint(narrow_u16(addr))
    }
    fn add_one_shot_breakpoint(&mut self, addr: Address) {
        self.add_one_shot_breakpoint(narrow_u16(addr))
    }
    fn add_breakpoint_after(&mut self, addr: Address, target_hits: u64) {
        self.add_breakpoint_after(narrow_u16(addr), target_hits)
    }
    fn set_event_breakpoint(&mut self, event: zeff_emu_common::debug::DebugEvent, enabled: bool) {
        self.set_event_breakpoint(event, enabled)
    }
    fn add_watchpoint_range(
        &mut self,
        start: Address,
        end: Address,
        wt: zeff_emu_common::debug::WatchType,
    ) {
        self.add_watchpoint_range(narrow_u16(start), narrow_u16(end), wt)
    }
    fn remove_watchpoint(
        &mut self,
        start: Address,
        end: Address,
        wt: zeff_emu_common::debug::WatchType,
    ) {
        self.remove_watchpoint(narrow_u16(start), narrow_u16(end), wt)
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

    fn set_instruction_trace_enabled(&mut self, enabled: bool) {
        self.set_instruction_trace_enabled(enabled);
    }

    fn set_instruction_trace_capacity(&mut self, capacity: usize) {
        self.set_instruction_trace_capacity(capacity);
    }

    fn clear_instruction_trace(&mut self) {
        self.clear_instruction_trace();
    }
}

pub(crate) struct NesBackend {
    pub(crate) emu: NesEmulator,
    paths: BackendPaths,
    sram_recovery: crate::save_paths::SramRecoverySession,
}

impl NesBackend {
    pub(crate) fn battery_components(&self) -> Vec<(&'static str, Vec<u8>)> {
        self.emu
            .dump_persistent_data()
            .map(|bytes| vec![(crate::save_paths::SRAM_COMPONENT, bytes)])
            .unwrap_or_default()
    }

    pub(crate) fn new(emu: NesEmulator, rom_path: PathBuf) -> Self {
        let sram_recovery =
            crate::save_paths::battery_sram_session(&rom_path, "nes", emu.rom_hash());
        Self {
            emu,
            paths: BackendPaths::new(rom_path),
            sram_recovery,
        }
    }

    pub(crate) fn with_source_path(
        emu: NesEmulator,
        rom_path: PathBuf,
        source_path: PathBuf,
    ) -> Self {
        let sram_recovery =
            crate::save_paths::battery_sram_session(&rom_path, "nes", emu.rom_hash());
        Self {
            emu,
            paths: BackendPaths::with_source_path(rom_path, source_path),
            sram_recovery,
        }
    }

    pub(crate) fn source_path(&self) -> &Path {
        self.paths.source_path()
    }

    pub(crate) fn nominal_frame_duration_ns(&self) -> u64 {
        self.emu.nominal_frame_duration_ns()
    }

    pub(crate) fn firmware_manifests(&self) -> &[zeff_emu_common::replay::ReplayFirmwareManifest] {
        self.paths.firmware_manifests()
    }

    pub(crate) fn set_firmware_manifests(
        &mut self,
        firmware_manifests: Vec<zeff_emu_common::replay::ReplayFirmwareManifest>,
    ) {
        self.paths.set_firmware_manifests(firmware_manifests);
    }

    pub(crate) fn set_fds_disk_side(&mut self, side: u8) -> anyhow::Result<()> {
        self.emu.set_fds_disk_side(side)
    }

    #[cfg(test)]
    pub(crate) fn fds_disk_side(&self) -> Option<u8> {
        self.emu.fds_disk_side()
    }

    pub(crate) fn media_slot_snapshot(&self) -> Option<zeff_emu_common::media::MediaSlotSnapshot> {
        self.emu.media_slot_snapshot()
    }

    pub(crate) fn apply_media_event(
        &mut self,
        event: &zeff_emu_common::media::MediaEvent,
    ) -> anyhow::Result<()> {
        self.emu.apply_media_event(event)
    }
}

impl EmulatorCore for NesBackend {
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
        let arr: [bool; 5] = std::array::from_fn(|i| mutes.get(i).copied().unwrap_or(false));
        self.emu.set_apu_channel_mutes(arr);
    }

    #[inline]
    fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.emu.set_input(buttons_pressed, dpad_pressed);
    }

    #[inline]
    fn set_zapper_state(
        &mut self,
        enabled: bool,
        trigger: bool,
        hit: bool,
        screen_pos: Option<(u16, u16)>,
    ) {
        self.emu.set_zapper_state(enabled, trigger, hit, screen_pos);
    }

    #[inline]
    fn is_suspended(&self) -> bool {
        self.emu.is_cpu_suspended()
    }

    fn flush_battery_sram(&mut self) -> anyhow::Result<Option<String>> {
        crate::save_paths::flush_battery_sram(
            &mut self.sram_recovery,
            self.paths.rom_path(),
            "nes",
            self.emu.rom_hash(),
            self.emu.dump_persistent_data(),
        )
    }

    fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        self.emu.encode_state()
    }

    fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.emu.load_state_from_bytes(bytes)
    }

    fn state_restores_framebuffer(&self) -> bool {
        true
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
        0x2000
    }

    fn palette_ram_len(&self) -> usize {
        self.emu.ppu_palette_ram().len()
    }

    fn oam_len(&self) -> usize {
        self.emu.ppu_oam().len()
    }

    fn supports_save_states(&self) -> bool {
        true
    }

    fn supports_audio(&self) -> bool {
        true
    }

    fn supports_cheats(&self) -> bool {
        true
    }

    fn install_rom_patches(&mut self, cheats: &[CheatPatch]) {
        self.emu.clear_game_genie();
        for patch in cheats.iter().copied() {
            if let Some((address, value, compare)) = patch.constant_rom_write() {
                self.emu
                    .add_game_genie_patch(zeff_nes_core::cheats::NesGameGeniePatch {
                        address,
                        value,
                        compare,
                    });
            }
        }
    }

    fn apply_ram_cheats(&mut self, cheats: &[CheatPatch]) {
        zeff_emu_common::cheats::apply_ram_cheats_16(&mut self.emu, cheats);
    }

    fn debug_suspend(&mut self) {
        self.emu.debug_suspend();
    }

    fn supports_debugger(&self) -> bool {
        true
    }

    fn supports_opcode_history(&self) -> bool {
        true
    }

    #[inline]
    fn set_input_p2(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.emu.set_input_p2(buttons_pressed, dpad_pressed);
    }

    fn audio_semantic_frame(&self) -> Option<AudioSemanticFrame> {
        Some(nes_audio_semantic_frame(
            self.emu.frame_count(),
            self.emu.apu_channel_snapshot(),
        ))
    }

    fn audio_topology(&self) -> Option<AudioTopology> {
        Some(AudioTopology {
            generation: 1,
            channels: NES_AUDIO_CHANNELS,
        })
    }

    fn copy_memory_region(
        &mut self,
        id_or_alias: &str,
        out: &mut Vec<u8>,
    ) -> anyhow::Result<MemoryRegionDescriptor> {
        let regions = self.memory_regions();
        let region = resolve_memory_region(&regions, id_or_alias)
            .ok_or_else(|| anyhow::anyhow!("unknown memory region '{id_or_alias}' for NES"))?;

        match region.kind {
            MemoryRegionKind::SystemRam => copy_slice_to_vec(out, self.emu.system_ram()),
            MemoryRegionKind::VideoRam => {
                let video_ram = self.emu.video_ram_snapshot();
                copy_slice_to_vec(out, &video_ram);
            }
            MemoryRegionKind::PaletteRam => copy_slice_to_vec(out, self.emu.ppu_palette_ram()),
            MemoryRegionKind::Oam => copy_slice_to_vec(out, self.emu.ppu_oam()),
            MemoryRegionKind::SaveRam => {
                copy_optional_region_to_vec(out, self.emu.dump_battery_sram(), region.id)?
            }
            MemoryRegionKind::Framebuffer => copy_slice_to_vec(out, self.emu.framebuffer()),
            MemoryRegionKind::CpuAddressSpace => {
                return Err(anyhow::anyhow!(
                    "NES CPU address space is not copyable as a finite memory region"
                ));
            }
            MemoryRegionKind::IoRegisters
            | MemoryRegionKind::ExternalWorkRam
            | MemoryRegionKind::InternalWorkRam => {
                return Err(anyhow::anyhow!(
                    "NES memory region '{}' is not exposed as a copyable region",
                    region.id
                ));
            }
        }

        Ok(region)
    }
}

impl MachineTiming for NesBackend {
    #[inline]
    fn timing_snapshot(&self) -> TimingSnapshot {
        self.emu.timing_snapshot()
    }
}

impl Reset for NesBackend {
    #[inline]
    fn reset(&mut self) {
        Reset::reset(&mut self.emu);
    }
}

impl FrameLifecycle for NesBackend {
    #[inline]
    fn step_frame(&mut self) {
        FrameLifecycle::step_frame(&mut self.emu);
    }

    #[inline]
    fn frame_count(&self) -> u64 {
        FrameLifecycle::frame_count(&self.emu)
    }
}

fn nes_audio_semantic_frame(
    frame: u64,
    snap: zeff_nes_core::hardware::apu::ApuChannelSnapshot,
) -> AudioSemanticFrame {
    AudioSemanticFrame {
        frame,
        tempo_us_per_beat: NTSC_60_TEMPO_US_PER_BEAT,
        voices: vec![
            AudioVoiceState {
                channel: AudioChannelId(0),
                name: "NES Pulse 1",
                class: AudioVoiceClass::Pulse,
                active: snap.pulse1_enabled,
                pitch_hz: Some(nes_pulse_freq_to_hz(snap.pulse1_timer_period)),
                level: Some(level_from_u4(snap.pulse1_volume)),
            },
            AudioVoiceState {
                channel: AudioChannelId(1),
                name: "NES Pulse 2",
                class: AudioVoiceClass::Pulse,
                active: snap.pulse2_enabled,
                pitch_hz: Some(nes_pulse_freq_to_hz(snap.pulse2_timer_period)),
                level: Some(level_from_u4(snap.pulse2_volume)),
            },
            AudioVoiceState {
                channel: AudioChannelId(2),
                name: "NES Triangle",
                class: AudioVoiceClass::Triangle,
                active: snap.triangle_enabled,
                pitch_hz: Some(nes_triangle_freq_to_hz(snap.triangle_timer_period)),
                level: Some(level_from_u4(snap.triangle_volume)),
            },
            AudioVoiceState {
                channel: AudioChannelId(3),
                name: "NES Noise",
                class: AudioVoiceClass::Noise,
                active: snap.noise_enabled,
                pitch_hz: None,
                level: Some(level_from_u4(snap.noise_volume)),
            },
            AudioVoiceState {
                channel: AudioChannelId(4),
                name: "NES DMC",
                class: AudioVoiceClass::Pcm,
                active: snap.dmc_enabled,
                pitch_hz: None,
                level: Some(f32::from(snap.dmc_output_level) / 127.0),
            },
        ],
    }
}

fn nes_pulse_freq_to_hz(timer_period: u16) -> f64 {
    zeff_nes_core::hardware::constants::APU_CPU_CLOCK_NTSC
        / (16.0 * (f64::from(timer_period) + 1.0))
}

fn nes_triangle_freq_to_hz(timer_period: u16) -> f64 {
    zeff_nes_core::hardware::constants::APU_CPU_CLOCK_NTSC
        / (32.0 * (f64::from(timer_period) + 1.0))
}

pub(crate) fn try_load_battery_sram(
    emu: &mut NesEmulator,
    rom_path: &Path,
) -> anyhow::Result<Option<String>> {
    #[cfg(not(target_arch = "wasm32"))]
    let result =
        crate::save_paths::try_load_battery_sram(rom_path, "NES", emu.has_battery(), |bytes| {
            emu.load_persistent_data(bytes)
        });
    #[cfg(target_arch = "wasm32")]
    let result = crate::save_paths::try_load_browser_battery_sram(
        crate::save_paths::BrowserBatterySramRequest {
            rom_path,
            system_subdir: "nes",
            media_identity: emu.rom_hash(),
            component: crate::save_paths::SRAM_COMPONENT,
            system_label: "NES",
            has_battery: emu.has_battery(),
        },
        |bytes| emu.load_persistent_data(bytes),
    );
    result
}
