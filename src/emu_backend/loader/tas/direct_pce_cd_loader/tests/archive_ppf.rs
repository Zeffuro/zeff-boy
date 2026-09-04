use super::multicue::ArchiveKind;
use super::*;
use crate::emu_backend::loader::tas::direct_pce_cd::{
    PCE_CD_UNPATCHED_DISC_PATCH_FORMAT, direct_pce_cd_archive_ppf_tas_sync_config_sha256,
    direct_pce_cd_rar_ppf_tas_sync_config_sha256,
    direct_pce_cd_selected_archive_ppf_tas_sync_config_sha256,
    direct_pce_cd_selected_rar_ppf_tas_sync_config_sha256,
    direct_pce_cd_selected_zip_ppf_tas_sync_config_sha256,
    direct_pce_cd_zip_ppf_tas_sync_config_sha256,
};
use anyhow::Context;

impl ArchiveKind {
    fn ppf_sync(self, selected: bool) -> TasDigest {
        match (self, selected) {
            (Self::SevenZip, false) => direct_pce_cd_archive_ppf_tas_sync_config_sha256(),
            (Self::SevenZip, true) => direct_pce_cd_selected_archive_ppf_tas_sync_config_sha256(),
            (Self::Rar, false) => direct_pce_cd_rar_ppf_tas_sync_config_sha256(),
            (Self::Rar, true) => direct_pce_cd_selected_rar_ppf_tas_sync_config_sha256(),
            (Self::Zip, false) => direct_pce_cd_zip_ppf_tas_sync_config_sha256(),
            (Self::Zip, true) => direct_pce_cd_selected_zip_ppf_tas_sync_config_sha256(),
        }
    }
}

#[test]
fn archive_ppf_six_routes_create_reopen_seek_and_bind_ordered_stack() -> Result<()> {
    for kind in [ArchiveKind::SevenZip, ArchiveKind::Rar, ArchiveKind::Zip] {
        exercise_route(kind, false).with_context(|| format!("{kind:?} unique"))?;
        exercise_route(kind, true).with_context(|| format!("{kind:?} selected"))?;
    }
    Ok(())
}

fn exercise_route(kind: ArchiveKind, selected: bool) -> Result<()> {
    let directory = crate::test_support::test_directory(&format!(
        "pce-cd-tas-archive-ppf-{kind:?}-{selected}"
    ))?;
    let path = directory.path().join(format!("disc.{}", kind.extension()));
    let first = ppf1(0, &[0xA5]);
    let second = ppf1(1, &[0x5A]);
    write_ppf_package(&path, kind, selected, 0x41, [&first, &second], false)?;
    fs::write(directory.path().join("ignored-sidecar.ppf"), b"not a PPF")?;
    fs::write(directory.path().join("mods.json"), b"not JSON")?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let rom_path = selected.then(|| path.join("second").join("disc.cue"));
    let mut loader =
        DirectPceCdTasExecutionLoader::new_with_rom_path(path.clone(), rom_path, Vec::new())?;
    loader.system_card_override = Some(system_card);
    loader.system_card_sha256_override = Some(TEST_SYSTEM_CARD_SHA256);
    let mut project = loader.create_project()?;
    assert_eq!(
        project.identity().sync_config_sha256,
        kind.ppf_sync(selected)
    );
    assert_eq!(project.identity().patches.len(), 1);
    assert_eq!(
        project.identity().patches[0].format,
        PCE_CD_UNPATCHED_DISC_PATCH_FORMAT
    );
    let loaded = loader.load_fresh_backend()?;
    let provenance = loaded
        .pce()
        .and_then(crate::emu_backend::PceBackend::tas_load_provenance)
        .expect("archive PPF provenance");
    assert!(provenance.load.direct_pce_cd_archive_ppf);
    assert!(!provenance.load.any_mod_applied || provenance.load.any_mod_enabled);
    assert_eq!(provenance.load.archive_ppf_patches.len(), 2);
    assert!(
        provenance.load.archive_ppf_patches[0]
            .member_path
            .ends_with("/0001.ppf")
    );
    assert!(
        provenance.load.archive_ppf_patches[1]
            .member_path
            .ends_with("/0002.ppf")
    );
    assert_eq!(
        project.identity().patches[0].sha256.0,
        provenance
            .load
            .source_disc_sha256
            .expect("unpatched witness")
    );
    assert_eq!(
        project.identity().effective_media_sha256.0,
        provenance.load.effective_disc_sha256.expect("patched disc")
    );
    assert_ne!(
        provenance.load.source_disc_sha256,
        provenance.load.effective_disc_sha256
    );

    project
        .edit_transaction(|edit| edit.set_input_range("main", 0, 1, TasInputFrame::default()))?;
    let manual = directory.path().join("movie.ztas");
    let autosaves = TasAutosaveStore::beside_manual_save(&manual, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project.clone(), manual, autosaves, cache)?;
    let mut engine = loader.load_editor_engine(&project)?;
    assert!(engine.seek(&mut editor, 1)?.reached_target());
    let reached = engine.backend().encode_state_bytes()?;
    assert!(engine.seek(&mut editor, 0)?.reached_target());
    assert!(engine.seek(&mut editor, 1)?.reached_target());
    assert_eq!(engine.backend().encode_state_bytes()?, reached);

    let mut reopened =
        DirectPceCdTasExecutionLoader::new_for_project(path.clone(), Vec::new(), &project)?;
    reopened.system_card_override = Some(system_card);
    reopened.system_card_sha256_override = Some(TEST_SYSTEM_CARD_SHA256);
    reopened.load_editor_engine(&project)?;

    if selected {
        let wrong_path = path.join("first").join("disc.cue");
        let mut wrong =
            DirectPceCdTasExecutionLoader::new_with_rom_path(path, Some(wrong_path), Vec::new())?;
        wrong.system_card_override = Some(system_card);
        wrong.system_card_sha256_override = Some(TEST_SYSTEM_CARD_SHA256);
        assert!(wrong.load_editor_engine(&project).is_err());
    }
    Ok(())
}

#[test]
fn selected_zip_archive_ppf_rejects_identity_patch_name_gap_order_and_base_mutations() -> Result<()>
{
    let directory = crate::test_support::test_directory("pce-cd-tas-archive-ppf-mutations")?;
    let path = directory.path().join("disc.zip");
    let first = ppf1(0, &[0xA5]);
    let second = ppf1(1, &[0x5A]);
    write_ppf_package(
        &path,
        ArchiveKind::Zip,
        true,
        0x51,
        [&first, &second],
        false,
    )?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let selected = path.join("second").join("disc.cue");
    let configured = || -> Result<DirectPceCdTasExecutionLoader> {
        let mut loader = DirectPceCdTasExecutionLoader::new_with_rom_path(
            path.clone(),
            Some(selected.clone()),
            Vec::new(),
        )?;
        loader.system_card_override = Some(system_card);
        loader.system_card_sha256_override = Some(TEST_SYSTEM_CARD_SHA256);
        Ok(loader)
    };
    let loader = configured()?;
    let project = loader.create_project()?;

    let mut identity = project.identity().clone();
    identity.source_media_sha256 = TasDigest([0xA6; 32]);
    assert!(
        loader
            .load_editor_engine(&project_with_identity(&project, identity)?)
            .is_err()
    );
    let mut identity = project.identity().clone();
    identity.effective_media_sha256 = TasDigest([0x5A; 32]);
    assert!(
        loader
            .load_editor_engine(&project_with_identity(&project, identity)?)
            .is_err()
    );
    let mut identity = project.identity().clone();
    identity.patches[0].sha256 = TasDigest([0x33; 32]);
    assert!(
        loader
            .load_editor_engine(&project_with_identity(&project, identity)?)
            .is_err()
    );

    write_ppf_package(
        &path,
        ArchiveKind::Zip,
        true,
        0x51,
        [&ppf1(0, &[0xA4]), &second],
        false,
    )?;
    assert!(configured()?.load_editor_engine(&project).is_err());
    write_ppf_package(
        &path,
        ArchiveKind::Zip,
        true,
        0x51,
        [&second, &first],
        false,
    )?;
    assert!(configured()?.load_editor_engine(&project).is_err());
    write_ppf_package(
        &path,
        ArchiveKind::Zip,
        true,
        0x52,
        [&first, &second],
        false,
    )?;
    assert!(configured()?.load_editor_engine(&project).is_err());
    write_ppf_package(&path, ArchiveKind::Zip, true, 0x51, [&first, &second], true)?;
    assert!(configured()?.load_editor_engine(&project).is_err());

    write_named_ppf_package(&path, "second/disc.ppf/0002.ppf", &second)?;
    assert!(configured()?.load_fresh_backend().is_err());
    write_named_ppf_package(&path, "second/disc.ppf/0001.PPF", &first)?;
    assert!(configured()?.load_fresh_backend().is_err());
    Ok(())
}

fn project_with_identity(
    project: &TasProject,
    identity: crate::tas_project::TasProjectIdentity,
) -> Result<TasProject> {
    TasProject::new(
        "mutated",
        identity,
        project.start_state().to_vec(),
        Default::default(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count: 1,
            input_spans: Vec::new(),
            events: Vec::new(),
        },
        BTreeMap::new(),
    )
}

fn write_ppf_package(
    path: &Path,
    kind: ArchiveKind,
    selected: bool,
    fill: u8,
    patches: [&[u8]; 2],
    extra: bool,
) -> Result<()> {
    let [first, second] = patches;
    let target = if selected { "second" } else { "set" };
    let mut entries = Vec::new();
    if selected {
        entries.extend(cue_entries("first", fill.wrapping_add(1)));
    }
    entries.extend(cue_entries(target, fill));
    entries.push((format!("{target}/disc.ppf/0002.ppf"), second.to_vec()));
    entries.push((format!("{target}/disc.ppf/0001.ppf"), first.to_vec()));
    if extra {
        entries.push(("outside/ignored.bin".to_owned(), vec![0xEE]));
    }
    write_entries(path, kind, entries)
}

fn write_named_ppf_package(path: &Path, patch_name: &str, patch: &[u8]) -> Result<()> {
    let mut entries = cue_entries("second", 0x51);
    entries.extend(cue_entries("first", 0x52));
    entries.push((patch_name.to_owned(), patch.to_vec()));
    write_entries(path, ArchiveKind::Zip, entries)
}

fn cue_entries(directory: &str, fill: u8) -> Vec<(String, Vec<u8>)> {
    vec![
        (
            format!("{directory}/disc.cue"),
            b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n".to_vec(),
        ),
        (
            format!("{directory}/disc.bin"),
            vec![fill; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES],
        ),
    ]
}

fn write_entries(path: &Path, kind: ArchiveKind, entries: Vec<(String, Vec<u8>)>) -> Result<()> {
    match kind {
        ArchiveKind::SevenZip => {
            let mut writer = ArchiveWriter::create(path)?;
            writer.set_content_methods(vec![EncoderConfiguration::new(EncoderMethod::COPY)]);
            for (name, bytes) in entries {
                writer
                    .push_archive_entry(ArchiveEntry::new_file(&name), Some(Cursor::new(bytes)))?;
            }
            writer.finish()?;
        }
        ArchiveKind::Rar => {
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
        ArchiveKind::Zip => {
            let file = fs::File::create(path)?;
            let mut writer = zip::ZipWriter::new(file);
            for (name, bytes) in entries {
                writer.start_file(name, zip::write::SimpleFileOptions::default())?;
                writer.write_all(&bytes)?;
            }
            writer.finish()?;
        }
    }
    Ok(())
}
