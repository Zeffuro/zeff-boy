use anyhow::bail;
use zeff_emu_common::save_ram::SaveRamKind;

use super::{SAVE_STATE_FORMAT_VERSION, SAVE_STATE_MAGIC, decode_state};
use crate::emulator::Emulator;
use crate::hardware::cartridge::{Sega8MapperKind, Sega8System};
use crate::hardware::input::ControllerPort;
use crate::hardware::region::Sega8Region;
use crate::hardware::serial::GameGearSerialDebugSnapshot;
use crate::hardware::timing::Sega8VideoStandard;

pub const GAME_GEAR_TAS_DETERMINISM_ABI_ID: &str = "zeff-game-gear-determinism-v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentNativeGameGearTasStateProjection {
    pub replay_state_bytes: Vec<u8>,
    pub frame_count: u64,
    pub framebuffer: Box<[u8]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentNativeGameGearTasStateInspection {
    pub projection: CurrentNativeGameGearTasStateProjection,
    pub rom_sha256: [u8; 32],
    pub mapper_kind: Sega8MapperKind,
    pub save_ram_kind: SaveRamKind,
    pub video_standard: Sega8VideoStandard,
    pub console_region: Sega8Region,
    pub boot_rom_enabled: bool,
    pub controller_raw: [u8; 2],
    pub start_pressed: bool,
    pub serial: GameGearSerialDebugSnapshot,
}

pub fn inspect_current_native_game_gear_tas_state(
    emulator: &Emulator,
    data: &[u8],
) -> anyhow::Result<CurrentNativeGameGearTasStateInspection> {
    if data.len() < 12 || data[..8] != SAVE_STATE_MAGIC {
        bail!("TAS requires a native Sega 8-bit save-state");
    }
    let version = u32::from_le_bytes(data[8..12].try_into().expect("length checked"));
    if version != SAVE_STATE_FORMAT_VERSION {
        bail!("TAS requires Sega 8-bit save-state format {SAVE_STATE_FORMAT_VERSION}");
    }
    if emulator.system() != Sega8System::GameGear {
        bail!("TAS state requires a Game Gear emulator");
    }

    let mut candidate = emulator.clone();
    decode_state(&mut candidate, data)?;
    if candidate.system() != Sega8System::GameGear {
        bail!("TAS state did not restore a Game Gear machine");
    }

    Ok(CurrentNativeGameGearTasStateInspection {
        projection: CurrentNativeGameGearTasStateProjection {
            replay_state_bytes: data.to_vec(),
            frame_count: candidate.frame_count(),
            framebuffer: candidate.framebuffer().into(),
        },
        rom_sha256: candidate.rom_hash(),
        mapper_kind: candidate.bus().mapper().kind(),
        save_ram_kind: candidate.save_ram_kind(),
        video_standard: candidate.video_standard(),
        console_region: candidate.console_region(),
        boot_rom_enabled: candidate.bus().boot_rom_enabled(),
        controller_raw: [
            candidate.bus().input().read_controller(ControllerPort::One),
            candidate.bus().input().read_controller(ControllerPort::Two),
        ],
        start_pressed: candidate.bus().input().game_gear_start_pressed(),
        serial: candidate.bus().game_gear_serial().debug_snapshot(),
    })
}

pub fn validate_and_load_current_native_game_gear_tas_state(
    emulator: &mut Emulator,
    data: &[u8],
) -> anyhow::Result<CurrentNativeGameGearTasStateProjection> {
    let inspection = inspect_current_native_game_gear_tas_state(emulator, data)?;
    emulator.load_state(data)?;
    Ok(inspection.projection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emulator::{DEFAULT_SAMPLE_RATE, Sega8LoadConfig};
    use crate::hardware::cartridge::SystemHint;
    use crate::hardware::constants::{IO_PORT_GG_SERIAL_CONTROL, IO_PORT_GG_SERIAL_TX};

    fn game_gear(mapper_kind: Sega8MapperKind, rom: &[u8]) -> Emulator {
        Emulator::new_with_config(
            rom,
            Sega8LoadConfig::new(DEFAULT_SAMPLE_RATE)
                .with_system_hint(SystemHint::GameGear)
                .with_mapper_kind(Some(mapper_kind)),
        )
        .unwrap()
    }

    #[test]
    fn current_game_gear_state_restores_exact_output_and_continuation() {
        let rom = vec![0x76; 64 * 1024];
        let mut source = game_gear(Sega8MapperKind::Korean, &rom);
        source.set_console_region(Sega8Region::Japanese);
        source.set_input(0x09, 0x06);
        source.bus_mut().io_write(IO_PORT_GG_SERIAL_CONTROL, 0x30);
        source.bus_mut().io_write(IO_PORT_GG_SERIAL_TX, 0xA5);
        for _ in 0..3 {
            source.step_frame();
        }
        let state = super::super::encode_state(&source).unwrap();
        let mut restored = game_gear(Sega8MapperKind::Korean, &rom);

        let inspection = inspect_current_native_game_gear_tas_state(&restored, &state).unwrap();
        assert_eq!(inspection.rom_sha256, source.rom_hash());
        assert_eq!(inspection.mapper_kind, Sega8MapperKind::Korean);
        assert_eq!(inspection.save_ram_kind, SaveRamKind::None);
        assert_eq!(inspection.video_standard, Sega8VideoStandard::Ntsc);
        assert_eq!(inspection.console_region, Sega8Region::Japanese);
        assert!(!inspection.boot_rom_enabled);
        assert_eq!(inspection.controller_raw, [0xEA, 0xFF]);
        assert!(inspection.start_pressed);
        assert_eq!(
            inspection.serial,
            source.bus().game_gear_serial().debug_snapshot()
        );
        assert!(!inspection.serial.peer_present);
        assert_eq!(restored.frame_count(), 0);

        let projection =
            validate_and_load_current_native_game_gear_tas_state(&mut restored, &state).unwrap();
        assert_eq!(projection.replay_state_bytes, state);
        assert_eq!(projection.frame_count, source.frame_count());
        assert_eq!(projection.framebuffer.as_ref(), source.framebuffer());
        assert_eq!(super::super::encode_state(&restored).unwrap(), state);

        source.step_frame();
        restored.step_frame();
        assert_eq!(restored.framebuffer(), source.framebuffer());
        assert_eq!(
            super::super::encode_state(&restored).unwrap(),
            super::super::encode_state(&source).unwrap()
        );
    }

    #[test]
    fn current_game_gear_state_rejects_wrong_identity_atomically() {
        let rom = vec![0x76; 64 * 1024];
        let source = game_gear(Sega8MapperKind::Korean, &rom);
        let current = super::super::encode_state(&source).unwrap();
        let mut target = game_gear(Sega8MapperKind::Korean, &rom);
        target.step_frame();
        let before = super::super::encode_state(&target).unwrap();

        let mut legacy = current.clone();
        legacy[8..12].copy_from_slice(&(SAVE_STATE_FORMAT_VERSION - 1).to_le_bytes());
        let mut trailing = current.clone();
        trailing.push(0);
        let truncated = &current[..current.len() - 1];
        for invalid in [&legacy[..], &trailing, truncated] {
            assert!(
                validate_and_load_current_native_game_gear_tas_state(&mut target, invalid).is_err()
            );
            assert_eq!(super::super::encode_state(&target).unwrap(), before);
        }

        let wrong_mapper =
            super::super::encode_state(&game_gear(Sega8MapperKind::Sega, &rom)).unwrap();
        assert!(
            validate_and_load_current_native_game_gear_tas_state(&mut target, &wrong_mapper)
                .is_err()
        );
        assert_eq!(super::super::encode_state(&target).unwrap(), before);

        let mut other_rom = rom.clone();
        other_rom[0] ^= 0xFF;
        let wrong_rom =
            super::super::encode_state(&game_gear(Sega8MapperKind::Korean, &other_rom)).unwrap();
        assert!(
            validate_and_load_current_native_game_gear_tas_state(&mut target, &wrong_rom).is_err()
        );
        assert_eq!(super::super::encode_state(&target).unwrap(), before);

        let mut sms =
            Emulator::new_with_hint(&rom, DEFAULT_SAMPLE_RATE, SystemHint::MasterSystem).unwrap();
        let sms_before = super::super::encode_state(&sms).unwrap();
        assert!(validate_and_load_current_native_game_gear_tas_state(&mut sms, &current).is_err());
        assert_eq!(super::super::encode_state(&sms).unwrap(), sms_before);
    }
}
