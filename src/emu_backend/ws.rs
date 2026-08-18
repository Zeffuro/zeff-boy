use std::path::{Path, PathBuf};

use zeff_emu_common::address::Address;
use zeff_emu_common::memory::{MemoryRegionDescriptor, MemoryRegionKind, resolve_memory_region};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::time::{FrameLifecycle, MachineTiming, Reset, TimingSnapshot};
use zeff_ws_core::emulator::Emulator as WsEmulator;
use zeff_ws_core::hardware::apu::ApuDebugSnapshot;

use crate::audio_tooling::{
    AudioChannelId, AudioSemanticFrame, AudioVoiceClass, AudioVoiceState, WS_TEMPO_US_PER_BEAT,
};
use crate::emu_backend::paths::BackendPaths;
use crate::emu_core_trait::{EmulatorCore, copy_optional_region_to_vec, copy_slice_to_vec};

impl crate::emu_core_trait::DebuggableEmulator for WsEmulator {
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

pub(crate) struct WsBackend {
    pub(crate) emu: WsEmulator,
    paths: BackendPaths,
}

impl WsBackend {
    pub(crate) fn new(emu: WsEmulator, rom_path: PathBuf) -> Self {
        Self {
            emu,
            paths: BackendPaths::new(rom_path),
        }
    }

    pub(crate) fn with_source_path(
        emu: WsEmulator,
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

    pub(crate) fn firmware_manifests(&self) -> &[zeff_emu_common::replay::ReplayFirmwareManifest] {
        self.paths.firmware_manifests()
    }

    pub(crate) fn set_firmware_manifests(
        &mut self,
        firmware_manifests: Vec<zeff_emu_common::replay::ReplayFirmwareManifest>,
    ) {
        self.paths.set_firmware_manifests(firmware_manifests);
    }

    pub(crate) fn preferred_orientation(
        &self,
    ) -> zeff_ws_core::hardware::cartridge::RomOrientation {
        self.emu.preferred_orientation()
    }
}

impl EmulatorCore for WsBackend {
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

    fn supports_debugger(&self) -> bool {
        true
    }

    fn supports_opcode_history(&self) -> bool {
        true
    }

    fn cpu_address_bits(&self) -> u8 {
        20
    }

    fn audio_semantic_frame(&self) -> Option<AudioSemanticFrame> {
        Some(ws_audio_semantic_frame(
            self.emu.frame_count(),
            self.emu.apu_debug_snapshot(),
        ))
    }

    fn copy_memory_region(
        &mut self,
        id_or_alias: &str,
        out: &mut Vec<u8>,
    ) -> anyhow::Result<MemoryRegionDescriptor> {
        let regions = self.memory_regions();
        let region = resolve_memory_region(&regions, id_or_alias).ok_or_else(|| {
            anyhow::anyhow!("unknown memory region '{id_or_alias}' for WonderSwan")
        })?;

        match region.kind {
            MemoryRegionKind::SystemRam => copy_slice_to_vec(out, self.emu.system_ram()),
            MemoryRegionKind::VideoRam => copy_slice_to_vec(out, self.emu.video_ram_snapshot()),
            MemoryRegionKind::SaveRam => {
                copy_optional_region_to_vec(out, self.emu.dump_battery_sram(), region.id)?
            }
            MemoryRegionKind::Framebuffer => copy_slice_to_vec(out, self.emu.framebuffer()),
            MemoryRegionKind::CpuAddressSpace => {
                return Err(anyhow::anyhow!(
                    "WonderSwan CPU address space is not copyable as a finite memory region"
                ));
            }
            MemoryRegionKind::PaletteRam
            | MemoryRegionKind::Oam
            | MemoryRegionKind::IoRegisters
            | MemoryRegionKind::ExternalWorkRam
            | MemoryRegionKind::InternalWorkRam => {
                return Err(anyhow::anyhow!(
                    "WonderSwan memory region '{}' is not exposed as a copyable region",
                    region.id
                ));
            }
        }

        Ok(region)
    }
}

impl MachineTiming for WsBackend {
    #[inline]
    fn timing_snapshot(&self) -> TimingSnapshot {
        self.emu.timing_snapshot()
    }
}

impl Reset for WsBackend {
    #[inline]
    fn reset(&mut self) {
        Reset::reset(&mut self.emu);
    }
}

impl FrameLifecycle for WsBackend {
    #[inline]
    fn step_frame(&mut self) {
        FrameLifecycle::step_frame(&mut self.emu);
    }

    #[inline]
    fn frame_count(&self) -> u64 {
        FrameLifecycle::frame_count(&self.emu)
    }
}

fn ws_audio_semantic_frame(frame: u64, snap: ApuDebugSnapshot) -> AudioSemanticFrame {
    let mut voices = Vec::with_capacity(5);
    for channel in 0..4 {
        voices.push(ws_channel_voice(&snap, channel));
    }
    voices.push(ws_hyper_voice(&snap));

    AudioSemanticFrame {
        frame,
        tempo_us_per_beat: WS_TEMPO_US_PER_BEAT,
        voices,
    }
}

fn ws_channel_voice(snap: &ApuDebugSnapshot, channel: usize) -> AudioVoiceState {
    let mode = ws_channel_mode(snap, channel);
    AudioVoiceState {
        channel: AudioChannelId(channel as u16),
        name: ws_channel_name(mode, channel),
        class: ws_channel_class(mode),
        active: ws_channel_active(snap, mode, channel),
        pitch_hz: ws_channel_pitch_hz(snap, mode, channel),
        level: Some(ws_channel_level(snap, mode, channel)),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WsChannelMode {
    Wave,
    DirectVoice,
    Sweep,
    Noise,
}

fn ws_channel_mode(snap: &ApuDebugSnapshot, channel: usize) -> WsChannelMode {
    match channel {
        1 if snap.control & 0x20 != 0 => WsChannelMode::DirectVoice,
        2 if snap.control & 0x40 != 0 => WsChannelMode::Sweep,
        3 if snap.control & 0x80 != 0 && snap.noise_control & 0x10 != 0 => WsChannelMode::Noise,
        _ => WsChannelMode::Wave,
    }
}

fn ws_channel_name(mode: WsChannelMode, channel: usize) -> &'static str {
    match (channel, mode) {
        (0, _) => "WS CH0 Wave",
        (1, WsChannelMode::DirectVoice) => "WS CH1 Direct Voice",
        (1, _) => "WS CH1 Wave",
        (2, WsChannelMode::Sweep) => "WS CH2 Sweep",
        (2, _) => "WS CH2 Wave",
        (3, WsChannelMode::Noise) => "WS CH3 Noise",
        (3, _) => "WS CH3 Wave",
        _ => "WS Audio Channel",
    }
}

fn ws_channel_class(mode: WsChannelMode) -> AudioVoiceClass {
    match mode {
        WsChannelMode::Wave | WsChannelMode::Sweep => AudioVoiceClass::Wavetable,
        WsChannelMode::DirectVoice => AudioVoiceClass::Pcm,
        WsChannelMode::Noise => AudioVoiceClass::Noise,
    }
}

fn ws_channel_active(snap: &ApuDebugSnapshot, mode: WsChannelMode, channel: usize) -> bool {
    snap.control & (1 << channel) != 0
        && !snap.channel_mutes[channel]
        && ws_channel_level(snap, mode, channel) > 0.0
}

fn ws_channel_pitch_hz(
    snap: &ApuDebugSnapshot,
    mode: WsChannelMode,
    channel: usize,
) -> Option<f64> {
    matches!(mode, WsChannelMode::Wave | WsChannelMode::Sweep)
        .then(|| ws_period_to_hz(snap.period[channel]))
}

fn ws_period_to_hz(period: u16) -> f64 {
    let clocks = 2048u16.saturating_sub(period & 0x07FF);
    if clocks <= 4 {
        return 0.0;
    }
    f64::from(zeff_ws_core::hardware::constants::CPU_CLOCK_HZ) / f64::from(clocks) / 32.0
}

fn ws_channel_level(snap: &ApuDebugSnapshot, mode: WsChannelMode, channel: usize) -> f32 {
    let volume = snap.volume[channel];
    if mode == WsChannelMode::DirectVoice {
        let full = f32::from(volume) / 255.0;
        let half = full * 0.5;
        let left = if snap.voice_volume & 0x04 != 0 {
            full
        } else if snap.voice_volume & 0x08 != 0 {
            half
        } else {
            0.0
        };
        let right = if snap.voice_volume & 0x01 != 0 {
            full
        } else if snap.voice_volume & 0x02 != 0 {
            half
        } else {
            0.0
        };
        return left.max(right);
    }

    let left = volume >> 4;
    let right = volume & 0x0F;
    f32::from(left.max(right)) / 15.0
}

fn ws_hyper_voice(snap: &ApuDebugSnapshot) -> AudioVoiceState {
    let left = f32::from(snap.hyper_voice_left_output).abs() / 32768.0;
    let right = f32::from(snap.hyper_voice_right_output).abs() / 32768.0;
    AudioVoiceState {
        channel: AudioChannelId(4),
        name: "WS HyperVoice",
        class: AudioVoiceClass::Pcm,
        active: snap.hyper_voice_control & 0x80 != 0 && (left > 0.0 || right > 0.0),
        pitch_hz: None,
        level: Some(left.max(right).clamp(0.0, 1.0)),
    }
}

pub(crate) fn try_load_battery_sram(
    emu: &mut WsEmulator,
    rom_path: &Path,
) -> anyhow::Result<Option<String>> {
    crate::save_paths::try_load_battery_sram(rom_path, "WS", emu.has_battery(), |bytes| {
        emu.load_battery_sram(bytes)
    })
}
