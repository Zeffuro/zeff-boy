use std::path::{Path, PathBuf};

use zeff_emu_common::address::{Address, narrow_u16};
use zeff_emu_common::memory::{MemoryRegionDescriptor, MemoryRegionKind, resolve_memory_region};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::time::{FrameLifecycle, MachineTiming, Reset, TimingSnapshot};
use zeff_sega8_core::emulator::Emulator as Sega8Emulator;
use zeff_sega8_core::hardware::cartridge::{Sega8MapperKind, Sega8System, SystemHint};
use zeff_sega8_core::hardware::region::Sega8Region;
use zeff_sega8_core::hardware::timing::Sega8VideoStandard;

use crate::audio_tooling::{
    AudioChannelDescriptor, AudioChannelId, AudioSemanticCaps, AudioSemanticFrame, AudioTopology,
    AudioVoiceClass, AudioVoiceState, NTSC_60_TEMPO_US_PER_BEAT,
};
use crate::cheats::CheatPatch;
use crate::emu_backend::ActiveSystem;
use crate::emu_backend::paths::BackendPaths;
use crate::emu_core_trait::{EmulatorCore, copy_slice_to_vec};

const SEGA8_AUDIO_CHANNELS: &[AudioChannelDescriptor] = &[
    AudioChannelDescriptor {
        id: AudioChannelId(0),
        name: "Sega PSG Tone 0",
        group: "SN76489 PSG",
        class: AudioVoiceClass::Tone,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(1),
        name: "Sega PSG Tone 1",
        group: "SN76489 PSG",
        class: AudioVoiceClass::Tone,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(2),
        name: "Sega PSG Tone 2",
        group: "SN76489 PSG",
        class: AudioVoiceClass::Tone,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(3),
        name: "Sega PSG Noise",
        group: "SN76489 PSG",
        class: AudioVoiceClass::Noise,
        caps: AudioSemanticCaps::GATE_LEVEL,
        muteable: true,
    },
];

impl crate::emu_core_trait::DebuggableEmulator for Sega8Emulator {
    fn add_breakpoint(&mut self, addr: Address) {
        self.add_breakpoint(addr);
    }
    fn add_one_shot_breakpoint(&mut self, addr: Address) {
        self.add_one_shot_breakpoint(addr);
    }

    fn add_breakpoint_after(&mut self, addr: Address, target_hits: u64) {
        self.add_breakpoint_after(addr, target_hits);
    }

    fn set_event_breakpoint(&mut self, event: zeff_emu_common::debug::DebugEvent, enabled: bool) {
        self.set_event_breakpoint(event, enabled);
    }

    fn add_watchpoint_range(
        &mut self,
        start: Address,
        end: Address,
        wt: zeff_emu_common::debug::WatchType,
    ) {
        self.add_watchpoint_range(start, end, wt);
    }
    fn remove_watchpoint(
        &mut self,
        start: Address,
        end: Address,
        wt: zeff_emu_common::debug::WatchType,
    ) {
        self.remove_watchpoint(start, end, wt);
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

pub(crate) struct Sega8Backend {
    pub(crate) emu: Sega8Emulator,
    paths: BackendPaths,
    sram_recovery: crate::save_paths::SramRecoverySession,
    sms_tas_load_provenance: Option<tas_provenance::SmsTasLoadProvenance>,
    game_gear_tas_load_provenance: Option<game_gear_tas_provenance::GameGearTasLoadProvenance>,
    sg1000_tas_load_provenance: Option<sg1000_tas_provenance::Sg1000TasLoadProvenance>,
}

impl Sega8Backend {
    pub(crate) fn battery_components(&self) -> Vec<(&'static str, Vec<u8>)> {
        self.emu
            .dump_battery_sram()
            .map(|bytes| vec![(crate::save_paths::SRAM_COMPONENT, bytes)])
            .unwrap_or_default()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn game_gear_tas_battery_bytes(&self) -> Option<Vec<u8>> {
        use zeff_sega8_core::hardware::cartridge::GameGearStandardMapperRam;

        let bytes = self.emu.dump_battery_sram()?;
        (self.system() == ActiveSystem::GameGear
            && self.emu.save_ram_kind() == SaveRamKind::known_battery_backed(8 * 1024)
            && self
                .game_gear_tas_load_provenance()
                .and_then(|provenance| provenance.standard_mapper_ram_identity)
                .is_some_and(|identity| {
                    identity.ram() == GameGearStandardMapperRam::BatteryBacked8KiB
                })
            && bytes.len() == 8 * 1024)
            .then_some(bytes)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn game_gear_tas_battery_baseline(
        &self,
    ) -> anyhow::Result<crate::save_paths::SaveTargetBaseline> {
        crate::save_paths::battery_sram_baseline(self.paths.rom_path())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn publish_game_gear_tas_battery_if_unchanged(
        &mut self,
        expected: crate::save_paths::SaveTargetBaseline,
    ) -> Option<(String, crate::save_paths::SavePublicationOutcome)> {
        let bytes = self.game_gear_tas_battery_bytes()?;
        Some(crate::save_paths::publish_battery_sram_if_unchanged(
            &mut self.sram_recovery,
            self.paths.rom_path(),
            ActiveSystem::GameGear.storage_subdir(),
            self.emu.rom_hash(),
            expected,
            &bytes,
        ))
    }

    pub(crate) fn new(emu: Sega8Emulator, rom_path: PathBuf) -> Self {
        let system = active_system_for_sega8(emu.system());
        let sram_recovery = crate::save_paths::battery_sram_session(
            &rom_path,
            system.storage_subdir(),
            emu.rom_hash(),
        );
        Self {
            emu,
            paths: BackendPaths::new(rom_path),
            sram_recovery,
            sms_tas_load_provenance: None,
            game_gear_tas_load_provenance: None,
            sg1000_tas_load_provenance: None,
        }
    }

    pub(crate) fn with_source_path(
        emu: Sega8Emulator,
        rom_path: PathBuf,
        source_path: PathBuf,
    ) -> Self {
        let system = active_system_for_sega8(emu.system());
        let sram_recovery = crate::save_paths::battery_sram_session(
            &rom_path,
            system.storage_subdir(),
            emu.rom_hash(),
        );
        Self {
            emu,
            paths: BackendPaths::with_source_path(rom_path, source_path),
            sram_recovery,
            sms_tas_load_provenance: None,
            game_gear_tas_load_provenance: None,
            sg1000_tas_load_provenance: None,
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

    pub(crate) fn system(&self) -> ActiveSystem {
        active_system_for_sega8(self.emu.system())
    }

    pub(crate) fn nominal_frame_duration_ns(&self) -> u64 {
        self.emu.video_standard().nominal_frame_duration_ns()
    }
}

impl EmulatorCore for Sega8Backend {
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
        let system_subdir = self.system().storage_subdir();
        let media_identity = self.emu.rom_hash();
        let sram = self.emu.dump_battery_sram();
        crate::save_paths::flush_battery_sram(
            &mut self.sram_recovery,
            self.paths.rom_path(),
            system_subdir,
            media_identity,
            sram,
        )
    }

    fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        self.emu.encode_state()
    }

    fn load_state_from_bytes(
        &mut self,
        bytes: Vec<u8>,
    ) -> anyhow::Result<zeff_emu_common::StateRestoreOutcome> {
        self.emu.load_state_from_bytes(bytes)?;
        Ok(zeff_emu_common::StateRestoreOutcome::Exact)
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

    fn audio_semantic_frame(&self) -> Option<AudioSemanticFrame> {
        Some(sega8_audio_semantic_frame(
            self.emu.frame_count(),
            self.emu.video_standard(),
            self.emu.bus().apu().debug_snapshot(),
        ))
    }

    fn audio_topology(&self) -> Option<AudioTopology> {
        Some(AudioTopology {
            generation: 1,
            channels: SEGA8_AUDIO_CHANNELS,
        })
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

impl MachineTiming for Sega8Backend {
    #[inline]
    fn timing_snapshot(&self) -> TimingSnapshot {
        self.emu.timing_snapshot()
    }
}

impl Reset for Sega8Backend {
    #[inline]
    fn reset(&mut self) {
        Reset::reset(&mut self.emu);
    }
}

impl FrameLifecycle for Sega8Backend {
    #[inline]
    fn step_frame(&mut self) {
        FrameLifecycle::step_frame(&mut self.emu);
    }

    #[inline]
    fn frame_count(&self) -> u64 {
        FrameLifecycle::frame_count(&self.emu)
    }
}

fn sega8_audio_semantic_frame(
    frame: u64,
    video_standard: Sega8VideoStandard,
    snap: zeff_sega8_core::hardware::apu::ApuDebugSnapshot,
) -> AudioSemanticFrame {
    let mut voices = Vec::with_capacity(zeff_sega8_core::hardware::apu::PSG_CHANNEL_COUNT);
    for channel in 0..zeff_sega8_core::hardware::apu::PSG_CHANNEL_COUNT {
        let volume = snap.volume.get(channel).copied().unwrap_or(15).min(15);
        let level = f32::from(15 - volume) / 15.0;
        let muted = snap.channel_mutes.get(channel).copied().unwrap_or(false);
        let active = volume < 15 && !muted;
        let tone_channel = channel < zeff_sega8_core::hardware::apu::PSG_TONE_CHANNEL_COUNT;

        voices.push(AudioVoiceState {
            channel: AudioChannelId(channel as u16),
            name: match channel {
                0 => "Sega PSG Tone 0",
                1 => "Sega PSG Tone 1",
                2 => "Sega PSG Tone 2",
                3 => "Sega PSG Noise",
                _ => "Sega PSG Unknown",
            },
            class: if tone_channel {
                AudioVoiceClass::Tone
            } else {
                AudioVoiceClass::Noise
            },
            active,
            pitch_hz: tone_channel
                .then(|| sega8_tone_freq_to_hz(video_standard, snap.tone_period[channel])),
            level: Some(level),
        });
    }

    AudioSemanticFrame {
        frame,
        tempo_us_per_beat: NTSC_60_TEMPO_US_PER_BEAT,
        voices,
    }
}

fn sega8_tone_freq_to_hz(video_standard: Sega8VideoStandard, period: u16) -> f64 {
    if period <= 1 {
        return 0.0;
    }
    f64::from(video_standard.clock_hz_approx()) / (32.0 * f64::from(period))
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
    #[cfg(not(target_arch = "wasm32"))]
    let result = crate::save_paths::try_load_battery_sram(
        rom_path,
        "Sega 8-bit",
        emu.has_battery(),
        |bytes| emu.load_battery_sram(bytes),
    );
    #[cfg(target_arch = "wasm32")]
    let result = crate::save_paths::try_load_browser_battery_sram(
        crate::save_paths::BrowserBatterySramRequest {
            rom_path,
            system_subdir: active_system_for_sega8(emu.system()).storage_subdir(),
            media_identity: emu.rom_hash(),
            component: crate::save_paths::SRAM_COMPONENT,
            system_label: "Sega 8-bit",
            has_battery: emu.has_battery(),
        },
        |bytes| emu.load_battery_sram(bytes),
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_pitch_uses_the_selected_video_standard_clock() {
        let period = 0x20;
        let ntsc = sega8_tone_freq_to_hz(Sega8VideoStandard::Ntsc, period);
        let pal = sega8_tone_freq_to_hz(Sega8VideoStandard::Pal, period);

        assert_eq!(
            ntsc,
            f64::from(Sega8VideoStandard::Ntsc.clock_hz_approx()) / (32.0 * f64::from(period))
        );
        assert_eq!(
            pal,
            f64::from(Sega8VideoStandard::Pal.clock_hz_approx()) / (32.0 * f64::from(period))
        );
        assert!(pal < ntsc);
    }
}
mod game_gear_tas_provenance;
mod game_gear_tas_runtime;
mod sg1000_tas_provenance;
pub(crate) mod tas_provenance;
pub(crate) use game_gear_tas_provenance::{
    GameGearTasLoadProvenance, GameGearTasLoadProvenanceSeed, GameGearTasLoadSetup,
    GameGearTasPersistentLoadOutcome, game_gear_persistent_load_outcome,
};
pub(crate) use game_gear_tas_runtime::{
    validate_direct_game_gear_tas_execution_runtime,
    validate_direct_game_gear_tas_private_execution_runtime,
    validate_direct_game_gear_tas_private_runtime, validate_direct_game_gear_tas_runtime,
};
pub(crate) use sg1000_tas_provenance::{
    Sg1000TasControllerModel, Sg1000TasLoadProvenance, Sg1000TasLoadProvenanceSeed,
    Sg1000TasLoadSetup, Sg1000TasPersistentLoadOutcome, sg1000_persistent_load_outcome,
};
pub(crate) use tas_provenance::{SmsTasLoadProvenanceSeed, SmsTasLoadSetup};

pub(crate) enum Sega8TasLoadProvenanceSeed {
    MasterSystem(SmsTasLoadProvenanceSeed),
    GameGear(GameGearTasLoadProvenanceSeed),
    Sg1000(Sg1000TasLoadProvenanceSeed),
}
