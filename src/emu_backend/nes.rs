use std::path::{Path, PathBuf};

use crate::audio_recorder::MidiApuSnapshot;

pub(super) fn drain_audio_samples(emu: &mut zeff_nes_core::emulator::Emulator) -> Vec<f32> {
    let mono = emu.drain_audio_samples();
    let mut stereo = Vec::with_capacity(mono.len() * 2);
    for &sample in &mono {
        stereo.push(sample);
        stereo.push(sample);
    }
    stereo
}

pub(super) fn drain_audio_samples_into(
    emu: &mut zeff_nes_core::emulator::Emulator,
    buf: &mut Vec<f32>,
) {
    let mono = emu.drain_audio_samples();
    buf.clear();
    buf.reserve(mono.len() * 2);
    for &sample in &mono {
        buf.push(sample);
        buf.push(sample);
    }
}

pub(super) fn set_sample_rate(emu: &mut zeff_nes_core::emulator::Emulator, rate: u32) {
    emu.bus.apu.output_sample_rate = rate as f64;
}

pub(super) fn set_apu_sample_generation_enabled(
    _emu: &mut zeff_nes_core::emulator::Emulator,
    _enabled: bool,
) {
    // NES APU always generates samples.
}

pub(super) fn set_apu_channel_mutes(
    emu: &mut zeff_nes_core::emulator::Emulator,
    mutes: [bool; 4],
) {
    emu.bus.apu.set_channel_mutes(mutes);
}

pub(super) fn set_input(
    emu: &mut zeff_nes_core::emulator::Emulator,
    buttons_pressed: u8,
    dpad_pressed: u8,
) {
    let mut nes_byte = 0u8;
    if buttons_pressed & 0x01 != 0 {
        nes_byte |= 0x01;
    }
    if buttons_pressed & 0x02 != 0 {
        nes_byte |= 0x02;
    }
    if buttons_pressed & 0x04 != 0 {
        nes_byte |= 0x04;
    }
    if buttons_pressed & 0x08 != 0 {
        nes_byte |= 0x08;
    }
    if dpad_pressed & 0x04 != 0 {
        nes_byte |= 0x10;
    }
    if dpad_pressed & 0x08 != 0 {
        nes_byte |= 0x20;
    }
    if dpad_pressed & 0x02 != 0 {
        nes_byte |= 0x40;
    }
    if dpad_pressed & 0x01 != 0 {
        nes_byte |= 0x80;
    }
    emu.bus.controller1.set_buttons(nes_byte);
}

pub(super) fn flush_battery_sram(
    emu: &mut zeff_nes_core::emulator::Emulator,
) -> anyhow::Result<Option<String>> {
    emu.flush_battery_sram()
}

pub(super) fn encode_state_bytes(
    emu: &zeff_nes_core::emulator::Emulator,
) -> anyhow::Result<Vec<u8>> {
    emu.encode_state()
}

pub(super) fn load_state_from_bytes(
    emu: &mut zeff_nes_core::emulator::Emulator,
    bytes: Vec<u8>,
) -> anyhow::Result<()> {
    emu.load_state_from_bytes(bytes)
}

pub(super) fn slot_path(
    emu: &zeff_nes_core::emulator::Emulator,
    slot: u8,
) -> anyhow::Result<PathBuf> {
    zeff_nes_core::save_state::slot_path(emu.rom_hash, slot)
}

pub(super) fn auto_save_path(emu: &zeff_nes_core::emulator::Emulator) -> Option<PathBuf> {
    Some(zeff_nes_core::save_state::auto_save_path(emu.rom_hash))
}

pub(super) fn load_state(
    emu: &mut zeff_nes_core::emulator::Emulator,
    slot: u8,
) -> anyhow::Result<String> {
    emu.load_state_slot(slot)
}

pub(super) fn load_state_from_path(
    emu: &mut zeff_nes_core::emulator::Emulator,
    path: &Path,
) -> anyhow::Result<()> {
    emu.load_state_from_path(path)
}

pub(super) fn rumble_active(_emu: &zeff_nes_core::emulator::Emulator) -> bool {
    false
}

pub(super) fn is_mbc7(_emu: &zeff_nes_core::emulator::Emulator) -> bool {
    false
}

pub(super) fn apu_channel_snapshot(
    emu: &zeff_nes_core::emulator::Emulator,
) -> Option<MidiApuSnapshot> {
    Some(MidiApuSnapshot::Nes(emu.bus.apu.channel_snapshot()))
}

