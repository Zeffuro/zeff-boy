use super::*;

#[test]
fn worker_tas_input_preserves_zapper_state() {
    let input = TasInputFrame {
        zapper: zeff_emu_common::replay::ReplayZapperFrame {
            enabled: true,
            trigger: true,
            hit: false,
            screen_pos: Some((120, 80)),
        },
        ..TasInputFrame::default()
    };
    assert_eq!(replay_frame(input).zapper, input.zapper);
    assert!(validate_gb_input(input).is_err());
}

#[test]
fn coleco_worker_input_rejects_generic_pad_authority() {
    let input = TasInputFrame {
        p1_buttons: 1,
        coleco: [
            crate::tas_project::TasColecoControllerInput {
                keypad: crate::tas_project::TasColecoKeypadKey::Pound,
                ..Default::default()
            },
            Default::default(),
        ],
        ..TasInputFrame::default()
    };
    assert!(validate_coleco_input(input).is_err());
}

#[test]
fn coleco_worker_input_applies_both_semantic_controllers() {
    use std::path::PathBuf;

    let mut rom = vec![0; 8 * 1024];
    rom[..2].copy_from_slice(&[0xAA, 0x55]);
    static BIOS: [u8; zeff_coleco_core::constants::BIOS_SIZE] =
        [0; zeff_coleco_core::constants::BIOS_SIZE];
    let path = PathBuf::from("game.col");
    let load = || {
        crate::emu_backend::loader::load_backend_from_rom_source(
            ActiveSystem::Coleco,
            &path,
            &path,
            Some(rom.clone()),
            crate::emu_backend::loader::BackendLoadConfig {
                coleco_bios_override: Some(&BIOS),
                ..Default::default()
            },
        )
        .unwrap()
        .backend
    };
    let controllers = [
        crate::tas_project::TasColecoControllerInput {
            left: true,
            left_button: true,
            keypad: crate::tas_project::TasColecoKeypadKey::Star,
            ..Default::default()
        },
        crate::tas_project::TasColecoControllerInput {
            right: true,
            right_button: true,
            keypad: crate::tas_project::TasColecoKeypadKey::Pound,
            ..Default::default()
        },
    ];
    let mut actual = load();
    apply_coleco_input(
        &mut actual,
        TasInputFrame {
            coleco: controllers,
            ..Default::default()
        },
    )
    .unwrap();
    let mut expected = load();
    expected.apply_coleco_tas_input(controllers).unwrap();

    assert_eq!(
        actual.encode_state_bytes().unwrap(),
        expected.encode_state_bytes().unwrap()
    );
}
