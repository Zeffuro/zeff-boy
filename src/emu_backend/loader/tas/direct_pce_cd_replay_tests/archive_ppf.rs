use anyhow::Context;

use super::*;

#[derive(Clone, Copy, Debug)]
enum ArchivePpfKind {
    SevenZip,
    Rar,
    Zip,
}

impl ArchivePpfKind {
    fn extension(self) -> &'static str {
        match self {
            Self::SevenZip => "7z",
            Self::Rar => "rar",
            Self::Zip => "zip",
        }
    }

    fn media(self, selected: bool) -> MediaKind {
        match (self, selected) {
            (Self::SevenZip, false) => MediaKind::ArchivePpf,
            (Self::SevenZip, true) => MediaKind::SelectedArchivePpf,
            (Self::Rar, false) => MediaKind::RarPpf,
            (Self::Rar, true) => MediaKind::SelectedRarPpf,
            (Self::Zip, false) => MediaKind::ZipPpf,
            (Self::Zip, true) => MediaKind::SelectedZipPpf,
        }
    }
}

#[test]
fn archive_ppf_replay_auto_import_reopens_and_seeks_all_six_routes() -> Result<()> {
    for (index, kind) in [
        ArchivePpfKind::SevenZip,
        ArchivePpfKind::Rar,
        ArchivePpfKind::Zip,
    ]
    .into_iter()
    .enumerate()
    {
        exercise_archive_ppf_replay(kind, false, 0x91 + index as u8)
            .with_context(|| format!("{kind:?} unique"))?;
        exercise_archive_ppf_replay(kind, true, 0xA1 + index as u8)
            .with_context(|| format!("{kind:?} selected"))?;
    }
    Ok(())
}

fn exercise_archive_ppf_replay(kind: ArchivePpfKind, selected: bool, fill: u8) -> Result<()> {
    let directory = crate::test_support::test_directory(&format!(
        "pce-cd-zrpl-archive-ppf-{}-{selected}",
        kind.extension()
    ))?;
    let archive = directory.path().join(format!("disc.{}", kind.extension()));
    write_archive_ppf(&archive, kind, selected, fill)?;
    let rom_path = selected.then(|| archive.join("second").join("disc.cue"));
    let system_card = system_card();
    let loader = if let Some(rom_path) = rom_path.clone() {
        DirectPceCdTasExecutionLoader::new_with_rom_path_and_system_card_override(
            archive.clone(),
            rom_path,
            system_card,
            SYSTEM_CARD_SHA256,
        )?
    } else {
        DirectPceCdTasExecutionLoader::new_with_system_card_override(
            archive.clone(),
            system_card,
            SYSTEM_CARD_SHA256,
        )
    };
    let project_path = directory.path().join("source.ztas");
    let replay_path = directory.path().join("verified.zrpl");
    let imported_path = directory.path().join("imported.ztas");
    let mut project = loader.create_project()?;
    let mut input = TasInputFrame::default();
    input.players[0].buttons = 0x03;
    input.players[0].dpad = 0x04;
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    assert_eq!(
        project.identity().sync_config_sha256,
        expected_sync_config(kind.media(selected), CardKind::None)
    );
    assert_eq!(project.identity().patches.len(), 1);

    let autosaves =
        TasAutosaveStore::beside_manual_save(&project_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("source-seek-cache"))?;
    let mut editor = TasEditorSession::new(project.clone(), &project_path, autosaves, cache)?;
    let plan = PrivateTasExecutionLoader::DirectPceCd(loader);
    assert_eq!(
        plan.verify_and_export_editor_session(&mut editor, &replay_path)?,
        replay_path
    );

    let _system_card =
        super::super::register_test_pce_cd_system_card(SYSTEM_CARD_SHA256, system_card);
    let start_state = TasProject::read_zrpl_start_state(&replay_path)?;
    if selected {
        let wrong_import = directory.path().join("wrong-member.ztas");
        let wrong = super::super::select_private_tas_execution_loader_for_replay(
            archive.clone(),
            Some(archive.join("first").join("disc.cue")),
            crate::emu_backend::ActiveSystem::Pce,
            Vec::new(),
            &start_state,
        )
        .and_then(|loader| loader.import_replay_file(&replay_path, &wrong_import, false));
        assert!(wrong.is_err());
        assert!(!wrong_import.exists());
    }
    let selected_loader = super::super::select_private_tas_execution_loader_for_replay(
        archive.clone(),
        rom_path,
        crate::emu_backend::ActiveSystem::Pce,
        Vec::new(),
        &start_state,
    )?;
    let imported = selected_loader.import_replay_file(&replay_path, &imported_path, false)?;
    assert_eq!(imported.identity(), project.identity());
    assert_eq!(imported.branch("main").expect("main").input_at(0), input);

    let reopened = super::super::select_private_tas_execution_loader_for_project(
        archive,
        crate::emu_backend::ActiveSystem::Pce,
        Vec::new(),
        &imported,
    )?;
    let autosaves =
        TasAutosaveStore::beside_manual_save(&imported_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("imported-seek-cache"))?;
    let mut imported_editor = TasEditorSession::open(&imported_path, autosaves, cache)?;
    let mut engine = reopened.load_editor_engine(imported_editor.project())?;
    assert!(engine.seek(&mut imported_editor, 1)?.reached_target());
    Ok(())
}

fn write_archive_ppf(path: &Path, kind: ArchivePpfKind, selected: bool, fill: u8) -> Result<()> {
    let target = if selected { "second" } else { "set" };
    let mut entries = Vec::new();
    if selected {
        entries.extend(cue_entries("first", fill.wrapping_add(1)));
    }
    entries.extend(cue_entries(target, fill));
    entries.push((format!("{target}/disc.ppf/0002.ppf"), ppf1(1, &[0x5A])));
    entries.push((format!("{target}/disc.ppf/0001.ppf"), ppf1(0, &[0xA5])));
    write_archive_entries(path, kind, entries)
}

fn cue_entries(directory: &str, fill: u8) -> Vec<(String, Vec<u8>)> {
    vec![
        (format!("{directory}/disc.cue"), cue()),
        (
            format!("{directory}/disc.bin"),
            vec![fill; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES],
        ),
    ]
}

fn write_archive_entries(
    path: &Path,
    kind: ArchivePpfKind,
    entries: Vec<(String, Vec<u8>)>,
) -> Result<()> {
    match kind {
        ArchivePpfKind::SevenZip => {
            let mut writer = ArchiveWriter::create(path)?;
            writer.set_content_methods(vec![EncoderConfiguration::new(EncoderMethod::COPY)]);
            for (name, bytes) in entries {
                writer
                    .push_archive_entry(ArchiveEntry::new_file(&name), Some(Cursor::new(bytes)))?;
            }
            writer.finish()?;
        }
        ArchivePpfKind::Rar => {
            let entries = entries
                .into_iter()
                .map(|(name, bytes)| {
                    RarArchiveEntry::new(
                        name.into_bytes(),
                        EntrySource::from_bytes(std::sync::Arc::<[u8]>::from(bytes)),
                    )
                })
                .collect::<Vec<_>>();
            let bytes = Rar50Writer::new(
                WriterOptions::new(ArchiveVersion::Rar50, FeatureSet::store_only())
                    .with_compression_level(0),
            )
            .entries(entries)
            .finish()?;
            fs::write(path, bytes)?;
        }
        ArchivePpfKind::Zip => {
            let mut writer = zip::ZipWriter::new(fs::File::create(path)?);
            for (name, bytes) in entries {
                writer.start_file(name, zip::write::SimpleFileOptions::default())?;
                writer.write_all(&bytes)?;
            }
            writer.finish()?;
        }
    }
    Ok(())
}

fn cue() -> Vec<u8> {
    b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n".to_vec()
}
