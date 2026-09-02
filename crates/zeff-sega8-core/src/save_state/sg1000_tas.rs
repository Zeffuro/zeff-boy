use anyhow::bail;
use zeff_emu_common::save_ram::SaveRamKind;

use super::{SAVE_STATE_FORMAT_VERSION, SAVE_STATE_MAGIC, decode_state};
use crate::emulator::Emulator;
use crate::hardware::cartridge::{Sega8MapperKind, Sega8System};
use crate::hardware::input::ControllerPort;
use crate::hardware::region::Sega8Region;
use crate::hardware::timing::Sega8VideoStandard;

pub const SG1000_TAS_DETERMINISM_ABI_ID: &str = "zeff-sg1000-determinism-v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentNativeSg1000TasStateProjection {
    pub replay_state_bytes: Vec<u8>,
    pub frame_count: u64,
    pub framebuffer: Box<[u8]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentNativeSg1000TasStateInspection {
    pub projection: CurrentNativeSg1000TasStateProjection,
    pub rom_sha256: [u8; 32],
    pub mapper_kind: Sega8MapperKind,
    pub save_ram_kind: SaveRamKind,
    pub video_standard: Sega8VideoStandard,
    pub console_region: Sega8Region,
    pub boot_rom_enabled: bool,
    pub controller_raw: [u8; 2],
    pub type_b_ram_extension: bool,
}

pub fn inspect_current_native_sg1000_tas_state(
    emulator: &Emulator,
    data: &[u8],
) -> anyhow::Result<CurrentNativeSg1000TasStateInspection> {
    if data.len() < 12 || data[..8] != SAVE_STATE_MAGIC {
        bail!("TAS requires a native Sega 8-bit save-state");
    }
    let version = u32::from_le_bytes(data[8..12].try_into().expect("length checked"));
    if version != SAVE_STATE_FORMAT_VERSION {
        bail!("TAS requires Sega 8-bit save-state format {SAVE_STATE_FORMAT_VERSION}");
    }
    if emulator.system() != Sega8System::Sg1000 {
        bail!("TAS state requires an SG-1000 emulator");
    }

    let mut candidate = emulator.clone();
    decode_state(&mut candidate, data)?;
    if candidate.system() != Sega8System::Sg1000 {
        bail!("TAS state did not restore an SG-1000 machine");
    }

    Ok(CurrentNativeSg1000TasStateInspection {
        projection: CurrentNativeSg1000TasStateProjection {
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
        type_b_ram_extension: candidate.bus().sg_type_b_ram_extension,
    })
}

pub fn validate_and_load_current_native_sg1000_tas_state(
    emulator: &mut Emulator,
    data: &[u8],
) -> anyhow::Result<CurrentNativeSg1000TasStateProjection> {
    let inspection = inspect_current_native_sg1000_tas_state(emulator, data)?;
    emulator.load_state(data)?;
    Ok(inspection.projection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emulator::{DEFAULT_SAMPLE_RATE, Sega8LoadConfig};
    use crate::hardware::cartridge::SystemHint;

    fn sg1000(mapper_kind: Sega8MapperKind, rom: &[u8]) -> Emulator {
        Emulator::new_with_config(
            rom,
            Sega8LoadConfig::new(DEFAULT_SAMPLE_RATE)
                .with_system_hint(SystemHint::Sg1000)
                .with_mapper_kind(Some(mapper_kind))
                .with_video_standard(Sega8VideoStandard::Ntsc)
                .with_console_region(Some(Sega8Region::Japanese)),
        )
        .unwrap()
    }

    #[test]
    fn current_sg1000_state_restores_exact_output_and_continuation() {
        let rom = vec![0x76; 32 * 1024];
        let mut source = sg1000(Sega8MapperKind::Sega, &rom);
        source.set_input(0x01, 0x04);
        source.set_input_p2(0x02, 0x08);
        for _ in 0..3 {
            source.step_frame();
        }
        let state = super::super::encode_state(&source).unwrap();
        let mut restored = sg1000(Sega8MapperKind::Sega, &rom);

        let inspection = inspect_current_native_sg1000_tas_state(&restored, &state).unwrap();
        assert_eq!(inspection.rom_sha256, source.rom_hash());
        assert_eq!(inspection.mapper_kind, Sega8MapperKind::Sega);
        assert_eq!(inspection.save_ram_kind, SaveRamKind::None);
        assert_eq!(inspection.video_standard, Sega8VideoStandard::Ntsc);
        assert_eq!(inspection.console_region, Sega8Region::Japanese);
        assert!(!inspection.boot_rom_enabled);
        assert_eq!(inspection.controller_raw, [0xEE, 0xDD]);
        assert!(!inspection.type_b_ram_extension);
        assert_eq!(restored.frame_count(), 0);

        let projection =
            validate_and_load_current_native_sg1000_tas_state(&mut restored, &state).unwrap();
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
    fn current_sg1000_state_rejects_wrong_identity_atomically() {
        let rom = vec![0x76; 32 * 1024];
        let source = sg1000(Sega8MapperKind::Sega, &rom);
        let current = super::super::encode_state(&source).unwrap();
        let mut target = sg1000(Sega8MapperKind::Sega, &rom);
        target.step_frame();
        let before = super::super::encode_state(&target).unwrap();

        let mut legacy = current.clone();
        legacy[8..12].copy_from_slice(&(SAVE_STATE_FORMAT_VERSION - 1).to_le_bytes());
        let mut trailing = current.clone();
        trailing.push(0);
        let truncated = &current[..current.len() - 1];
        for invalid in [&legacy[..], &trailing, truncated] {
            assert!(
                validate_and_load_current_native_sg1000_tas_state(&mut target, invalid).is_err()
            );
            assert_eq!(super::super::encode_state(&target).unwrap(), before);
        }

        let wrong_mapper =
            super::super::encode_state(&sg1000(Sega8MapperKind::Korean, &rom)).unwrap();
        assert!(
            validate_and_load_current_native_sg1000_tas_state(&mut target, &wrong_mapper).is_err()
        );
        assert_eq!(super::super::encode_state(&target).unwrap(), before);

        let mut sms =
            Emulator::new_with_hint(&rom, DEFAULT_SAMPLE_RATE, SystemHint::MasterSystem).unwrap();
        let sms_before = super::super::encode_state(&sms).unwrap();
        assert!(validate_and_load_current_native_sg1000_tas_state(&mut sms, &current).is_err());
        assert_eq!(super::super::encode_state(&sms).unwrap(), sms_before);
    }
}
