use super::*;
use crate::netplay::md_fixture as guest;
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasControllerInput, TasEditorSession, TasInputFrame,
    TasSeekCacheLimits, TasSeekStateCache,
};
use zeff_md_core::emulator::rollback::{MdAllocationKind, MdRollbackConfig, MdRollbackHashes};

fn frame(profile: MdTasProfile, odd: bool) -> TasInputFrame {
    let words = match (profile, odd) {
        (MdTasProfile::ThreeButton, false) => [0x0101u16, 0x080a],
        (MdTasProfile::ThreeButton, true) => [0x0402, 0x0241],
        (MdTasProfile::SixButton, false) => [0x0115, 0x082e],
        (MdTasProfile::SixButton, true) => [0x04a2, 0x02d1],
    };
    let mut input = TasInputFrame::default();
    for (player, word) in input.players[..2].iter_mut().zip(words) {
        *player = TasControllerInput {
            buttons: word as u8,
            dpad: (word >> 8) as u8,
        };
    }
    input
}

fn movie(loader: &DirectMdTasExecutionLoader) -> Result<TasProject> {
    let mut project = loader.create_project()?;
    project.edit_transaction(|edit| {
        edit.insert_frames("main", 1, 3)?;
        for cursor in 0..4 {
            edit.set_input_range("main", cursor, 1, frame(loader.profile, cursor % 2 != 0))?;
        }
        Ok(())
    })?;
    Ok(project)
}

pub(super) fn open_editor(
    project: TasProject,
    root: &Path,
    cache: TasSeekStateCache,
) -> Result<TasEditorSession> {
    let path = root.join("movie.ztas");
    let autosaves = TasAutosaveStore::beside_manual_save(&path, TasAutosaveConfig::default())?;
    TasEditorSession::new(project, path, autosaves, cache)
}

type BackendObservation = (Vec<u8>, Vec<u8>, MdRollbackHashes, [u8; 32]);

fn observe(backend: &EmuBackend) -> Result<BackendObservation> {
    let EmuBackend::Md(md) = backend else {
        anyhow::bail!("MD editor test lost its backend")
    };
    let config = MdRollbackConfig::capture(md.core())?;
    Ok((
        backend.encode_state_bytes()?,
        md.core().encode_debugger_checkpoint()?,
        config.boundary_hashes(md.core())?,
        md.tas_editor_host_witness_for_test()?,
    ))
}

fn control(
    loader: &DirectMdTasExecutionLoader,
    project: &TasProject,
    cursor: u64,
) -> Result<EmuBackend> {
    let mut backend = loader.load_fresh_backend()?;
    for position in 0..cursor {
        let input = project.branch("main").unwrap().input_at(position);
        backend.set_input(input.players[0].buttons, input.players[0].dpad);
        backend.set_input_p2(input.players[1].buttons, input.players[1].dpad);
        let before = backend.frame_count();
        backend.step_frame();
        assert_eq!(backend.frame_count(), before + 1);
    }
    Ok(backend)
}

#[test]
fn native_md_owned_editor_seeks_real_cache_and_reopens_both_two_pad_profiles() -> Result<()> {
    for profile in [MdTasProfile::ThreeButton, MdTasProfile::SixButton] {
        let directory = crate::test_support::test_directory("md-owned-editor")?;
        let path = directory.path().join("parallel.md");
        std::fs::write(&path, guest::rom())?;
        let loader = DirectMdTasExecutionLoader::new(path, profile);
        let cache = TasSeekStateCache::open(directory.path().join("cache"))?;
        let mut editor = open_editor(movie(&loader)?, directory.path(), cache.clone())?;
        let provider: &dyn TasEditorExecutionProvider = &loader;
        let mut engine = provider.load_editor_engine(editor.project())?;
        let first = engine.seek(&mut editor, 2)?;
        assert_eq!(first.restored_cache_cursor, None);
        assert_eq!(first.executed_frames, 2);
        assert!(first.cache_store_error.is_none());
        let at_two = observe(engine.backend())?;
        let EmuBackend::Md(md) = engine.backend() else {
            unreachable!()
        };
        assert!(md.core().pending_audio_frames() > 0);
        let (width, height) = md.core().framebuffer_dimensions();
        assert_eq!(
            (first.framebuffer.width(), first.framebuffer.height()),
            (width as u32, height as u32)
        );
        assert_eq!(first.framebuffer.rgba(), md.core().framebuffer());
        let second = engine.seek(&mut editor, 4)?;
        assert_eq!(second.restored_cache_cursor, Some(2));
        assert_eq!(second.executed_frames, 2);
        assert!(second.cache_store_error.is_none());
        let again = engine.seek(&mut editor, 2)?;
        assert_eq!(again.restored_cache_cursor, Some(2));
        assert_eq!(again.executed_frames, 0);
        assert_eq!(observe(engine.backend())?, at_two);
        let reference = control(&loader, editor.project(), 2)?;
        assert_eq!(observe(engine.backend())?, observe(&reference)?);
        let EmuBackend::Md(actual) = engine.backend() else {
            unreachable!()
        };
        let EmuBackend::Md(expected) = &reference else {
            unreachable!()
        };
        let mut actual = actual.core().clone();
        let mut expected = expected.core().clone();
        assert_eq!(
            guest::bits(&actual.drain_audio_samples()),
            guest::bits(&expected.drain_audio_samples())
        );
        for _ in 0..2 {
            actual
                .step_field()
                .map_err(|fault| anyhow::anyhow!("actual future field: {fault:?}"))?;
            expected
                .step_field()
                .map_err(|fault| anyhow::anyhow!("control future field: {fault:?}"))?;
            let pcm = actual.drain_audio_samples();
            assert!(!pcm.is_empty() && pcm.iter().all(|sample| sample.is_finite()));
            assert!(pcm.iter().any(|sample| *sample != 0.0));
            assert_eq!(
                guest::bits(&pcm),
                guest::bits(&expected.drain_audio_samples())
            );
            let config = MdRollbackConfig::capture(&actual)?;
            assert_eq!(
                config.boundary_hashes(&actual)?,
                config.boundary_hashes(&expected)?
            );
        }
        editor.edit_transaction(|edit| edit.set_input_range("main", 2, 1, frame(profile, true)))?;
        let suffix = engine.seek(&mut editor, 4)?;
        assert_eq!(suffix.restored_cache_cursor, Some(2));
        assert_eq!(suffix.executed_frames, 2);
        assert_eq!(
            observe(engine.backend())?,
            observe(&control(&loader, editor.project(), 4)?)?
        );
        let project_path = directory.path().join("accepted.ztas");
        editor.project().save_atomic(&project_path)?;
        let reopened = TasProject::load(&project_path)?;
        let plan = super::super::select_private_tas_execution_loader_for_project(
            loader.source_path.clone(),
            ActiveSystem::Md,
            Vec::new(),
            &reopened,
        )?;
        let mut reopened = open_editor(reopened, directory.path(), cache.clone())?;
        let replay_path = directory.path().join("accepted.zrpl");
        plan.verify_and_export_editor_session_cancellable(
            &mut reopened,
            &replay_path,
            &std::sync::atomic::AtomicBool::new(false),
            &mut |_| {},
        )?;
        let imported =
            plan.import_replay_file(&replay_path, &directory.path().join("imported.ztas"), false)?;
        let mut rebuilt_editor = open_editor(imported, directory.path(), cache)?;
        let mut rebuilt = provider.load_editor_engine(rebuilt_editor.project())?;
        let rebuilt_outcome = rebuilt.seek(&mut rebuilt_editor, 4)?;
        assert_eq!(rebuilt_outcome.restored_cache_cursor, None);
        assert_eq!(rebuilt_outcome.executed_frames, 4);
        assert_eq!(observe(rebuilt.backend())?, observe(engine.backend())?);
    }
    Ok(())
}

#[test]
fn native_md_owned_editor_refuses_corrupt_foreign_project_profile_and_source_without_mutation()
-> Result<()> {
    let directory = crate::test_support::test_directory("md-owned-editor-refusals")?;
    let path = directory.path().join("parallel.md");
    let bytes = guest::rom();
    std::fs::write(&path, &bytes)?;
    let loader = DirectMdTasExecutionLoader::new(path.clone(), MdTasProfile::ThreeButton);
    let cache = TasSeekStateCache::open(directory.path().join("cache"))?;
    let mut editor = open_editor(movie(&loader)?, directory.path(), cache.clone())?;
    let mut engine = loader.load_editor_engine(editor.project())?;
    engine.seek(&mut editor, 2)?;
    let prior = observe(engine.backend())?;
    let project = editor.project().encode()?;
    cache.corrupt_md_newest_for_test()?;
    assert!(engine.seek(&mut editor, 4).is_err());
    assert_eq!(observe(engine.backend())?, prior);
    assert_eq!(editor.cursor(), 2);
    assert_eq!(editor.project().encode()?, project);
    let mut invalid = frame(MdTasProfile::ThreeButton, false);
    invalid.players[0].buttons |= 0x10;
    editor.edit_transaction(|edit| edit.set_input_range("main", 0, 1, invalid))?;
    let invalid_project = editor.project().encode()?;
    assert!(engine.seek(&mut editor, 4).is_err());
    assert_eq!(observe(engine.backend())?, prior);
    assert_eq!(editor.project().encode()?, invalid_project);
    editor.edit_transaction(|edit| {
        edit.set_input_range("main", 0, 1, frame(MdTasProfile::ThreeButton, false))
    })?;
    let wrong = DirectMdTasExecutionLoader::new(path.clone(), MdTasProfile::SixButton);
    assert!(wrong.load_editor_engine(editor.project()).is_err());
    assert_eq!(observe(engine.backend())?, prior);
    let mut changed = bytes.clone();
    changed[0x3ff] ^= 1;
    std::fs::write(&path, changed)?;
    assert!(engine.seek(&mut editor, 4).is_err());
    assert_eq!(observe(engine.backend())?, prior);
    std::fs::write(&path, bytes)?;
    let mut first = loader.load_fresh_backend()?;
    let EmuBackend::Md(first) = &mut first else {
        unreachable!()
    };
    let first_owner = first.begin_tas_editor()?;
    assert!(first.begin_tas_editor().is_err());
    let foreign = first.capture_tas_editor(&first_owner)?;
    let mut second = loader.load_fresh_backend()?;
    let before = observe(&second)?;
    let EmuBackend::Md(md) = &mut second else {
        unreachable!()
    };
    let second_owner = md.begin_tas_editor()?;
    let baseline = md.capture_tas_editor(&second_owner)?;
    assert!(md.restore_tas_editor(&first_owner, &foreign).is_err());
    assert!(md.restore_tas_editor(&second_owner, &foreign).is_err());
    assert_eq!(observe(&second)?, before);
    let identity = editor.project().seek_cache_identity("main", 0)?;
    assert!(
        cache
            .store_md(identity, foreign, &second_owner, &baseline, &second)
            .is_err()
    );
    assert_eq!(observe(&second)?, before);
    Ok(())
}

#[test]
fn native_md_owned_editor_cache_uses_existing_budget_and_counts_shared_and_deep_payloads()
-> Result<()> {
    let directory = crate::test_support::test_directory("md-owned-editor-budget")?;
    let path = directory.path().join("parallel.md");
    let rom = guest::rom();
    std::fs::write(&path, &rom)?;
    let loader = DirectMdTasExecutionLoader::new(path, MdTasProfile::SixButton);
    let limits = TasSeekCacheLimits {
        max_entries: 1,
        ..Default::default()
    };
    let cache = TasSeekStateCache::open_with_limits(directory.path().join("cache"), limits)?;
    let mut project = movie(&loader)?;
    project.edit_transaction(|edit| edit.insert_frames("main", 4, 8))?;
    let mut editor = open_editor(project, directory.path(), cache.clone())?;
    let mut engine = loader.load_editor_engine(editor.project())?;
    assert!(engine.seek(&mut editor, 1)?.cache_store_error.is_none());
    assert_eq!(engine.seek(&mut editor, 2)?.restored_cache_cursor, Some(1));
    let (entries, bytes, payload) = cache.md_retained_for_test()?;
    assert_eq!(entries, 1);
    assert!(bytes <= limits.max_bytes && bytes > payload.requested_payload_bytes());
    assert_eq!(payload.count(MdAllocationKind::CoreLayout), 3);
    assert!(payload.arc_allocations() > 0);
    assert_eq!(payload.bytes(MdAllocationKind::Rom), rom.len() as u64);
    assert!(payload.bytes(MdAllocationKind::Framebuffer) > 0);
    assert!(payload.bytes(MdAllocationKind::AudioSamples) > 0);
    let evicted = engine.seek(&mut editor, 1)?;
    assert_eq!(evicted.restored_cache_cursor, None);
    assert_eq!(evicted.executed_frames, 1);
    assert_eq!(
        observe(engine.backend())?,
        observe(&control(&loader, editor.project(), 1)?)?
    );
    let long = engine.seek(&mut editor, 12)?;
    assert_eq!(long.restored_cache_cursor, Some(1));
    assert_eq!(long.executed_frames, 11);
    let EmuBackend::Md(md) = engine.backend() else {
        unreachable!()
    };
    assert_eq!(md.core().pending_audio_frames(), 4096);
    assert_eq!(
        observe(engine.backend())?,
        observe(&control(&loader, editor.project(), 12)?)?
    );
    Ok(())
}
