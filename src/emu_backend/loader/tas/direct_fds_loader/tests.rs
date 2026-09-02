use anyhow::Result;
use zeff_emu_common::media::{MediaEvent, MediaObjectId, MediaSlotId};
use zeff_emu_common::replay::ReplayEvent;

use super::*;
use crate::test_support::write_zip;

static FDS_BIOS: [u8; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE] =
    [0xEA; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE];
static OTHER_FDS_BIOS: [u8; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE] =
    [0xE9; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE];

fn disk(sides: usize) -> Vec<u8> {
    (0..sides)
        .flat_map(|side| {
            (0..zeff_nes_core::hardware::cartridge::mappers::FDS_SIDE_SIZE)
                .map(move |index| (side as u8).wrapping_mul(0x51).wrapping_add(index as u8))
        })
        .collect()
}

fn editor(
    project: TasProject,
    directory: &Path,
    name: &str,
) -> Result<crate::tas_project::TasEditorSession> {
    let manual_path = directory.join(format!("{name}.ztas"));
    let autosaves = crate::tas_project::TasAutosaveStore::beside_manual_save(
        &manual_path,
        crate::tas_project::TasAutosaveConfig::default(),
    )?;
    let seek_cache =
        crate::tas_project::TasSeekStateCache::open(directory.join(format!("{name}-cache")))?;
    crate::tas_project::TasEditorSession::new(project, manual_path, autosaves, seek_cache)
}

#[test]
fn direct_project_owns_disk_and_rejects_changed_bios() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-fds-direct")?;
    let disk_path = directory.path().join("game.fds");
    std::fs::write(&disk_path, disk(2))?;
    assert!(
        DirectFdsTasExecutionLoader::new(disk_path.clone(), Vec::new())
            .create_project()
            .is_err()
    );
    let project = DirectFdsTasExecutionLoader::new_with_bios_override(disk_path.clone(), &FDS_BIOS)
        .create_project()?;
    assert_eq!(project.assets().len(), 1);
    assert_eq!(
        crate::emu_backend::loader::classify_direct_tas_execution_profile(&project)?,
        crate::emu_thread::TasExecutionProfile::DirectFdsDisk
    );

    std::fs::write(&disk_path, disk(1))?;
    let reopened =
        DirectFdsTasExecutionLoader::new_for_project(disk_path.clone(), Vec::new(), &project)?
            .with_project_bios_override(&FDS_BIOS);
    assert_eq!(
        reopened.load_session(project.start_state())?.identity(),
        project.identity()
    );
    let mut engine = reopened.load_editor_engine(&project)?;
    let mut editor = editor(project.clone(), directory.path(), "movie")?;
    let first = engine.seek(&mut editor, 1)?;
    engine.seek(&mut editor, 0)?;
    let second = engine.seek(&mut editor, 1)?;
    assert_eq!(first.framebuffer, second.framebuffer);
    let wrong_bios = DirectFdsTasExecutionLoader::new_for_project(disk_path, Vec::new(), &project)?
        .with_project_bios_override(&OTHER_FDS_BIOS);
    assert!(wrong_bios.load_editor_engine(&project).is_err());
    assert!(reopened.load_session(&project.start_state()[..32]).is_err());
    let mut changed_drive = reopened.load_editor_engine(&project)?.into_backend();
    changed_drive.set_fds_disk_side(1)?;
    assert!(
        reopened
            .load_session(&changed_drive.encode_state_bytes()?)
            .is_err()
    );
    Ok(())
}

#[test]
fn media_events_are_exact_across_seek_state_and_headless_verification() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-fds-media-events")?;
    let disk_path = directory.path().join("game.fds");
    let disk_bytes = disk(5);
    let media_id = zeff_nes_core::hardware::cartridge::mappers::FdsImage::parse(&disk_bytes)?
        .media_object_id();
    std::fs::write(&disk_path, &disk_bytes)?;
    let loader = DirectFdsTasExecutionLoader::new_with_bios_override(disk_path.clone(), &FDS_BIOS);
    let mut project = loader.create_project()?;
    project.edit_transaction(|edit| {
        edit.insert_frames("main", 1, 3)?;
        edit.replace_branch_events(
            "main",
            vec![
                ReplayEvent::FdsDiskSide { frame: 0, side: 4 },
                ReplayEvent::Media {
                    frame: 1,
                    sequence: 0,
                    event: MediaEvent::SetWriteProtected {
                        slot: MediaSlotId::from("fds.drive0"),
                        write_protected: true,
                    },
                },
                ReplayEvent::Media {
                    frame: 2,
                    sequence: 0,
                    event: MediaEvent::Eject {
                        slot: MediaSlotId::from("fds.drive0"),
                    },
                },
                ReplayEvent::Media {
                    frame: 3,
                    sequence: 0,
                    event: MediaEvent::Insert {
                        slot: MediaSlotId::from("fds.drive0"),
                        media_id: media_id.clone(),
                        side: Some(4),
                        write_protected: false,
                    },
                },
            ],
        )
    })?;
    validate_fds_tas_branch_scope(&project, "main")?;

    std::fs::write(&disk_path, disk(1))?;
    let reopened = DirectFdsTasExecutionLoader::new_for_project(disk_path, Vec::new(), &project)?
        .with_project_bios_override(&FDS_BIOS);
    let mut engine = reopened.load_editor_engine(&project)?;
    let mut session = editor(project.clone(), directory.path(), "media-events")?;
    let outcome = engine.seek(&mut session, 1)?;
    assert_eq!(outcome.events_applied, 1);
    assert!(
        engine
            .backend()
            .media_slot_snapshot()
            .is_some_and(|snapshot| snapshot.state.side == Some(4))
    );
    let outcome = engine.seek(&mut session, 2)?;
    assert_eq!(outcome.events_applied, 1);
    assert!(
        engine
            .backend()
            .media_slot_snapshot()
            .is_some_and(|snapshot| { snapshot.inserted() && snapshot.state.write_protected })
    );
    engine.seek(&mut session, 3)?;
    assert!(
        engine
            .backend()
            .media_slot_snapshot()
            .is_some_and(|snapshot| !snapshot.inserted())
    );
    engine.seek(&mut session, 4)?;
    assert!(
        engine
            .backend()
            .media_slot_snapshot()
            .is_some_and(|snapshot| {
                snapshot.inserted()
                    && snapshot.state.side == Some(4)
                    && !snapshot.state.write_protected
            })
    );
    engine.seek(&mut session, 0)?;
    assert!(
        engine
            .backend()
            .media_slot_snapshot()
            .is_some_and(|snapshot| {
                snapshot.inserted()
                    && snapshot.state.side == Some(0)
                    && !snapshot.state.write_protected
            })
    );

    let start_state = project.start_state().to_vec();
    let witness = crate::tas_project::TasExecutionWitness {
        identity: reopened.load_session(&start_state)?.identity().clone(),
    };
    project.verify_branch_with_factory("main", &witness, || reopened.load_session(&start_state))?;

    for events in [
        vec![ReplayEvent::Media {
            frame: 0,
            sequence: 1,
            event: MediaEvent::Eject {
                slot: MediaSlotId::from("fds.drive0"),
            },
        }],
        vec![ReplayEvent::Media {
            frame: 0,
            sequence: 0,
            event: MediaEvent::Eject {
                slot: MediaSlotId::from("other"),
            },
        }],
        vec![ReplayEvent::Media {
            frame: 0,
            sequence: 0,
            event: MediaEvent::Insert {
                slot: MediaSlotId::from("fds.drive0"),
                media_id: media_id.clone(),
                side: Some(0),
                write_protected: false,
            },
        }],
        vec![
            ReplayEvent::Media {
                frame: 0,
                sequence: 0,
                event: MediaEvent::Eject {
                    slot: MediaSlotId::from("fds.drive0"),
                },
            },
            ReplayEvent::Media {
                frame: 1,
                sequence: 0,
                event: MediaEvent::Insert {
                    slot: MediaSlotId::from("fds.drive0"),
                    media_id: MediaObjectId::from("sha256:other"),
                    side: Some(0),
                    write_protected: false,
                },
            },
        ],
        vec![
            ReplayEvent::Media {
                frame: 0,
                sequence: 0,
                event: MediaEvent::Eject {
                    slot: MediaSlotId::from("fds.drive0"),
                },
            },
            ReplayEvent::FdsDiskSide { frame: 1, side: 0 },
        ],
        vec![
            ReplayEvent::Media {
                frame: 0,
                sequence: 0,
                event: MediaEvent::Eject {
                    slot: MediaSlotId::from("fds.drive0"),
                },
            },
            ReplayEvent::Media {
                frame: 1,
                sequence: 0,
                event: MediaEvent::SetWriteProtected {
                    slot: MediaSlotId::from("fds.drive0"),
                    write_protected: true,
                },
            },
        ],
        vec![
            ReplayEvent::Media {
                frame: 0,
                sequence: 0,
                event: MediaEvent::Eject {
                    slot: MediaSlotId::from("fds.drive0"),
                },
            },
            ReplayEvent::Media {
                frame: 0,
                sequence: 0,
                event: MediaEvent::Insert {
                    slot: MediaSlotId::from("fds.drive0"),
                    media_id: media_id.clone(),
                    side: Some(0),
                    write_protected: false,
                },
            },
        ],
        vec![ReplayEvent::Media {
            frame: 4,
            sequence: 0,
            event: MediaEvent::Eject {
                slot: MediaSlotId::from("fds.drive0"),
            },
        }],
    ] {
        let mut invalid = project.clone();
        invalid.edit_transaction(|edit| edit.replace_branch_events("main", events))?;
        assert!(validate_fds_tas_branch_scope(&invalid, "main").is_err());
    }
    Ok(())
}

#[test]
fn selected_zip_member_is_owned_and_events_are_bounded() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-fds-zip")?;
    let archive_path = directory.path().join("games.zip");
    let first = disk(1);
    let selected = disk(2);
    write_zip(
        &archive_path,
        &[("first.fds", &first), ("folder/selected.fds", &selected)],
    )?;
    let loader = DirectFdsTasExecutionLoader::new_zip_with_bios_override(
        archive_path.clone(),
        Some(archive_path.join("folder/selected.fds")),
        &FDS_BIOS,
    );
    let mut project = loader.create_project()?;
    assert!(
        project
            .assets()
            .values()
            .next()
            .is_some_and(|asset| asset.ends_with(&selected))
    );
    project.edit_transaction(|edit| {
        edit.replace_branch_events("main", vec![ReplayEvent::FdsDiskSide { frame: 0, side: 1 }])
    })?;
    validate_fds_tas_branch_scope(&project, "main")?;

    let mut invalid = project.clone();
    invalid.edit_transaction(|edit| {
        edit.replace_branch_events("main", vec![ReplayEvent::FdsDiskSide { frame: 1, side: 0 }])
    })?;
    assert!(validate_fds_tas_branch_scope(&invalid, "main").is_err());

    let unsupported_path = directory.path().join("too-many.fds");
    let too_many_sides = disk(usize::from(u8::MAX) + 1);
    std::fs::write(&unsupported_path, &too_many_sides)?;
    assert!(
        DirectFdsTasExecutionLoader::new_with_bios_override(unsupported_path, &FDS_BIOS)
            .create_project()
            .is_err()
    );
    let oversized_archive = directory.path().join("too-many.zip");
    write_zip(&oversized_archive, &[("too-many.fds", &too_many_sides)])?;
    assert!(
        DirectFdsTasExecutionLoader::new_zip_with_bios_override(
            oversized_archive.clone(),
            Some(oversized_archive.join("too-many.fds")),
            &FDS_BIOS,
        )
        .create_project()
        .is_err()
    );
    let mut headered = vec![0; zeff_nes_core::hardware::cartridge::mappers::FDS_HEADER_SIZE];
    headered[..4].copy_from_slice(b"FDS\x1A");
    headered[4] = 1;
    headered.extend_from_slice(&first);
    let headered_archive = directory.path().join("headered.zip");
    write_zip(&headered_archive, &[("headered.fds", &headered)])?;
    assert!(
        DirectFdsTasExecutionLoader::new_zip_with_bios_override(
            headered_archive.clone(),
            Some(headered_archive.join("headered.fds")),
            &FDS_BIOS,
        )
        .create_project()
        .is_err()
    );
    Ok(())
}

#[test]
fn three_side_disk_set_is_exact_for_direct_and_zip_projects() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-fds-three-side")?;
    let direct_path = directory.path().join("three.fds");
    let three_sides = disk(3);
    std::fs::write(&direct_path, &three_sides)?;
    let direct_loader =
        DirectFdsTasExecutionLoader::new_with_bios_override(direct_path.clone(), &FDS_BIOS);
    let mut direct_project = direct_loader.create_project()?;
    assert_eq!(
        direct_project.identity().sync_config_sha256,
        direct_fds_tas_sync_config_sha256(3)?
    );
    for side_count in [1, 2, 4] {
        assert_ne!(
            direct_project.identity().sync_config_sha256,
            direct_fds_tas_sync_config_sha256(side_count)?
        );
    }
    direct_project.edit_transaction(|edit| {
        edit.replace_branch_events("main", vec![ReplayEvent::FdsDiskSide { frame: 0, side: 2 }])
    })?;
    validate_fds_tas_branch_scope(&direct_project, "main")?;
    std::fs::write(&direct_path, disk(1))?;
    let reopened =
        DirectFdsTasExecutionLoader::new_for_project(direct_path, Vec::new(), &direct_project)?
            .with_project_bios_override(&FDS_BIOS);
    assert_eq!(
        reopened
            .load_session(direct_project.start_state())?
            .identity(),
        direct_project.identity()
    );

    let archive_path = directory.path().join("three.zip");
    write_zip(&archive_path, &[("set/three.fds", &three_sides)])?;
    let zip_loader = DirectFdsTasExecutionLoader::new_zip_with_bios_override(
        archive_path.clone(),
        Some(archive_path.join("set/three.fds")),
        &FDS_BIOS,
    );
    let mut zip_project = zip_loader.create_project()?;
    assert_eq!(
        zip_project.identity().sync_config_sha256,
        zip_fds_tas_sync_config_sha256("set/three.fds", 3)?
    );
    for side_count in [1, 2, 4] {
        assert_ne!(
            zip_project.identity().sync_config_sha256,
            zip_fds_tas_sync_config_sha256("set/three.fds", side_count)?
        );
    }
    zip_project.edit_transaction(|edit| {
        edit.replace_branch_events("main", vec![ReplayEvent::FdsDiskSide { frame: 0, side: 2 }])
    })?;
    validate_fds_tas_branch_scope(&zip_project, "main")?;
    write_zip(&archive_path, &[("changed.fds", &disk(1))])?;
    let zip_reopened =
        DirectFdsTasExecutionLoader::new_for_project(archive_path, Vec::new(), &zip_project)?
            .with_project_bios_override(&FDS_BIOS);
    assert_eq!(
        zip_reopened
            .load_session(zip_project.start_state())?
            .identity(),
        zip_project.identity()
    );
    Ok(())
}

#[test]
fn four_side_disk_set_is_exact_for_direct_zip_and_side_events() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-fds-four-side")?;
    let direct_path = directory.path().join("four.fds");
    let four_sides = disk(4);
    std::fs::write(&direct_path, &four_sides)?;
    let direct_loader =
        DirectFdsTasExecutionLoader::new_with_bios_override(direct_path.clone(), &FDS_BIOS);
    let mut direct_project = direct_loader.create_project()?;
    assert_eq!(
        direct_project.identity().sync_config_sha256,
        direct_fds_tas_sync_config_sha256(4)?
    );
    assert_ne!(
        direct_project.identity().sync_config_sha256,
        direct_fds_tas_sync_config_sha256(2)?
    );
    direct_project.edit_transaction(|edit| {
        edit.replace_branch_events("main", vec![ReplayEvent::FdsDiskSide { frame: 0, side: 3 }])
    })?;
    validate_fds_tas_branch_scope(&direct_project, "main")?;

    std::fs::write(&direct_path, disk(1))?;
    let reopened =
        DirectFdsTasExecutionLoader::new_for_project(direct_path, Vec::new(), &direct_project)?
            .with_project_bios_override(&FDS_BIOS);
    let mut engine = reopened.load_editor_engine(&direct_project)?;
    let mut direct_editor = editor(direct_project.clone(), directory.path(), "four-direct")?;
    engine.seek(&mut direct_editor, 1)?;
    let backend = engine.into_backend();
    let nes = backend.nes().context("FDS test backend must remain NES")?;
    assert_eq!(nes.fds_disk_side(), Some(3));

    let archive_path = directory.path().join("four.zip");
    write_zip(&archive_path, &[("set/four.fds", &four_sides)])?;
    let zip_loader = DirectFdsTasExecutionLoader::new_zip_with_bios_override(
        archive_path.clone(),
        Some(archive_path.join("set/four.fds")),
        &FDS_BIOS,
    );
    let mut zip_project = zip_loader.create_project()?;
    assert_eq!(
        zip_project.identity().sync_config_sha256,
        zip_fds_tas_sync_config_sha256("set/four.fds", 4)?
    );
    zip_project.edit_transaction(|edit| {
        edit.replace_branch_events("main", vec![ReplayEvent::FdsDiskSide { frame: 0, side: 3 }])
    })?;
    validate_fds_tas_branch_scope(&zip_project, "main")?;
    write_zip(&archive_path, &[("changed.fds", &disk(1))])?;
    let zip_reopened =
        DirectFdsTasExecutionLoader::new_for_project(archive_path, Vec::new(), &zip_project)?
            .with_project_bios_override(&FDS_BIOS);
    assert_eq!(
        zip_reopened
            .load_session(zip_project.start_state())?
            .identity(),
        zip_project.identity()
    );
    Ok(())
}

#[test]
fn five_side_disk_set_is_exact_for_direct_and_selected_zip() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-fds-five-side")?;
    let direct_path = directory.path().join("five.fds");
    let five_sides = disk(5);
    std::fs::write(&direct_path, &five_sides)?;
    let direct_loader =
        DirectFdsTasExecutionLoader::new_with_bios_override(direct_path.clone(), &FDS_BIOS);
    let mut direct_project = direct_loader.create_project()?;
    assert_eq!(
        direct_project.identity().sync_config_sha256,
        direct_fds_tas_sync_config_sha256(5)?
    );
    for side_count in 1..=4 {
        assert_ne!(
            direct_project.identity().sync_config_sha256,
            direct_fds_tas_sync_config_sha256(side_count)?
        );
    }
    assert!(fds_tas_side_count_supported(usize::from(u8::MAX)));
    assert!(!fds_tas_side_count_supported(usize::from(u8::MAX) + 1));
    assert!(direct_fds_tas_sync_config_sha256(0).is_err());
    assert!(direct_fds_tas_sync_config_sha256(256).is_err());
    assert_ne!(
        direct_fds_tas_sync_config_sha256(5)?,
        direct_fds_tas_sync_config_sha256(6)?
    );
    direct_project.edit_transaction(|edit| {
        edit.replace_branch_events("main", vec![ReplayEvent::FdsDiskSide { frame: 0, side: 4 }])
    })?;
    validate_fds_tas_branch_scope(&direct_project, "main")?;
    std::fs::write(&direct_path, disk(1))?;
    let reopened =
        DirectFdsTasExecutionLoader::new_for_project(direct_path, Vec::new(), &direct_project)?
            .with_project_bios_override(&FDS_BIOS);
    let mut engine = reopened.load_editor_engine(&direct_project)?;
    let mut direct_editor = editor(direct_project.clone(), directory.path(), "five-direct")?;
    engine.seek(&mut direct_editor, 1)?;
    assert_eq!(engine.backend().nes().unwrap().fds_disk_side(), Some(4));

    let archive_path = directory.path().join("five.zip");
    write_zip(&archive_path, &[("set/five.fds", &five_sides)])?;
    let zip_loader = DirectFdsTasExecutionLoader::new_zip_with_bios_override(
        archive_path.clone(),
        Some(archive_path.join("set/five.fds")),
        &FDS_BIOS,
    );
    let mut zip_project = zip_loader.create_project()?;
    assert_eq!(
        zip_project.identity().sync_config_sha256,
        zip_fds_tas_sync_config_sha256("set/five.fds", 5)?
    );
    assert_ne!(
        zip_project.identity().sync_config_sha256,
        zip_fds_tas_sync_config_sha256("set/six.fds", 5)?
    );
    zip_project.edit_transaction(|edit| {
        edit.replace_branch_events("main", vec![ReplayEvent::FdsDiskSide { frame: 0, side: 4 }])
    })?;
    validate_fds_tas_branch_scope(&zip_project, "main")?;
    write_zip(&archive_path, &[("changed.fds", &disk(1))])?;
    let zip_reopened =
        DirectFdsTasExecutionLoader::new_for_project(archive_path, Vec::new(), &zip_project)?
            .with_project_bios_override(&FDS_BIOS);
    assert_eq!(
        zip_reopened
            .load_session(zip_project.start_state())?
            .identity(),
        zip_project.identity()
    );
    Ok(())
}
