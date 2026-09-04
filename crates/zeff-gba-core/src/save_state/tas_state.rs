use anyhow::{Result, ensure};
use zeff_emu_common::save_ram::SaveRamKind;

use super::{MAGIC, TILT_VERSION, VERSION, decode_state, encode_state};
use crate::emulator::Emulator;
use crate::hardware::cartridge::{RtcDateTime, RtcState, SensorKind, TiltState};

pub const TAS_DETERMINISM_ABI_ID: &str = "zeff-gba-tas-determinism-v2";
pub const TAS_STATE_FORMAT_COMPATIBILITY_ID: &str = "zeff-gba-native-state-v10";
pub const TILT_TAS_DETERMINISM_ABI_ID: &str = "zeff-gba-tilt-tas-determinism-v1";
pub const TILT_TAS_STATE_FORMAT_COMPATIBILITY_ID: &str = "zeff-gba-native-state-v11-tilt";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GbaTasKeypadState {
    pub buttons: u8,
    pub dpad: u8,
    pub keycnt: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GbaTasStartup {
    InternalPostBoot,
    ExternalBios,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentNativeGbaTasStateProjection {
    pub frame_count: u64,
    pub framebuffer: Box<[u8]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentNativeGbaTasStateInspection {
    pub projection: CurrentNativeGbaTasStateProjection,
    pub rom_sha256: [u8; 32],
    pub keypad: GbaTasKeypadState,
    pub save_ram_kind: SaveRamKind,
    pub battery_data: Option<Vec<u8>>,
    pub rtc_present: bool,
    pub rtc_date_time: Option<RtcDateTime>,
    pub rtc_state: Option<RtcState>,
    pub rtc_persistence_state: Option<Vec<u8>>,
    pub complete_rtc_persistence: Option<Vec<u8>>,
    pub sensor_kind: SensorKind,
    pub tilt_state: Option<TiltState>,
    pub external_bios: bool,
    pub executing_in_bios: bool,
    pub sample_rate: u32,
    pub startup: GbaTasStartup,
}

pub fn inspect_current_native_gba_tas_state(
    emu: &Emulator,
    data: &[u8],
) -> Result<CurrentNativeGbaTasStateInspection> {
    inspect_native_gba_tas_state(emu, data, VERSION)
}

pub fn inspect_current_native_gba_tilt_tas_state(
    emu: &Emulator,
    data: &[u8],
) -> Result<CurrentNativeGbaTasStateInspection> {
    inspect_native_gba_tas_state(emu, data, TILT_VERSION)
}

fn inspect_native_gba_tas_state(
    emu: &Emulator,
    data: &[u8],
    expected_version: u32,
) -> Result<CurrentNativeGbaTasStateInspection> {
    ensure!(
        data.len() >= 12 && data[..8] == *MAGIC,
        "GBA TAS requires a native save state"
    );
    let version = u32::from_le_bytes(data[8..12].try_into().expect("length checked"));
    ensure!(
        version == expected_version,
        "GBA TAS requires current native v{expected_version} state"
    );

    let mut candidate = emu.clone();
    decode_state(&mut candidate, data)?;
    ensure!(
        encode_state(&candidate)? == data,
        "GBA TAS state is not canonical current-native data"
    );

    let keyinput = candidate.bus.keypad.read_keyinput();
    let pressed = !keyinput & 0x03FF;
    Ok(CurrentNativeGbaTasStateInspection {
        projection: CurrentNativeGbaTasStateProjection {
            frame_count: candidate.frame_count,
            framebuffer: candidate.framebuffer().into(),
        },
        rom_sha256: candidate.rom_hash,
        keypad: GbaTasKeypadState {
            buttons: ((pressed & 0x000F) as u8) | (((pressed >> 8) as u8 & 0x03) << 4),
            dpad: (pressed >> 4) as u8 & 0x0F,
            keycnt: candidate.bus.keypad.read_keycnt(),
        },
        save_ram_kind: candidate.bus.cartridge.save_ram_kind(),
        battery_data: candidate.bus.cartridge.dump_battery_data(),
        rtc_present: candidate.bus.cartridge.has_rtc(),
        rtc_date_time: candidate.bus.cartridge.rtc_date_time(),
        rtc_state: candidate.bus.cartridge.rtc_state(),
        rtc_persistence_state: candidate.bus.cartridge.dump_rtc_persistence_state(),
        complete_rtc_persistence: candidate.bus.cartridge.dump_complete_rtc_persistence(),
        sensor_kind: candidate.bus.cartridge.sensor_kind(),
        tilt_state: candidate.bus.cartridge.tilt_state(),
        external_bios: candidate.bus.has_external_bios(),
        executing_in_bios: candidate.cpu.pc() <= 0x3FFF,
        sample_rate: candidate.bus.apu.sample_rate(),
        startup: if candidate.bus.has_external_bios() {
            GbaTasStartup::ExternalBios
        } else {
            GbaTasStartup::InternalPostBoot
        },
    })
}

pub fn restore_current_native_gba_tas_state(
    emu: &mut Emulator,
    data: &[u8],
) -> Result<CurrentNativeGbaTasStateInspection> {
    restore_native_gba_tas_state(emu, data, VERSION)
}

pub fn restore_current_native_gba_tilt_tas_state(
    emu: &mut Emulator,
    data: &[u8],
) -> Result<CurrentNativeGbaTasStateInspection> {
    restore_native_gba_tas_state(emu, data, TILT_VERSION)
}

fn restore_native_gba_tas_state(
    emu: &mut Emulator,
    data: &[u8],
    expected_version: u32,
) -> Result<CurrentNativeGbaTasStateInspection> {
    let inspection = inspect_native_gba_tas_state(emu, data, expected_version)?;
    let mut candidate = emu.clone();
    decode_state(&mut candidate, data)?;
    candidate.bus.apu.clear_host_output_after_state_load();
    candidate.opcode_log.clear();
    candidate.instruction_trace.clear();
    *emu = candidate;
    Ok(inspection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save_state::{VERSION_9_ROM_HASH_SIZE, VERSION_10_BACKUP_EXECUTION_STATE_SIZE};

    fn rom(marker: u8) -> Vec<u8> {
        let mut rom = vec![0; 0xC0];
        rom[0xA0..0xA4].copy_from_slice(b"TEST");
        rom[0xB2] = 0x96;
        rom[0] = marker;
        rom
    }

    #[test]
    fn inspects_current_state_with_rom_keypad_and_runtime_facts() {
        let rom = rom(0x21);
        let mut emu = Emulator::new(&rom, 48_000).unwrap();
        emu.set_input(0x31, 0x06);
        emu.step_frame();
        let state = encode_state(&emu).unwrap();

        let inspection = inspect_current_native_gba_tas_state(&emu, &state).unwrap();
        assert_eq!(inspection.rom_sha256, emu.rom_hash());
        assert_eq!(inspection.projection.frame_count, 1);
        assert_eq!(inspection.keypad.buttons, 0x31);
        assert_eq!(inspection.keypad.dpad, 0x06);
        assert_eq!(inspection.save_ram_kind, SaveRamKind::none());
        assert!(!inspection.rtc_present);
        assert!(!inspection.external_bios);
        assert_eq!(inspection.startup, GbaTasStartup::InternalPostBoot);
        assert_eq!(inspection.sample_rate, 48_000);
    }

    #[test]
    fn rejects_legacy_wrong_rom_and_malformed_state_without_mutating_target() {
        let source_rom = rom(0x21);
        let target_rom = rom(0x22);
        let source = Emulator::new(&source_rom, 48_000).unwrap();
        let state = encode_state(&source).unwrap();
        let mut target = Emulator::new(&target_rom, 48_000).unwrap();
        let before = encode_state(&target).unwrap();

        assert!(restore_current_native_gba_tas_state(&mut target, &state).is_err());
        assert_eq!(encode_state(&target).unwrap(), before);

        let legacy_len =
            state.len() - VERSION_10_BACKUP_EXECUTION_STATE_SIZE - VERSION_9_ROM_HASH_SIZE;
        let mut legacy = state[..legacy_len].to_vec();
        legacy[8..12].copy_from_slice(&8u32.to_le_bytes());
        assert!(inspect_current_native_gba_tas_state(&source, &legacy).is_err());
        let mut legacy_target = Emulator::new(&source_rom, 48_000).unwrap();
        legacy_target.load_state(&legacy).unwrap();

        let mut malformed = state;
        malformed.pop();
        assert!(restore_current_native_gba_tas_state(&mut target, &malformed).is_err());
        assert_eq!(encode_state(&target).unwrap(), before);
    }
}
