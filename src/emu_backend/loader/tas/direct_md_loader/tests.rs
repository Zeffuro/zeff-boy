use super::*;
use crate::netplay::md_fixture as guest;
use crate::tas_project::{TasControllerInput, TasInputFrame, TasSeekStateCache};

fn input(profile: MdTasProfile) -> TasInputFrame {
    let (p1, p2) = match profile {
        MdTasProfile::ThreeButton => (0x01, 0x0a),
        MdTasProfile::SixButton => (0x15, 0x2e),
    };
    TasInputFrame {
        players: [
            TasControllerInput {
                buttons: p1,
                dpad: 0x01,
            },
            TasControllerInput {
                buttons: p2,
                dpad: 0x08,
            },
            TasControllerInput::default(),
            TasControllerInput::default(),
            TasControllerInput::default(),
        ],
        ..Default::default()
    }
}

#[test]
fn native_md_normal_loaded_fresh_project_two_pad_guest_reopens_and_exports() -> Result<()> {
    for profile in [MdTasProfile::ThreeButton, MdTasProfile::SixButton] {
        let directory = crate::test_support::test_directory("md-tas-foundation")?;
        let path = directory.path().join("parallel.md");
        std::fs::write(&path, guest::rom())?;
        let loader = DirectMdTasExecutionLoader::new(path, profile);
        let project_path = directory.path().join("source.ztas");
        let mut project = loader.create_project_file(&project_path)?;
        let initial = project.start_state().to_vec();
        project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input(profile)))?;
        loader.validate_project(&project)?;
        assert_eq!(
            super::super::project_profile::classify_direct_tas_execution_profile(&project)?,
            super::super::md_product::md_execution_profile(profile)
        );

        let mut actual = loader.load_fresh_backend()?;
        actual.set_input(input(profile).players[0].buttons, 0x01);
        actual.set_input_p2(input(profile).players[1].buttons, 0x08);
        actual.step_frame();
        assert_eq!(actual.frame_count(), 1);
        let EmuBackend::Md(md) = &actual else {
            unreachable!()
        };
        let ram = md.core().work_ram();
        assert_eq!(&ram[0x100..0x104], &[0x67, 0x5d, 0x33, 0x11]);
        if profile == MdTasProfile::SixButton {
            assert_eq!(&ram[0x104..0x108], &[0x63, 0x56, 0x3f, 0x1f]);
        }
        let mut pcm = Vec::new();
        actual.drain_audio_samples_into(&mut pcm);
        assert!(!pcm.is_empty() && pcm.iter().all(|value| value.is_finite()));
        assert!(pcm.iter().any(|value| *value != 0.0));
        assert!(direct_md::identity(&actual, &initial).is_err());
        assert!(loader.load_session(&actual.encode_state_bytes()?).is_err());

        let mut rebuilt = loader.load_fresh_backend()?;
        rebuilt.set_input(input(profile).players[0].buttons, 0x01);
        rebuilt.set_input_p2(input(profile).players[1].buttons, 0x08);
        rebuilt.step_frame();
        rebuilt.drain_audio_samples_into(&mut pcm);
        assert_eq!(actual.encode_state_bytes()?, rebuilt.encode_state_bytes()?);
        assert_eq!(actual.framebuffer(), rebuilt.framebuffer());
        for _ in 0..2 {
            actual.step_frame();
            rebuilt.step_frame();
            let mut actual_pcm = Vec::new();
            let mut rebuilt_pcm = Vec::new();
            actual.drain_audio_samples_into(&mut actual_pcm);
            rebuilt.drain_audio_samples_into(&mut rebuilt_pcm);
            assert!(actual_pcm.iter().any(|sample| *sample != 0.0));
            assert_eq!(guest::bits(&actual_pcm), guest::bits(&rebuilt_pcm));
            let EmuBackend::Md(md) = &actual else {
                unreachable!()
            };
            let EmuBackend::Md(reference) = &rebuilt else {
                unreachable!()
            };
            let config = zeff_md_core::emulator::rollback::MdRollbackConfig::capture(md.core())?;
            assert_eq!(
                config.boundary_hashes(md.core())?,
                config.boundary_hashes(reference.core())?
            );
        }

        project.save_atomic(&project_path)?;
        let reopened = TasProject::load(&project_path)?;
        let plan = super::super::select_private_tas_execution_loader_for_project(
            loader.source_path.clone(),
            ActiveSystem::Md,
            Vec::new(),
            &reopened,
        )?;
        assert_eq!(reopened.identity(), project.identity());
        assert_eq!(reopened.branch("main").unwrap().input_at(0), input(profile));
        let replay = directory.path().join("movie.zrpl");
        let cache = TasSeekStateCache::open(directory.path().join("cache"))?;
        let mut editor = super::editor_tests::open_editor(project, directory.path(), cache)?;
        plan.verify_and_export_editor_session_cancellable(
            &mut editor,
            &replay,
            &std::sync::atomic::AtomicBool::new(false),
            &mut |_| {},
        )?;
        let imported =
            plan.import_replay_file(&replay, &directory.path().join("imported.ztas"), false)?;
        assert_eq!(imported.start_state(), initial);
        assert_eq!(imported.identity(), editor.project().identity());
        assert_eq!(imported.branch("main").unwrap().input_at(0), input(profile));
    }
    Ok(())
}

#[test]
fn native_md_selected_zip_and_source_replacement_refuse_stale_identity() -> Result<()> {
    let directory = crate::test_support::test_directory("md-tas-selected")?;
    let archive = directory.path().join("selected.zip");
    let bytes = guest::rom();
    crate::test_support::write_zip(&archive, &[("a/same.md", &bytes), ("b/same.gen", &bytes)])?;
    let loader = DirectMdTasExecutionLoader::new_zip(
        archive.clone(),
        archive.join("b/same.gen"),
        MdTasProfile::SixButton,
    );
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().effective_media_sha256,
        TasDigest::from_bytes(&bytes)
    );
    assert_ne!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );
    let reopened = DirectMdTasExecutionLoader::new_zip_for_project(archive.clone(), &project)?;
    reopened.validate_project(&project)?;
    let wrong = DirectMdTasExecutionLoader::new_zip(
        archive.clone(),
        archive.join("a/same.md"),
        MdTasProfile::SixButton,
    );
    assert!(wrong.validate_project(&project).is_err());
    crate::test_support::write_zip(
        &archive,
        &[("b/same.gen", &bytes), ("note.txt", b"changed")],
    )?;
    assert!(loader.validate_project(&project).is_err());
    assert!(DirectMdTasExecutionLoader::new_zip_for_project(archive, &project).is_err());
    Ok(())
}

#[test]
fn native_md_strict_project_input_and_loader_admission_are_transactional() -> Result<()> {
    let directory = crate::test_support::test_directory("md-tas-refusals")?;
    let path = directory.path().join("factory.md");
    let bytes = guest::rom();
    std::fs::write(&path, &bytes)?;
    let loader = DirectMdTasExecutionLoader::new(path.clone(), MdTasProfile::ThreeButton);
    let project = loader.create_project()?;
    let mut malformed = project.start_state().to_vec();
    malformed[0] ^= 1;
    assert!(loader.load_session(&malformed).is_err());
    let wrong_pad = DirectMdTasExecutionLoader::new(path.clone(), MdTasProfile::SixButton);
    assert!(wrong_pad.validate_project(&project).is_err());
    let mut invalid = input(MdTasProfile::ThreeButton);
    invalid.players[0].buttons |= 0x10;
    assert!(direct_md::validate_input(MdTasProfile::ThreeButton, invalid).is_err());
    let projected = direct_md::canonicalize_local_host_input(MdTasProfile::ThreeButton, invalid)?;
    assert_eq!(projected.players[0].buttons, 0x01);
    let mut physical_a = TasInputFrame::default();
    physical_a.players[0].buttons = 0x40;
    direct_md::validate_input(MdTasProfile::ThreeButton, physical_a)?;
    let mut foreign = input(MdTasProfile::ThreeButton);
    foreign.players[2].buttons = 1;
    assert!(direct_md::validate_input(MdTasProfile::ThreeButton, foreign).is_err());
    foreign = input(MdTasProfile::ThreeButton);
    foreign.players[0].dpad = 0x10;
    assert!(direct_md::validate_input(MdTasProfile::ThreeButton, foreign).is_err());
    let mut changed = project.clone();
    changed.edit_transaction(|edit| edit.set_input_range("main", 0, 1, invalid))?;
    assert!(loader.validate_project(&changed).is_err());
    let cache = TasSeekStateCache::open(directory.path().join("cache"))?;
    let mut changed = super::editor_tests::open_editor(changed, directory.path(), cache)?;
    let invalid_before = changed.project().encode()?;
    let plan = super::super::PrivateTasExecutionLoader::DirectMd(loader.clone());
    assert!(!changed.manual_path().exists());
    let rejected_replay = directory.path().join("invalid-input.zrpl");
    assert!(
        plan.verify_and_export_editor_session_cancellable(
            &mut changed,
            &rejected_replay,
            &std::sync::atomic::AtomicBool::new(false),
            &mut |_| {},
        )
        .is_err()
    );
    assert_eq!(changed.project().encode()?, invalid_before);
    assert!(!changed.manual_path().exists());
    assert!(!rejected_replay.exists());

    let mut config = BackendLoadConfig {
        apply_mods: false,
        md_region: Some(Region::NtscUs),
        md_controller: zeff_md_core::ControllerKind::ThreeButton,
        md_load_save_ram: false,
        md_host_persistence: false,
        ..Default::default()
    };
    let preloaded = super::super::super::load_backend_from_rom_source(
        ActiveSystem::Md,
        &path,
        &path,
        Some(bytes.clone()),
        config.clone(),
    )?
    .backend;
    assert!(direct_md::identity(&preloaded, &preloaded.encode_state_bytes()?).is_err());
    for (region, timing, sample_rate, initial_input) in [
        (Region::Pal, TimingModel::Standard, None, None),
        (Region::NtscUs, TimingModel::Precise, None, None),
        (Region::NtscUs, TimingModel::Standard, Some(44_100), None),
        (Region::NtscUs, TimingModel::Standard, None, Some((1, 0))),
    ] {
        config.md_region = Some(region);
        config.md_timing = timing;
        config.sample_rate = sample_rate;
        config.initial_input = initial_input;
        let backend = super::super::super::load_backend_from_rom_source(
            ActiveSystem::Md,
            &path,
            &path,
            None,
            config.clone(),
        )?
        .backend;
        assert!(direct_md::identity(&backend, &backend.encode_state_bytes()?).is_err());
    }
    let mut backend = loader.load_fresh_backend()?;
    let raw = backend.encode_state_bytes()?;
    let EmuBackend::Md(md) = &mut backend else {
        unreachable!()
    };
    md.set_opcode_history_enabled(true);
    assert!(md.tas_load().is_err());
    assert_eq!(backend.encode_state_bytes()?, raw);
    let mut changed_source = bytes;
    changed_source[0x3ff] ^= 1;
    std::fs::write(&path, changed_source)?;
    assert!(loader.validate_project(&project).is_err());
    Ok(())
}
