use super::super::project_profile::TasProjectRuntimeWitness;
use super::*;
use crate::emu_backend::loader::{
    select_private_tas_execution_attachment, select_private_tas_execution_loader_for_project,
    select_private_tas_execution_loader_for_replay,
};
use crate::tas_project::TasEditorExecutionAttachment;

#[test]
fn native_md_product_selects_real_direct_and_explicit_zip_providers_for_both_pad_profiles()
-> Result<()> {
    for pad in [MdTasProfile::ThreeButton, MdTasProfile::SixButton] {
        for zipped in [false, true] {
            let directory = crate::test_support::test_directory("md-product-routing")?;
            let bytes = crate::netplay::md_fixture::rom();
            let source = directory
                .path()
                .join(if zipped { "game.zip" } else { "game.md" });
            let member = zipped.then(|| source.join("selected/game.gen"));
            if zipped {
                crate::test_support::write_zip(
                    &source,
                    &[("other.md", &bytes), ("selected/game.gen", &bytes)],
                )?;
                assert!(select_md_tas_loader(source.clone(), None, pad).is_err());
            } else {
                std::fs::write(&source, &bytes)?;
            }
            let loader = select_md_tas_loader(source.clone(), member.clone(), pad)?;
            let project = loader.create_project_file(&directory.path().join("movie.ztas"))?;
            assert_eq!(
                classify_md_tas_project(&project)?,
                md_execution_profile(pad)
            );
            let selected = select_private_tas_execution_loader_for_project(
                source.clone(),
                ActiveSystem::Md,
                Vec::new(),
                &project,
            )?;
            let super::super::PrivateTasExecutionLoader::DirectMd(actual) = &selected else {
                panic!("not the real MD provider")
            };
            actual.validate_project(&project)?;
            let engine = crate::tas_project::TasEditorExecutionProvider::load_editor_engine(
                &selected, &project,
            )?;
            assert_eq!(loaded_md_tas_profile(engine.backend()), Some(pad));
            let source_bytes = std::fs::read(&source)?;
            let expected_source = crate::emu_backend::capabilities::TasSourceMediaIdentity::new(
                zeff_firmware::sha256_bytes(&source_bytes),
                source_bytes.len(),
            );
            assert_eq!(
                TasDigest(expected_source.sha256),
                project.identity().source_media_sha256
            );
            let probe = engine.backend().capabilities().tas_execution_primitives;
            assert_eq!(probe.source_media_identity, Some(expected_source));
            assert!(probe.source_media_identity_observed);
            assert!(!probe.direct_runtime_profile_requirements_match);
            if zipped {
                assert_ne!(
                    expected_source.sha256,
                    project.identity().effective_media_sha256.0
                );
            }
            let replay_provider = select_private_tas_execution_loader_for_replay(
                source.clone(),
                member.clone(),
                ActiveSystem::Md,
                Vec::new(),
                project.start_state(),
            )?;
            assert_eq!(
                replay_provider
                    .load_session(project.start_state())?
                    .identity(),
                project.identity()
            );
            let attachment = select_private_tas_execution_attachment(
                Some(source.clone()),
                member,
                Some(ActiveSystem::Md),
                Vec::new(),
                Some(&project),
            );
            let TasEditorExecutionAttachment::Available(provider) = attachment else {
                panic!("MD attachment missing")
            };
            assert_eq!(
                provider.load_editor_engine(&project)?.backend().rom_hash(),
                project.identity().effective_media_sha256.0
            );
            let mut backend = actual.load_fresh_backend()?;
            backend.step_frame();
            let witness = build_md_tas_witness(&backend, false, md_execution_profile(pad))
                .map_err(|error| anyhow::anyhow!("actual MD witness failed: {error:?}"))?;
            assert_eq!(witness.frame_count, 1);
            validate_md_tas_project_witness(
                &project,
                "main",
                TasProjectRuntimeWitness {
                    profile: witness.profile,
                    source_media_sha256: witness.source_media_sha256,
                    effective_media_sha256: witness.effective_media_sha256,
                    current_state_bytes: &witness.current_state_bytes,
                    current_state_sha256: witness.current_state_sha256,
                    determinism_abi: witness.determinism_abi,
                    state_format_compatibility_id: witness.state_format_compatibility_id,
                    sync_config_sha256: witness.sync_config_sha256,
                },
            )?;
            let EmuBackend::Md(md) = &mut backend else {
                unreachable!()
            };
            let lease = md.begin_tas_live(false).unwrap();
            assert!(md.tas_load().is_err());
            assert_eq!(
                backend
                    .capabilities()
                    .tas_execution_primitives
                    .source_media_identity,
                Some(expected_source)
            );
            let EmuBackend::Md(md) = &mut backend else {
                unreachable!()
            };
            md.prepare_tas_live_original_return(&lease)?;
            md.complete_tas_live_original(&lease, Ok(()))?;
            if zipped {
                let wrong = super::super::DirectMdTasExecutionLoader::new_zip(
                    source,
                    directory.path().join("game.zip/other.md"),
                    pad,
                );
                assert!(wrong.validate_project(&project).is_err());
            }
        }
    }
    Ok(())
}

#[test]
fn native_md_loaded_authority_and_strict_import_are_distinct_from_local_pad_projection()
-> Result<()> {
    let directory = crate::test_support::test_directory("md-product-authority")?;
    let source = directory.path().join("game.md");
    let mut bytes = crate::netplay::md_fixture::rom();
    std::fs::write(&source, &bytes)?;
    let provider =
        super::super::DirectMdTasExecutionLoader::new(source.clone(), MdTasProfile::ThreeButton);
    let project = provider.create_project()?;
    let backend = provider.load_fresh_backend()?;
    let before = backend.encode_state_bytes()?;
    let captured_source = backend
        .capabilities()
        .tas_execution_primitives
        .source_media_identity;
    assert!(captured_source.is_some());
    assert_eq!(
        loaded_md_tas_profile(&backend),
        Some(MdTasProfile::ThreeButton)
    );
    assert!(build_md_tas_witness(&backend, false, TasExecutionProfile::DirectMdSixButton).is_err());
    let mut local = TasInputFrame::default();
    local.players[0].buttons = 0xff;
    local.players[1].buttons = 0xff;
    let projected =
        canonicalize_md_tas_host_input(TasExecutionProfile::DirectMdThreeButton, local)?;
    assert_eq!(
        [projected.players[0].buttons, projected.players[1].buttons],
        [0x4b; 2]
    );
    assert_eq!(projected.players[0].buttons & 0x40, 0x40);
    let mut wire = crate::emu_thread::TasInputFrame {
        p1_buttons: 0x40,
        ..Default::default()
    };
    validate_md_tas_worker_input(TasExecutionProfile::DirectMdThreeButton, wire)?;
    wire.p1_buttons |= 0x10;
    assert!(validate_md_tas_worker_input(TasExecutionProfile::DirectMdThreeButton, wire).is_err());
    validate_md_tas_worker_input(TasExecutionProfile::DirectMdSixButton, wire)?;
    wire.p3_buttons = 1;
    assert!(validate_md_tas_worker_input(TasExecutionProfile::DirectMdSixButton, wire).is_err());
    bytes[0x3ff] ^= 1;
    std::fs::write(&source, bytes)?;
    assert!(loaded_md_tas_profile(&backend).is_none());
    assert!(
        build_md_tas_witness(&backend, false, TasExecutionProfile::DirectMdThreeButton).is_err()
    );
    assert_eq!(
        backend
            .capabilities()
            .tas_execution_primitives
            .source_media_identity,
        captured_source
    );
    assert!(
        select_private_tas_execution_loader_for_project(
            source,
            ActiveSystem::Md,
            Vec::new(),
            &project
        )
        .is_err()
    );
    assert_eq!(backend.encode_state_bytes()?, before);
    Ok(())
}
