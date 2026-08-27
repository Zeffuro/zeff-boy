use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use zeff_emu_common::address::{Address, narrow_u16};
use zeff_emu_common::memory::{MemoryRegionDescriptor, MemoryRegionKind, resolve_memory_region};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::time::{FrameLifecycle, MachineTiming, Reset, TimingSnapshot};

use crate::audio_tooling::{
    AudioChannelDescriptor, AudioChannelId, AudioSemanticCaps, AudioSemanticFrame, AudioTopology,
    AudioVoiceClass, AudioVoiceState, NTSC_60_TEMPO_US_PER_BEAT,
};
use crate::cheats::CheatPatch;
use crate::emu_backend::paths::BackendPaths;
use crate::emu_core_trait::{DebuggableEmulator, EmulatorCore, copy_slice_to_vec};

const COLECO_AUDIO_CHANNELS: &[AudioChannelDescriptor] = &[
    AudioChannelDescriptor {
        id: AudioChannelId(0),
        name: "Coleco PSG Tone 0",
        group: "SN76489A PSG",
        class: AudioVoiceClass::Tone,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(1),
        name: "Coleco PSG Tone 1",
        group: "SN76489A PSG",
        class: AudioVoiceClass::Tone,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(2),
        name: "Coleco PSG Tone 2",
        group: "SN76489A PSG",
        class: AudioVoiceClass::Tone,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(3),
        name: "Coleco PSG Noise",
        group: "SN76489A PSG",
        class: AudioVoiceClass::Noise,
        caps: AudioSemanticCaps::GATE_LEVEL,
        muteable: true,
    },
];

impl DebuggableEmulator for zeff_coleco_core::Emulator {
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
        watch_type: zeff_emu_common::debug::WatchType,
    ) {
        self.add_watchpoint_range(start, end, watch_type);
    }

    fn remove_watchpoint(
        &mut self,
        start: Address,
        end: Address,
        watch_type: zeff_emu_common::debug::WatchType,
    ) {
        self.remove_watchpoint(start, end, watch_type);
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

    fn cpu_write8(&mut self, addr: Address, value: u8) {
        self.cpu_write8(narrow_u16(addr), value);
    }

    fn is_cpu_suspended(&self) -> bool {
        self.is_cpu_suspended()
    }

    fn debug_continue(&mut self) {
        self.debug_continue();
    }

    fn debug_step(&mut self) {
        self.debug_step();
    }

    fn supports_opcode_history(&self) -> bool {
        true
    }

    fn set_opcode_log_enabled(&mut self, enabled: bool) {
        self.set_opcode_log_enabled(enabled);
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

pub(crate) struct ColecoBackend {
    pub(crate) emu: zeff_coleco_core::Emulator,
    paths: BackendPaths,
    rom_hash: [u8; 32],
}

impl ColecoBackend {
    pub(crate) fn new(
        emu: zeff_coleco_core::Emulator,
        rom_path: PathBuf,
        rom_hash: [u8; 32],
    ) -> Self {
        Self {
            emu,
            paths: BackendPaths::new(rom_path),
            rom_hash,
        }
    }

    pub(crate) fn with_source_path(
        emu: zeff_coleco_core::Emulator,
        rom_path: PathBuf,
        source_path: PathBuf,
        rom_hash: [u8; 32],
    ) -> Self {
        Self {
            emu,
            paths: BackendPaths::with_source_path(rom_path, source_path),
            rom_hash,
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

    pub(crate) fn rom_hash_for_bytes(rom: &[u8]) -> [u8; 32] {
        Sha256::digest(rom).into()
    }
}

impl EmulatorCore for ColecoBackend {
    fn framebuffer(&self) -> &[u8] {
        self.emu.framebuffer()
    }

    fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.emu.drain_audio_samples_into(buf);
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.emu.set_sample_rate(rate);
    }

    fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.emu.set_audio_generation_enabled(enabled);
    }

    fn set_apu_channel_mutes(&mut self, mutes: &[bool]) {
        let mutes = std::array::from_fn(|index| mutes.get(index).copied().unwrap_or(false));
        self.emu.set_audio_channel_mutes(mutes);
    }

    fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.emu
            .set_controller(0, controller_from_input(buttons_pressed, dpad_pressed));
    }

    fn set_input_p2(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.emu
            .set_controller(1, controller_from_input(buttons_pressed, dpad_pressed));
    }

    fn is_suspended(&self) -> bool {
        self.emu.is_suspended()
    }

    fn flush_battery_sram(&mut self) -> anyhow::Result<Option<String>> {
        Ok(None)
    }

    fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        self.emu.save_state()
    }

    fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.emu.load_state(&bytes)
    }

    fn state_restores_framebuffer(&self) -> bool {
        true
    }

    fn rom_path(&self) -> &Path {
        self.paths.rom_path()
    }

    fn rom_hash(&self) -> [u8; 32] {
        self.rom_hash
    }

    fn save_ram_kind(&self) -> SaveRamKind {
        SaveRamKind::none()
    }

    fn system_ram_len(&self) -> usize {
        zeff_coleco_core::constants::WORK_RAM_SIZE
    }

    fn video_ram_len(&self) -> usize {
        zeff_coleco_core::constants::VRAM_SIZE
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
        Some(coleco_audio_semantic_frame(&self.emu))
    }

    fn audio_topology(&self) -> Option<AudioTopology> {
        Some(AudioTopology {
            generation: 1,
            channels: COLECO_AUDIO_CHANNELS,
        })
    }

    fn copy_memory_region(
        &mut self,
        id_or_alias: &str,
        out: &mut Vec<u8>,
    ) -> anyhow::Result<MemoryRegionDescriptor> {
        let regions = self.memory_regions();
        let region = resolve_memory_region(&regions, id_or_alias).ok_or_else(|| {
            anyhow::anyhow!("unknown memory region '{id_or_alias}' for ColecoVision")
        })?;

        match region.kind {
            MemoryRegionKind::SystemRam => copy_slice_to_vec(out, self.emu.bus().work_ram()),
            MemoryRegionKind::VideoRam => copy_slice_to_vec(out, self.emu.bus().vdp().vram()),
            MemoryRegionKind::Framebuffer => copy_slice_to_vec(out, self.emu.framebuffer()),
            MemoryRegionKind::CpuAddressSpace => {
                anyhow::bail!(
                    "ColecoVision CPU address space is not copyable as a finite memory region"
                );
            }
            _ => anyhow::bail!(
                "ColecoVision memory region '{}' is not exposed as a copyable region",
                region.id
            ),
        }

        Ok(region)
    }
}

fn coleco_audio_semantic_frame(emu: &zeff_coleco_core::Emulator) -> AudioSemanticFrame {
    let psg = emu.bus().psg();
    let periods = psg.effective_tone_periods();
    let volumes = psg.volumes();
    let mutes = psg.channel_mutes();
    let mut voices = Vec::with_capacity(zeff_coleco_core::psg::PSG_CHANNEL_COUNT);

    for channel in 0..zeff_coleco_core::psg::PSG_CHANNEL_COUNT {
        let tone = channel < zeff_coleco_core::psg::PSG_TONE_CHANNEL_COUNT;
        let volume = volumes[channel].min(15);
        voices.push(AudioVoiceState {
            channel: AudioChannelId(channel as u16),
            name: COLECO_AUDIO_CHANNELS[channel].name,
            class: COLECO_AUDIO_CHANNELS[channel].class,
            active: volume < 15 && !mutes[channel],
            pitch_hz: tone.then(|| {
                f64::from(zeff_coleco_core::psg::COLECO_PSG_INPUT_CLOCK_HZ)
                    / (32.0 * f64::from(periods[channel]))
            }),
            level: Some(f32::from(15 - volume) / 15.0),
        });
    }

    AudioSemanticFrame {
        frame: emu.frame_count(),
        tempo_us_per_beat: NTSC_60_TEMPO_US_PER_BEAT,
        voices,
    }
}

impl MachineTiming for ColecoBackend {
    fn timing_snapshot(&self) -> TimingSnapshot {
        self.emu.timing_snapshot()
    }
}

impl Reset for ColecoBackend {
    fn reset(&mut self) {
        self.emu.reset();
    }
}

impl FrameLifecycle for ColecoBackend {
    fn step_frame(&mut self) {
        self.emu.step_frame();
    }

    fn frame_count(&self) -> u64 {
        self.emu.frame_count()
    }
}

fn controller_from_input(
    buttons_pressed: u8,
    dpad_pressed: u8,
) -> zeff_coleco_core::StandardController {
    use zeff_coleco_core::KeypadKey;

    zeff_coleco_core::StandardController {
        right: dpad_pressed & (1 << 0) != 0,
        left: dpad_pressed & (1 << 1) != 0,
        up: dpad_pressed & (1 << 2) != 0,
        down: dpad_pressed & (1 << 3) != 0,
        left_button: buttons_pressed & (1 << 0) != 0,
        right_button: buttons_pressed & (1 << 1) != 0,
        keypad: keypad_from_dpad(dpad_pressed).or({
            if buttons_pressed & (1 << 3) != 0 {
                Some(KeypadKey::Two)
            } else if buttons_pressed & (1 << 2) != 0 {
                Some(KeypadKey::One)
            } else {
                None
            }
        }),
    }
}

fn keypad_from_dpad(dpad_pressed: u8) -> Option<zeff_coleco_core::KeypadKey> {
    use zeff_coleco_core::KeypadKey;

    match dpad_pressed >> 4 {
        1 => Some(KeypadKey::Zero),
        2 => Some(KeypadKey::One),
        3 => Some(KeypadKey::Two),
        4 => Some(KeypadKey::Three),
        5 => Some(KeypadKey::Four),
        6 => Some(KeypadKey::Five),
        7 => Some(KeypadKey::Six),
        8 => Some(KeypadKey::Seven),
        9 => Some(KeypadKey::Eight),
        10 => Some(KeypadKey::Nine),
        11 => Some(KeypadKey::Star),
        12 => Some(KeypadKey::Pound),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeff_coleco_core::KeypadKey;

    #[test]
    fn encoded_keypad_values_preserve_the_direction_nibble() {
        let keys = [
            KeypadKey::Zero,
            KeypadKey::One,
            KeypadKey::Two,
            KeypadKey::Three,
            KeypadKey::Four,
            KeypadKey::Five,
            KeypadKey::Six,
            KeypadKey::Seven,
            KeypadKey::Eight,
            KeypadKey::Nine,
            KeypadKey::Star,
            KeypadKey::Pound,
        ];
        for (index, key) in keys.into_iter().enumerate() {
            let controller = controller_from_input(0, ((index as u8 + 1) << 4) | 0x05);
            assert_eq!(controller.keypad, Some(key));
            assert!(controller.right);
            assert!(controller.up);
        }
    }
}
