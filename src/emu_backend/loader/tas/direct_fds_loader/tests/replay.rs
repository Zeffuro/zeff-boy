use super::*;
use crate::emu_backend::loader::PrivateTasExecutionLoader;
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasEditorSession, TasSeekStateCache,
};

fn export_replay(
    project: TasProject,
    loader: DirectFdsTasExecutionLoader,
    directory: &Path,
) -> Result<PathBuf> {
    let manual_path = directory.join("source.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.join("replay-cache"))?;
    let mut editor = TasEditorSession::new(project, manual_path, autosaves, cache)?;
    let replay_path = directory.join("movie.zrpl");
    PrivateTasExecutionLoader::DirectFds(loader)
        .verify_and_export_editor_session(&mut editor, &replay_path)?;
    Ok(replay_path)
}

#[test]
fn direct_fds_replay_import_restores_the_owned_disk_asset() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-fds-replay-import")?;
    let disk_path = directory.path().join("game.fds");
    let disk_bytes = disk(3);
    std::fs::write(&disk_path, &disk_bytes)?;
    let creation =
        DirectFdsTasExecutionLoader::new_with_bios_override(disk_path.clone(), &FDS_BIOS);
    let mut project = creation.create_project()?;
    project.edit_transaction(|edit| {
        edit.replace_branch_events("main", vec![ReplayEvent::FdsDiskSide { frame: 0, side: 2 }])
    })?;
    let execution =
        DirectFdsTasExecutionLoader::new_for_project(disk_path.clone(), Vec::new(), &project)?
            .with_project_bios_override(&FDS_BIOS);
    let replay_path = export_replay(project, execution, directory.path())?;

    let project_path = directory.path().join("imported.ztas");
    let plan = PrivateTasExecutionLoader::DirectFds(creation);
    let imported = plan.import_replay_file(&replay_path, &project_path, false)?;
    assert_eq!(fds_project_disk_bytes(&imported)?, disk_bytes);
    assert!(matches!(
        imported.branch("main").unwrap().events(),
        [ReplayEvent::FdsDiskSide { frame: 0, side: 2 }]
    ));
    plan.validate_project_branch_scope(&imported, "main")?;

    let wrong_bios_path = directory.path().join("wrong-bios.ztas");
    assert!(
        PrivateTasExecutionLoader::DirectFds(DirectFdsTasExecutionLoader::new_with_bios_override(
            disk_path,
            &OTHER_FDS_BIOS
        ))
        .import_replay_file(&replay_path, &wrong_bios_path, false)
        .is_err()
    );
    assert!(!wrong_bios_path.exists());
    Ok(())
}

#[test]
fn fds_replay_import_rejects_changed_source_without_publication() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-fds-replay-mutation")?;
    let disk_path = directory.path().join("game.fds");
    std::fs::write(&disk_path, disk(2))?;
    let creation =
        DirectFdsTasExecutionLoader::new_with_bios_override(disk_path.clone(), &FDS_BIOS);
    let project = creation.create_project()?;
    let execution =
        DirectFdsTasExecutionLoader::new_for_project(disk_path.clone(), Vec::new(), &project)?
            .with_project_bios_override(&FDS_BIOS);
    let replay_path = export_replay(project, execution, directory.path())?;

    std::fs::write(&disk_path, disk(1))?;
    let project_path = directory.path().join("rejected.ztas");
    assert!(
        PrivateTasExecutionLoader::DirectFds(DirectFdsTasExecutionLoader::new_with_bios_override(
            disk_path, &FDS_BIOS
        ))
        .import_replay_file(&replay_path, &project_path, false)
        .is_err()
    );
    assert!(!project_path.exists());
    Ok(())
}

#[test]
fn selected_zip_fds_replay_import_restores_the_selected_asset() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-fds-zip-replay-import")?;
    let archive_path = directory.path().join("games.zip");
    let disk_bytes = disk(5);
    write_zip(&archive_path, &[("set/game.fds", &disk_bytes)])?;
    let selected_path = archive_path.join("set/game.fds");
    let creation = DirectFdsTasExecutionLoader::new_zip_with_bios_override(
        archive_path.clone(),
        Some(selected_path.clone()),
        &FDS_BIOS,
    );
    let mut project = creation.create_project()?;
    project.edit_transaction(|edit| {
        edit.replace_branch_events("main", vec![ReplayEvent::FdsDiskSide { frame: 0, side: 4 }])
    })?;
    let execution =
        DirectFdsTasExecutionLoader::new_for_project(archive_path.clone(), Vec::new(), &project)?
            .with_project_bios_override(&FDS_BIOS);
    let replay_path = export_replay(project, execution, directory.path())?;

    let project_path = directory.path().join("imported-zip.ztas");
    let plan = PrivateTasExecutionLoader::DirectFds(
        DirectFdsTasExecutionLoader::new_zip_with_bios_override(
            archive_path,
            Some(selected_path),
            &FDS_BIOS,
        ),
    );
    let imported = plan.import_replay_file(&replay_path, &project_path, false)?;
    assert_eq!(fds_project_disk_bytes(&imported)?, disk_bytes);
    plan.validate_project_branch_scope(&imported, "main")?;
    Ok(())
}
