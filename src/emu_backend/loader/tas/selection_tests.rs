use std::collections::BTreeMap;

use zeff_emu_common::replay::ReplayStartMetadata;

use super::*;
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasControllerInput, TasEditorSession, TasInitialBranch,
    TasInputFrame, TasSeekStateCache,
};

#[test]
fn private_attachment_selection_reports_current_capability_reasons() {
    assert!(matches!(
        select_private_tas_execution_attachment(None, None, None, Vec::new(), None),
        TasEditorExecutionAttachment::Unavailable(
            TasEditorExecutionUnavailableReason::NoRunningEmulator
        )
    ));
    for (path, system) in [
        ("game.gb", ActiveSystem::GameBoy),
        ("game.gbc", ActiveSystem::GameBoy),
        ("game.nes", ActiveSystem::Nes),
    ] {
        assert!(matches!(
            select_private_tas_execution_attachment(
                Some(PathBuf::from(path)),
                None,
                Some(system),
                Vec::new(),
                None,
            ),
            TasEditorExecutionAttachment::Available(_)
        ));
    }
    for system in [ActiveSystem::Nes, ActiveSystem::GameBoy] {
        assert!(matches!(
            select_private_tas_execution_attachment(
                Some(PathBuf::from("game.zip")),
                None,
                Some(system),
                Vec::new(),
                None,
            ),
            TasEditorExecutionAttachment::Unavailable(
                TasEditorExecutionUnavailableReason::UnsupportedMedia(_)
            )
        ));
    }
}

#[test]
fn attached_direct_nes_profile_is_rechecked_after_a_later_edit() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-loader-persistent-scope")?;
    let rom_path = directory.path().join("game.nes");
    let rom = crate::test_support::build_nes_test_rom();
    std::fs::write(&rom_path, &rom)?;
    let backend = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &rom_path,
        &rom_path,
        Some(rom.clone()),
        BackendLoadConfig {
            sample_rate: None,
            apply_mods: false,
            initial_input: None,
            nes_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )?
    .backend;
    let start_state = backend.encode_state_bytes()?;
    let identity = direct_nes_tas_identity(&backend, &rom, &start_state)?;
    let project = TasProject::new(
        "persistent-direct-nes-scope",
        identity,
        start_state,
        ReplayStartMetadata::default(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count: 1,
            input_spans: Vec::new(),
            events: Vec::new(),
        },
        BTreeMap::new(),
    )?;
    let loader = DirectNesTasExecutionLoader::new(rom_path, Vec::new());
    let mut engine = loader.load_editor_engine(&project)?;
    let manual_path = directory.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let seek_cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project, manual_path, autosaves, seek_cache)?;

    editor.edit_transaction(|edit| {
        edit.set_input_range(
            "main",
            0,
            1,
            TasInputFrame {
                players: [
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                    TasControllerInput {
                        buttons: 1,
                        dpad: 0,
                    },
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                ],
                ..TasInputFrame::default()
            },
        )
    })?;

    let error = engine.seek(&mut editor, 1).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("attached editor execution profile")
    );
    assert_eq!(editor.cursor(), 0);
    assert!(editor.load_seek_state()?.is_none());
    Ok(())
}
