use std::collections::BTreeMap;
use std::path::PathBuf;

use zeff_emu_common::replay::ReplayStartMetadata;
use zeff_pce_core::hardware::{
    PceArcadeCardMode, PceCartridgeHardware, PceConsoleWiring, PceControllerMode, PceHuCardBoard,
    PceMemoryBaseMode,
};

use super::*;
use crate::emu_backend::loader::{BackendLoadConfig, load_backend_from_bounded_direct_source};
use crate::tas_project::{TasControllerInput, TasInitialBranch, TasInputFrame, TasInputSpan};

fn raw_rom() -> Vec<u8> {
    let mut raw = vec![0; zeff_pce_core::hardware::PCEAS_HEADER_LEN];
    raw[0] = 1;
    raw.extend(vec![0xEA; 0x2000]);
    raw
}

fn direct_backend() -> EmuBackend {
    load_backend_from_bounded_direct_source(
        ActiveSystem::Pce,
        &PathBuf::from("synthetic.pce"),
        raw_rom(),
        BackendLoadConfig {
            sample_rate: Some(48_000),
            pce_console_wiring: Some(PceConsoleWiring::PcEngine),
            pce_hucard_board: Some(PceHuCardBoard::Plain),
            pce_cartridge_hardware: Some(PceCartridgeHardware::Base),
            pce_controller_mode: PceControllerMode::TwoButton,
            pce_memory_base_mode: PceMemoryBaseMode::Disabled,
            pce_arcade_card_mode: PceArcadeCardMode::Disabled,
            pce_load_battery_bram: false,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend
}

#[test]
fn runtime_binds_raw_normalized_hardware_and_state() {
    let mut backend = direct_backend();
    let inspection = validate_direct_pce_tas_runtime(&backend, false).unwrap();
    assert_ne!(
        TasDigest::from_bytes(&raw_rom()),
        TasDigest(inspection.normalized_rom_sha256)
    );
    let state = backend.encode_state_bytes().unwrap();
    backend.step_frame();
    let projection = validate_direct_pce_tas_state(&mut backend, &state).unwrap();
    assert_eq!(projection.frame_count, 0);
    assert_eq!(backend.frame_count(), 0);
    assert_eq!(
        projection.framebuffer.as_ref(),
        backend.pce().unwrap().tas_core_framebuffer()
    );
    assert!(backend.pce().unwrap().tas_presented_frame_is_current());
}

#[test]
fn runtime_rejects_rate_input_and_cheats() {
    let mut rate = direct_backend();
    rate.set_sample_rate(44_100);
    assert!(validate_direct_pce_tas_execution_runtime(&rate, false).is_err());

    let mut input = direct_backend();
    input.set_input(1, 0);
    assert!(validate_direct_pce_tas_runtime(&input, false).is_err());
    assert!(validate_direct_pce_tas_runtime(&direct_backend(), true).is_err());

    let mut display = direct_backend();
    let EmuBackend::Pce(pce) = &mut display else {
        unreachable!();
    };
    pce.set_display_config(
        crate::settings::PceOverscanMode::Conservative,
        crate::settings::PcePaletteMode::RawRgb,
    );
    assert!(validate_direct_pce_tas_execution_runtime(&display, false).is_err());
}

#[test]
fn identity_and_scope_allow_only_one_two_button_pad() {
    let backend = direct_backend();
    let state = backend.encode_state_bytes().unwrap();
    let identity = direct_pce_tas_identity(&backend, &raw_rom(), &state).unwrap();
    let project = TasProject::new(
        "pce-test".to_owned(),
        identity,
        state,
        ReplayStartMetadata::default(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count: 1,
            input_spans: vec![TasInputSpan {
                start: 0,
                length: 1,
                input: TasInputFrame {
                    players: [
                        TasControllerInput {
                            buttons: 0x09,
                            dpad: 0x04,
                        },
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                    ],
                    ..TasInputFrame::default()
                },
            }],
            events: Vec::new(),
        },
        BTreeMap::new(),
    )
    .unwrap();
    validate_direct_pce_tas_project_identity(&project).unwrap();
    validate_direct_pce_tas_branch_scope(&project, "main").unwrap();
}
