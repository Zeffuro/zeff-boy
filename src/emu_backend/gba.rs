use std::path::{Path, PathBuf};

use zeff_emu_common::address::Address;
use zeff_emu_common::memory::{MemoryRegionDescriptor, MemoryRegionKind, resolve_memory_region};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::time::{FrameLifecycle, MachineTiming, Reset, TimingSnapshot};
use zeff_gba_core::emulator::Emulator as GbaEmulator;
use zeff_gba_core::hardware::apu::ApuDebugSnapshot;
use zeff_gba_core::hardware::cartridge::BackupKind;

use crate::audio_tooling::{
    AudioChannelDescriptor, AudioChannelId, AudioSemanticCaps, AudioSemanticFrame, AudioTopology,
    AudioVoiceClass, AudioVoiceState, NTSC_60_TEMPO_US_PER_BEAT, level_from_u4,
};
use crate::cheats::CheatPatch;
use crate::emu_backend::paths::BackendPaths;
use crate::emu_core_trait::{EmulatorCore, copy_optional_region_to_vec, copy_slice_to_vec};

mod tas_provenance;
#[cfg(not(target_arch = "wasm32"))]
mod tas_runtime;

pub(crate) use tas_provenance::{
    GbaTasLoadProvenance, GbaTasLoadProvenanceSeed, GbaTasLoadProvenanceView, GbaTasLoadSetup,
    GbaTasPersistentLoadOutcome, persistent_load_outcome,
};
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use tas_runtime::{
    DIRECT_GBA_SAMPLE_RATE, MAX_DIRECT_GBA_ROM_BYTES, direct_gba_tas_identity,
    direct_gba_tas_sync_config_sha256, gba_rtc_persistence_witness, gba_tas_sync_config,
    is_gba_tilt_tas_identity, restore_direct_gba_tas_execution_state,
    supported_gba_rtc_backup_kinds, validate_direct_gba_tas_branch_scope,
    validate_direct_gba_tas_execution_runtime, validate_direct_gba_tas_private_execution_runtime,
    validate_direct_gba_tas_private_runtime, validate_direct_gba_tas_project_identity,
    validate_direct_gba_tas_project_witness, validate_direct_gba_tas_runtime,
    validate_direct_gba_tas_state, zip_gba_battery_tas_sync_config_sha256,
    zip_gba_rtc_tas_sync_config_sha256, zip_gba_tas_identity, zip_gba_tas_sync_config_sha256,
    zip_gba_tilt_tas_sync_config_sha256,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GbaRtcPersistenceWitness {
    pub(crate) backup_kind: BackupKind,
    pub(crate) persistent_state: crate::tas_project::TasExternalIdentity,
    pub(crate) rtc_state: crate::tas_project::TasExternalIdentity,
    pub(crate) complete_byte_len: u64,
    pub(crate) complete_sha256: crate::tas_project::TasDigest,
}

const GBA_AUDIO_CHANNELS: &[AudioChannelDescriptor] = &[
    AudioChannelDescriptor {
        id: AudioChannelId(0),
        name: "GBA PSG 1 (Square + Sweep)",
        group: "PSG",
        class: AudioVoiceClass::Pulse,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(1),
        name: "GBA PSG 2 (Square)",
        group: "PSG",
        class: AudioVoiceClass::Pulse,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(2),
        name: "GBA PSG 3 (Wave)",
        group: "PSG",
        class: AudioVoiceClass::Wavetable,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(3),
        name: "GBA PSG 4 (Noise)",
        group: "PSG",
        class: AudioVoiceClass::Noise,
        caps: AudioSemanticCaps::GATE_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(4),
        name: "GBA FIFO A",
        group: "Direct Sound",
        class: AudioVoiceClass::Pcm,
        caps: AudioSemanticCaps::GATE_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(5),
        name: "GBA FIFO B",
        group: "Direct Sound",
        class: AudioVoiceClass::Pcm,
        caps: AudioSemanticCaps::GATE_LEVEL,
        muteable: true,
    },
];

impl crate::emu_core_trait::DebuggableEmulator for GbaEmulator {
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

pub(crate) struct GbaBackend {
    pub(crate) emu: GbaEmulator,
    paths: BackendPaths,
    sram_recovery: crate::save_paths::SramRecoverySession,
    tas_load_provenance: Option<GbaTasLoadProvenance>,
}

impl GbaBackend {
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
        if self.tas_load_provenance.is_none() || !self.emu.has_rtc() {
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
            .ok_or_else(|| anyhow::anyhow!("persisted GBA RTC sidecar layout is invalid"))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn tas_rtc_battery_bytes(&self) -> Option<Vec<u8>> {
        self.emu.dump_complete_rtc_persistence()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn tas_battery_component(
        &self,
    ) -> anyhow::Result<Option<(crate::emu_thread::TasGbaPersistenceKind, Vec<u8>)>> {
        use crate::emu_thread::TasGbaPersistenceKind;
        use zeff_gba_core::hardware::cartridge::BackupKind;

        let kind = tas_runtime::validate_gba_backup_kind(&self.emu)?;
        let Some(bytes) = self.emu.dump_battery_sram() else {
            anyhow::ensure!(kind == BackupKind::None, "GBA battery state is unavailable");
            return Ok(None);
        };
        anyhow::ensure!(
            bytes.len() == kind.size(),
            "GBA battery state has the wrong size"
        );
        let kind = match kind {
            BackupKind::Sram => TasGbaPersistenceKind::Sram,
            BackupKind::Flash512 => TasGbaPersistenceKind::Flash512,
            BackupKind::Flash1M => TasGbaPersistenceKind::Flash1M,
            BackupKind::Eeprom => TasGbaPersistenceKind::Eeprom,
            BackupKind::None => anyhow::bail!("GBA battery state has no backup kind"),
        };
        Ok(Some((kind, bytes)))
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
    ) -> anyhow::Result<Option<(String, crate::save_paths::SavePublicationOutcome)>> {
        let Some((_, bytes)) = self.tas_battery_component()? else {
            return Ok(None);
        };
        Ok(Some(crate::save_paths::publish_battery_sram_if_unchanged(
            &mut self.sram_recovery,
            self.paths.rom_path(),
            crate::emu_backend::ActiveSystem::GameBoyAdvance.storage_subdir(),
            self.emu.rom_hash(),
            expected,
            &bytes,
        )))
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
        let bytes = self.emu.dump_complete_rtc_persistence()?;
        let receipt = rtc_battery_receipt(&self.emu, &bytes)?;
        Some(crate::save_paths::publish_battery_aggregate_if_unchanged(
            &mut self.sram_recovery,
            self.paths.rom_path(),
            crate::save_paths::SaveRecoveryIdentity {
                system_subdir: crate::emu_backend::ActiveSystem::GameBoyAdvance.storage_subdir(),
                media_identity: self.emu.rom_hash(),
                component: crate::save_paths::SRAM_COMPONENT,
            },
            expected,
            &bytes,
            receipt,
        ))
    }

    #[allow(dead_code)]
    pub(crate) fn new(emu: GbaEmulator, rom_path: PathBuf) -> Self {
        let sram_recovery =
            crate::save_paths::battery_sram_session(&rom_path, "gba", emu.rom_hash());
        Self {
            emu,
            paths: BackendPaths::new(rom_path),
            sram_recovery,
            tas_load_provenance: None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn with_source_path(
        emu: GbaEmulator,
        rom_path: PathBuf,
        source_path: PathBuf,
    ) -> Self {
        let sram_recovery =
            crate::save_paths::battery_sram_session(&rom_path, "gba", emu.rom_hash());
        Self {
            emu,
            paths: BackendPaths::with_source_path(rom_path, source_path),
            sram_recovery,
            tas_load_provenance: None,
        }
    }

    pub(crate) fn with_tas_load_provenance(
        emu: GbaEmulator,
        rom_path: PathBuf,
        source_path: PathBuf,
        provenance: GbaTasLoadProvenance,
    ) -> Self {
        let sram_recovery =
            crate::save_paths::battery_sram_session(&rom_path, "gba", emu.rom_hash());
        Self {
            emu,
            paths: BackendPaths::with_source_path(rom_path, source_path),
            sram_recovery,
            tas_load_provenance: Some(provenance),
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

    #[allow(dead_code)]
    pub(crate) fn tas_load_provenance(&self) -> Option<GbaTasLoadProvenanceView<'_>> {
        Some(GbaTasLoadProvenanceView {
            load: self.tas_load_provenance.as_ref()?,
            current_sample_rate: self.emu.apu_debug_snapshot().sample_rate,
            external_bios_present: self.emu.has_external_bios(),
        })
    }

    pub(crate) fn tas_source_media_identity(
        &self,
    ) -> Option<crate::emu_backend::capabilities::TasSourceMediaIdentity> {
        self.tas_load_provenance
            .map(GbaTasLoadProvenance::source_media_identity)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn set_tas_sync_config_sha256(&mut self, sync_config_sha256: [u8; 32]) {
        if let Some(provenance) = &mut self.tas_load_provenance {
            provenance.set_sync_config_sha256(sync_config_sha256);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn rtc_battery_receipt(
    emu: &GbaEmulator,
    bytes: &[u8],
) -> Option<crate::save_paths::recovery_state::BatteryPublicationReceipt> {
    let backup_len = emu.backup_kind().size();
    if bytes.len() != backup_len + 40 {
        return None;
    }
    let mut validation = emu.clone();
    validation.load_complete_rtc_persistence(bytes).ok()?;
    crate::save_paths::aggregate_battery_receipt(
        bytes,
        backup_len,
        crate::save_paths::GBA_BACKUP_COMPONENT,
        crate::save_paths::GBA_RTC_COMPONENT,
    )
}

impl EmulatorCore for GbaBackend {
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
        let persistence = self
            .tas_load_provenance
            .as_ref()
            .and_then(|_| self.emu.dump_complete_rtc_persistence())
            .or_else(|| self.emu.dump_battery_sram());
        crate::save_paths::flush_battery_sram(
            &mut self.sram_recovery,
            self.paths.rom_path(),
            "gba",
            self.emu.rom_hash(),
            persistence,
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

    fn supports_save_states(&self) -> bool {
        true
    }

    fn supports_audio(&self) -> bool {
        true
    }

    fn supports_cheats(&self) -> bool {
        true
    }

    fn apply_ram_cheats(&mut self, cheats: &[CheatPatch]) {
        zeff_emu_common::cheats::apply_wide_ram_cheats(&mut self.emu, cheats);
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
        Some(gba_audio_semantic_frame(
            self.emu.frame_count(),
            self.emu.apu_debug_snapshot(),
        ))
    }

    fn audio_topology(&self) -> Option<AudioTopology> {
        Some(AudioTopology {
            generation: 1,
            channels: GBA_AUDIO_CHANNELS,
        })
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

impl MachineTiming for GbaBackend {
    #[inline]
    fn timing_snapshot(&self) -> TimingSnapshot {
        self.emu.timing_snapshot()
    }
}

impl Reset for GbaBackend {
    #[inline]
    fn reset(&mut self) {
        Reset::reset(&mut self.emu);
    }
}

impl FrameLifecycle for GbaBackend {
    #[inline]
    fn step_frame(&mut self) {
        FrameLifecycle::step_frame(&mut self.emu);
    }

    #[inline]
    fn frame_count(&self) -> u64 {
        FrameLifecycle::frame_count(&self.emu)
    }
}

fn gba_audio_semantic_frame(frame: u64, snap: ApuDebugSnapshot) -> AudioSemanticFrame {
    AudioSemanticFrame {
        frame,
        tempo_us_per_beat: NTSC_60_TEMPO_US_PER_BEAT,
        voices: vec![
            AudioVoiceState {
                channel: AudioChannelId(0),
                name: "GBA PSG 1 (Square + Sweep)",
                class: AudioVoiceClass::Pulse,
                active: psg_active(&snap, 0),
                pitch_hz: Some(gb_square_freq_to_hz(snap.psg_frequency[0])),
                level: Some(level_from_u4(snap.psg_volume[0])),
            },
            AudioVoiceState {
                channel: AudioChannelId(1),
                name: "GBA PSG 2 (Square)",
                class: AudioVoiceClass::Pulse,
                active: psg_active(&snap, 1),
                pitch_hz: Some(gb_square_freq_to_hz(snap.psg_frequency[1])),
                level: Some(level_from_u4(snap.psg_volume[1])),
            },
            AudioVoiceState {
                channel: AudioChannelId(2),
                name: "GBA PSG 3 (Wave)",
                class: AudioVoiceClass::Wavetable,
                active: psg_active(&snap, 2),
                pitch_hz: Some(gb_wave_freq_to_hz(snap.psg_frequency[2])),
                level: Some(gb_wave_level(snap.psg_volume[2])),
            },
            AudioVoiceState {
                channel: AudioChannelId(3),
                name: "GBA PSG 4 (Noise)",
                class: AudioVoiceClass::Noise,
                active: psg_active(&snap, 3),
                pitch_hz: None,
                level: Some(level_from_u4(snap.psg_volume[3])),
            },
            gba_fifo_voice(&snap, 0),
            gba_fifo_voice(&snap, 1),
        ],
    }
}

fn psg_active(snap: &ApuDebugSnapshot, channel: usize) -> bool {
    snap.psg_enabled[channel] && !snap.channel_mutes[channel]
}

fn gba_fifo_voice(snap: &ApuDebugSnapshot, fifo: usize) -> AudioVoiceState {
    let mute_index = 4 + fifo;
    let sample = snap.current_sample[fifo];
    AudioVoiceState {
        channel: AudioChannelId(mute_index as u16),
        name: if fifo == 0 {
            "GBA FIFO A"
        } else {
            "GBA FIFO B"
        },
        class: AudioVoiceClass::Pcm,
        active: !snap.channel_mutes[mute_index] && (snap.fifo_len[fifo] > 0 || sample != 0),
        pitch_hz: None,
        level: Some((f32::from(sample).abs() / 127.0).clamp(0.0, 1.0)),
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
    emu: &mut GbaEmulator,
    rom_path: &Path,
) -> anyhow::Result<Option<String>> {
    #[cfg(not(target_arch = "wasm32"))]
    let result =
        crate::save_paths::try_load_battery_sram(rom_path, "GBA", emu.has_battery(), |bytes| {
            emu.load_battery_sram(bytes)
        });
    #[cfg(target_arch = "wasm32")]
    let result = crate::save_paths::try_load_browser_battery_sram(
        crate::save_paths::BrowserBatterySramRequest {
            rom_path,
            system_subdir: "gba",
            media_identity: emu.rom_hash(),
            component: crate::save_paths::SRAM_COMPONENT,
            system_label: "GBA",
            has_battery: emu.has_battery(),
        },
        |bytes| emu.load_battery_sram(bytes),
    );
    result
}
