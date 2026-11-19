use super::*;
use crate::emu_backend::loader::{DirectMdTasExecutionLoader, MdTasProfile};
use crate::netplay::md_fixture as guest;
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasDigest, TasSeekStateCache, TasVerificationCheckpoint,
    TasVerificationProvenance,
};

#[test]
fn md_failed_checkpoint_after_advancing_restores_full_preseek_runtime_and_project() -> Result<()> {
    let directory = crate::test_support::test_directory("md-editor-failed-seek")?;
    let rom = directory.path().join("parallel.md");
    std::fs::write(&rom, guest::rom())?;
    let loader = DirectMdTasExecutionLoader::new(rom, MdTasProfile::ThreeButton);
    let mut project = loader.create_project()?;
    project.edit_transaction(|edit| edit.insert_frames("main", 1, 3))?;
    project.branches[0].verification = Some(TasVerificationProvenance {
        branch_movie_sha256: project.branch_movie_sha256("main")?,
        checkpoints: vec![TasVerificationCheckpoint {
            cursor: 3,
            state_sha256: TasDigest([0xff; 32]),
        }],
        final_state_sha256: None,
    });
    project.validate()?;
    let path = directory.path().join("movie.ztas");
    let autosave = TasAutosaveStore::beside_manual_save(&path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("cache"))?;
    let mut editor = TasEditorSession::new(project, path, autosave, cache)?;
    let mut engine = loader.load_editor_engine(editor.project())?;
    engine.seek(&mut editor, 2)?;
    let EmuBackend::Md(md) = engine.backend() else {
        unreachable!()
    };
    let raw = engine.backend().encode_state_bytes()?;
    let full = md.core().encode_debugger_checkpoint()?;
    let config = zeff_md_core::emulator::rollback::MdRollbackConfig::capture(md.core())?;
    let hashes = config.boundary_hashes(md.core())?;
    let host = md.tas_editor_host_witness_for_test()?;
    let project = editor.project().encode()?;
    let error = engine.seek(&mut editor, 4).unwrap_err();
    assert!(error.to_string().contains("checkpoint cursor 3"));
    let EmuBackend::Md(md) = engine.backend() else {
        unreachable!()
    };
    assert_eq!(engine.backend().encode_state_bytes()?, raw);
    assert_eq!(md.core().encode_debugger_checkpoint()?, full);
    assert_eq!(config.boundary_hashes(md.core())?, hashes);
    assert_eq!(md.tas_editor_host_witness_for_test()?, host);
    assert_eq!(editor.cursor(), 2);
    assert_eq!(editor.project().encode()?, project);
    Ok(())
}
