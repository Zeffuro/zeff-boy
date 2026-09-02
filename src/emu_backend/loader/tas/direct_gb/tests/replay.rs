use super::*;

#[test]
fn replay_export_import_uses_two_fresh_direct_gb_passes() -> Result<()> {
    let (directory, source_path, _) = write_direct_rom("tas-direct-gb-replay")?;
    let loader = DirectGbTasExecutionLoader::new(source_path, Vec::new());
    let mut project = loader.create_project()?;
    let input = TasInputFrame {
        players: [
            TasControllerInput {
                buttons: 0b0011,
                dpad: 0b0101,
            },
            TasControllerInput::default(),
            TasControllerInput::default(),
            TasControllerInput::default(),
            TasControllerInput::default(),
        ],
        ..TasInputFrame::default()
    };
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;

    let manual_path = directory.path().join("source.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("replay-cache"))?;
    let mut editor = TasEditorSession::new(project, manual_path, autosaves, cache)?;
    let plan = PrivateTasExecutionLoader::DirectGb(loader);
    let replay_path = directory.path().join("movie.zrpl");
    plan.verify_and_export_editor_session(&mut editor, &replay_path)?;
    assert!(editor.project().verification_is_current("main")?);

    let imported_path = directory.path().join("imported.ztas");
    let imported = plan.import_replay_file(&replay_path, &imported_path, false)?;
    assert_eq!(imported.branch("main").unwrap().input_at(0), input);
    assert_eq!(
        classify_direct_tas_execution_profile(&imported)?,
        crate::emu_thread::TasExecutionProfile::DirectGbCartridgeDmg
    );
    Ok(())
}
