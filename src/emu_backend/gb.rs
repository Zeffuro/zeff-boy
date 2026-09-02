use std::path::{Path, PathBuf};

use zeff_emu_common::address::{Address, narrow_u16};
use zeff_emu_common::memory::{MemoryRegionDescriptor, MemoryRegionKind, resolve_memory_region};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::time::{FrameLifecycle, MachineTiming, Reset, TimingSnapshot};
use zeff_gb_core::emulator::Emulator as GbEmulator;

use crate::audio_tooling::{
    AudioChannelDescriptor, AudioChannelId, AudioSemanticCaps, AudioSemanticFrame, AudioTopology,
    AudioVoiceClass, AudioVoiceState, GB_TEMPO_US_PER_BEAT, level_from_u4,
};
use crate::cheats::CheatPatch;
use crate::emu_backend::paths::BackendPaths;
use crate::emu_core_trait::{EmulatorCore, copy_optional_region_to_vec, copy_slice_to_vec};

mod tas_provenance;
pub(crate) use tas_provenance::{
    GbPersistentLoadOutcome, GbTasLoadProvenance, GbTasLoadProvenanceView, GbTasLoadSetup,
};
pub(crate) use tas_provenance::{GbTasLoadProvenanceSeed, persistent_load_outcome};

const GB_AUDIO_CHANNELS: &[AudioChannelDescriptor] = &[
    AudioChannelDescriptor {
        id: AudioChannelId(0),
        name: "GB CH1 (Square 1)",
        group: "Game Boy APU",
        class: AudioVoiceClass::Pulse,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(1),
        name: "GB CH2 (Square 2)",
        group: "Game Boy APU",
        class: AudioVoiceClass::Pulse,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(2),
        name: "GB CH3 (Wave)",
        group: "Game Boy APU",
        class: AudioVoiceClass::Wavetable,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(3),
        name: "GB CH4 (Noise)",
        group: "Game Boy APU",
        class: AudioVoiceClass::Noise,
        caps: AudioSemanticCaps::GATE_LEVEL,
        muteable: true,
    },
];

impl crate::emu_core_trait::DebuggableEmulator for GbEmulator {
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

pub(crate) struct GbBackend {
    pub(crate) emu: GbEmulator,
    paths: BackendPaths,
    sram_recovery: crate::save_paths::SramRecoverySession,
    tas_load_provenance: Option<GbTasLoadProvenance>,
}

impl GbBackend {
    pub(crate) fn battery_components(&self) -> Vec<(&'static str, Vec<u8>)> {
        self.emu
            .dump_battery_sram()
            .map(|bytes| vec![(crate::save_paths::SRAM_COMPONENT, bytes)])
            .unwrap_or_default()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn persisted_rtc_battery_receipt(
        &self,
    ) -> anyhow::Result<Option<crate::save_paths::recovery_state::BatteryPublicationReceipt>> {
        if !self.emu.header().cartridge_type.is_mbc3_with_rtc() {
            return Ok(None);
        }
        let Some(bytes) = crate::platform::read_save_data(&crate::save_paths::sram_path_for_rom(
            self.paths.rom_path(),
        ))?
        else {
            return Ok(Some(
                crate::save_paths::recovery_state::BatteryPublicationReceipt::from_components(&[]),
            ));
        };
        rtc_battery_receipt(&self.emu, &bytes)
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("persisted Game Boy RTC sidecar layout is invalid"))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn tas_battery_bytes(&self) -> Option<Vec<u8>> {
        self.emu.dump_battery_sram()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn tas_battery_baseline(
        &self,
    ) -> anyhow::Result<crate::save_paths::SaveTargetBaseline> {
        crate::save_paths::battery_sram_baseline(self.paths.rom_path())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn publish_tas_battery_if_unchanged(
        &mut self,
        expected: crate::save_paths::SaveTargetBaseline,
    ) -> Option<(String, crate::save_paths::SavePublicationOutcome)> {
        let bytes = self.emu.dump_battery_sram()?;
        Some(crate::save_paths::publish_battery_sram_if_unchanged(
            &mut self.sram_recovery,
            self.paths.rom_path(),
            crate::emu_backend::ActiveSystem::Gb.storage_subdir(),
            self.emu.rom_hash(),
            expected,
            &bytes,
        ))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn publish_tas_rtc_battery_if_unchanged(
        &mut self,
        expected: crate::save_paths::SaveTargetBaseline,
    ) -> Option<(
        String,
        crate::save_paths::SavePublicationOutcome,
        crate::save_paths::recovery_state::BatteryPublicationReceipt,
    )> {
        let bytes = self.emu.dump_battery_sram_with_rtc_subsecond()?;
        let ram_len = self.emu.header().ram_size.size_bytes();
        if bytes.len() != ram_len + 64 {
            return None;
        }
        let receipt = rtc_battery_receipt(&self.emu, &bytes)?;
        Some(crate::save_paths::publish_battery_aggregate_if_unchanged(
            &mut self.sram_recovery,
            self.paths.rom_path(),
            crate::save_paths::SaveRecoveryIdentity {
                system_subdir: crate::emu_backend::ActiveSystem::Gb.storage_subdir(),
                media_identity: self.emu.rom_hash(),
                component: crate::save_paths::SRAM_COMPONENT,
            },
            expected,
            &bytes,
            receipt,
        ))
    }

    #[allow(dead_code)]
    pub(crate) fn new(emu: GbEmulator, rom_path: PathBuf) -> Self {
        let sram_recovery = crate::save_paths::battery_sram_session(
            &rom_path,
            crate::emu_backend::ActiveSystem::Gb.storage_subdir(),
            emu.rom_hash(),
        );
        Self {
            emu,
            paths: BackendPaths::new(rom_path),
            sram_recovery,
            tas_load_provenance: None,
        }
    }

    pub(crate) fn source_path(&self) -> &Path {
        self.paths.source_path()
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
}

impl EmulatorCore for GbBackend {
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
        crate::save_paths::flush_battery_sram(
            &mut self.sram_recovery,
            self.paths.rom_path(),
            crate::emu_backend::ActiveSystem::Gb.storage_subdir(),
            self.emu.rom_hash(),
            self.emu.dump_battery_sram(),
        )
    }

    fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        self.emu.encode_state_bytes()
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
        self.emu.video_ram_snapshot().len()
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
        self.emu.clear_rom_patches();
        for patch in cheats.iter().copied().filter(|patch| patch.is_rom_patch()) {
            self.emu.add_rom_patch(patch);
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

    fn audio_topology(&self) -> Option<AudioTopology> {
        Some(AudioTopology {
            generation: 1,
            channels: GB_AUDIO_CHANNELS,
        })
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

impl MachineTiming for GbBackend {
    #[inline]
    fn timing_snapshot(&self) -> TimingSnapshot {
        self.emu.timing_snapshot()
    }
}

impl Reset for GbBackend {
    #[inline]
    fn reset(&mut self) {
        Reset::reset(&mut self.emu);
    }
}

impl FrameLifecycle for GbBackend {
    #[inline]
    fn step_frame(&mut self) {
        FrameLifecycle::step_frame(&mut self.emu);
    }

    #[inline]
    fn frame_count(&self) -> u64 {
        FrameLifecycle::frame_count(&self.emu)
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
    try_load_battery_sram_at_time(emu, rom_path, None)
}

fn rtc_battery_receipt(
    emu: &GbEmulator,
    bytes: &[u8],
) -> Option<crate::save_paths::recovery_state::BatteryPublicationReceipt> {
    let ram_len = emu.header().ram_size.size_bytes();
    if !matches!(bytes.len().checked_sub(ram_len), Some(44 | 48 | 64)) {
        return None;
    }
    crate::save_paths::aggregate_battery_receipt(
        bytes,
        ram_len,
        crate::save_paths::SRAM_COMPONENT,
        crate::save_paths::GB_RTC_COMPONENT,
    )
}

pub(crate) fn try_load_battery_sram_at_time(
    emu: &mut GbEmulator,
    rom_path: &Path,
    rtc_time_override: Option<u64>,
) -> anyhow::Result<Option<String>> {
    #[cfg(not(target_arch = "wasm32"))]
    let result =
        crate::save_paths::try_load_battery_sram(rom_path, "GB", emu.has_battery(), |bytes| {
            if let Some(unix_seconds) = rtc_time_override {
                emu.load_battery_sram_at_time(bytes, unix_seconds)
            } else {
                emu.load_battery_sram(bytes)
            }
        });
    #[cfg(target_arch = "wasm32")]
    let result = crate::save_paths::try_load_browser_battery_sram(
        crate::save_paths::BrowserBatterySramRequest {
            rom_path,
            system_subdir: "gb",
            media_identity: emu.rom_hash(),
            component: crate::save_paths::SRAM_COMPONENT,
            system_label: "GB",
            has_battery: emu.has_battery(),
        },
        |bytes| {
            if let Some(unix_seconds) = rtc_time_override {
                emu.load_battery_sram_at_time(bytes, unix_seconds)
            } else {
                emu.load_battery_sram(bytes)
            }
        },
    );
    result
}
