use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::emu_backend::loader::DirectNesTasExecutionLoader;
use crate::tas_project::{TasExecutionWitness, TasProject};

use super::HeadlessOptions;

pub(super) fn run_tas_project_headless(
    rom_source_path: &Path,
    firmware_search_dirs: Vec<PathBuf>,
    opts: &HeadlessOptions,
) -> Result<()> {
    let project_path = opts
        .tas_project_path
        .as_deref()
        .context("missing --tas-verify project path")?;
    let mut project = TasProject::load(project_path)
        .with_context(|| format!("failed to load TAS project {}", project_path.display()))?;
    let branch_id = opts
        .tas_branch_id
        .as_deref()
        .unwrap_or(project.active_branch_id())
        .to_owned();
    DirectNesTasExecutionLoader::validate_project_branch_scope(&project, &branch_id)?;

    let plan =
        DirectNesTasExecutionLoader::new(rom_source_path.to_path_buf(), firmware_search_dirs);
    let start_state = project.start_state().to_vec();
    let witness_session = plan.load_session(&start_state)?;
    let witness = TasExecutionWitness {
        identity: witness_session.identity().clone(),
    };
    let verification = project
        .verify_branch_with_factory(&branch_id, &witness, || plan.load_session(&start_state))?;

    project.save_atomic(project_path).with_context(|| {
        format!(
            "failed to save verified TAS project {}",
            project_path.display()
        )
    })?;
    println!(
        "[tas] verify project={} branch={} frames={} checkpoints={} final_state_sha256={} status=verified",
        project_path.display(),
        branch_id,
        project
            .branch(&branch_id)
            .expect("verified branch still exists")
            .frame_count(),
        verification.checkpoints.len(),
        verification
            .final_state_sha256
            .map_or_else(|| "none".to_owned(), |digest| digest.to_hex()),
    );
    println!("[tas] project_saved={}", project_path.display());

    if let Some(export_path) = opts.tas_export_path.as_deref() {
        project.export_verified_zrpl_with_factory(&branch_id, export_path, &witness, || {
            plan.load_session(&start_state)
        })?;
        println!(
            "[tas] export status=exported output={}",
            export_path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use zeff_emu_common::replay::{ReplayPlayer, ReplayStartMetadata};

    use crate::emu_backend::loader::{
        MAX_NES_CARTRIDGE_BYTES, direct_nes_tas_identity, read_nes_cartridge_bounded,
    };
    use crate::emu_backend::{
        ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
    };
    use crate::tas_project::{
        TasAnnotation, TasCameraInput, TasControllerInput, TasInitialBranch, TasInputFrame,
        TasInputSpan, TasMarker,
    };

    use super::*;
    use crate::test_support::test_directory;

    fn executable_nes_rom() -> Vec<u8> {
        let mut rom = vec![0; 16 + 0x4000 + 0x2000];
        rom[..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[5] = 1;
        let prg = 16;
        rom[prg] = 0xA9;
        rom[prg + 1] = 0x42;
        rom[prg + 2] = 0x85;
        rom[prg + 3] = 0x00;
        rom[prg + 4] = 0x4C;
        rom[prg + 5] = 0x04;
        rom[prg + 6] = 0x80;
        rom[prg + 0x3FFC] = 0x00;
        rom[prg + 0x3FFD] = 0x80;
        rom
    }

    fn project_for_rom(rom_path: &Path, rom: &[u8], frames: u64) -> Result<TasProject> {
        let backend = load_backend_from_rom_source(
            ActiveSystem::Nes,
            rom_path,
            rom_path,
            Some(rom.to_vec()),
            BackendLoadConfig::default(),
        )?
        .backend;
        let start_state = backend.encode_state_bytes()?;
        project_for_rom_with_start_state(rom, frames, &backend, start_state)
    }

    fn project_for_rom_with_start_state(
        rom: &[u8],
        frames: u64,
        backend: &EmuBackend,
        start_state: Vec<u8>,
    ) -> Result<TasProject> {
        let identity = direct_nes_tas_identity(backend, rom, &start_state)?;
        TasProject::new(
            "cli-verification",
            identity,
            start_state,
            ReplayStartMetadata::default(),
            TasInitialBranch {
                id: "main".to_owned(),
                name: "Main".to_owned(),
                frame_count: frames,
                input_spans: vec![TasInputSpan {
                    start: 0,
                    length: frames,
                    input: TasInputFrame {
                        players: [
                            TasControllerInput {
                                buttons: 1,
                                dpad: 0,
                            },
                            TasControllerInput::default(),
                            TasControllerInput::default(),
                            TasControllerInput::default(),
                            TasControllerInput::default(),
                        ],
                        ..TasInputFrame::default()
                    },
                }],
                events: Vec::new(),
            },
            BTreeMap::new(),
        )
    }

    #[test]
    fn native_cli_verifies_saves_then_exports_with_loader_owned_identity() -> Result<()> {
        let directory = test_directory("tas-cli")?;
        let rom_path = directory.path().join("game.nes");
        let project_path = directory.path().join("movie.ztas");
        let export_path = directory.path().join("movie.zrpl");
        let rom = executable_nes_rom();
        std::fs::write(&rom_path, &rom)?;
        project_for_rom(&rom_path, &rom, 301)?.save_atomic(&project_path)?;

        run_tas_project_headless(
            &rom_path,
            Vec::new(),
            &HeadlessOptions {
                tas_project_path: Some(project_path.clone()),
                tas_export_path: Some(export_path.clone()),
                ..HeadlessOptions::default()
            },
        )?;

        let saved = TasProject::load(&project_path)?;
        assert!(saved.verification_is_current("main")?);
        let verification = saved.branches()[0]
            .verification()
            .expect("verification should be persisted");
        assert_eq!(
            verification
                .checkpoints
                .iter()
                .map(|checkpoint| checkpoint.cursor)
                .collect::<Vec<_>>(),
            vec![300]
        );
        let replay = ReplayPlayer::load(&export_path)?;
        assert_eq!(replay.total_frames(), 301);
        assert_eq!(replay.metadata().checkpoints.len(), 1);
        assert_eq!(
            replay.metadata().final_state_sha256,
            verification.final_state_sha256.map(|digest| digest.0)
        );
        Ok(())
    }

    #[test]
    fn native_cli_rejects_media_change_without_mutating_project() -> Result<()> {
        let directory = test_directory("tas-cli-interrupted")?;
        let rom_path = directory.path().join("game.nes");
        let project_path = directory.path().join("movie.ztas");
        let rom = executable_nes_rom();
        std::fs::write(&rom_path, &rom)?;
        project_for_rom(&rom_path, &rom, 1)?.save_atomic(&project_path)?;
        let before = std::fs::read(&project_path)?;
        let mut changed = rom;
        changed[16] ^= 1;
        std::fs::write(&rom_path, changed)?;

        let result = run_tas_project_headless(
            &rom_path,
            Vec::new(),
            &HeadlessOptions {
                tas_project_path: Some(project_path.clone()),
                ..HeadlessOptions::default()
            },
        );
        assert!(result.is_err());
        assert_eq!(std::fs::read(project_path)?, before);
        Ok(())
    }

    #[test]
    fn public_tas_mutation_api_accounts_movie_and_presentation_edits() -> Result<()> {
        let directory = test_directory("tas-cli-edit-boundary")?;
        let rom_path = directory.path().join("game.nes");
        let rom = executable_nes_rom();
        let mut project = project_for_rom(&rom_path, &rom, 1)?;

        assert_eq!(project.edit_generation(), 0);
        assert_eq!(project.rerecord_count(), 0);
        project.edit_transaction(|edit| {
            edit.set_project_comment("presentation-only");
            edit.replace_markers(vec![TasMarker {
                id: "public-marker".to_owned(),
                branch_id: "main".to_owned(),
                cursor: 1,
                name: "End".to_owned(),
            }]);
            edit.replace_annotations(vec![TasAnnotation {
                id: "public-annotation".to_owned(),
                branch_id: "main".to_owned(),
                start: 0,
                length: 1,
                kind: "note".to_owned(),
                text: "Transaction-owned".to_owned(),
            }]);
            Ok(())
        })?;
        assert_eq!(project.project_comment(), "presentation-only");
        assert_eq!(project.markers()[0].id, "public-marker");
        assert_eq!(project.annotations()[0].id, "public-annotation");
        assert_eq!(project.edit_generation(), 1);
        assert_eq!(project.rerecord_count(), 0);

        project.edit_transaction(|edit| {
            edit.set_input_range("main", 0, 1, TasInputFrame::default())
        })?;
        assert_eq!(
            project.branch("main").unwrap().input_at(0),
            TasInputFrame::default()
        );
        assert_eq!(project.edit_generation(), 2);
        assert_eq!(project.rerecord_count(), 1);
        Ok(())
    }

    #[test]
    fn public_camera_asset_edits_commit_or_roll_back_with_their_movie() -> Result<()> {
        let directory = test_directory("tas-cli-camera-edit-boundary")?;
        let rom_path = directory.path().join("game.nes");
        let rom = executable_nes_rom();
        let mut project = project_for_rom(&rom_path, &rom, 1)?;
        let mut input = project.branch("main").unwrap().input_at(0);
        let mut digest = None;

        project.edit_transaction(|edit| {
            let asset = edit.insert_camera_asset(vec![1, 2, 3, 4]);
            digest = Some(asset);
            input.camera = TasCameraInput::Blob(asset);
            edit.set_input_range("main", 0, 1, input)
        })?;
        let digest = digest.expect("transaction should return its camera digest");
        assert_eq!(
            project.assets().get(&digest).map(Vec::as_slice),
            Some(&[1, 2, 3, 4][..])
        );
        assert_eq!(project.edit_generation(), 1);
        assert_eq!(project.rerecord_count(), 1);

        let before = project.clone();
        assert!(
            project
                .edit_transaction(|edit| {
                    assert!(edit.remove_camera_asset(digest));
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(project, before);
        Ok(())
    }

    #[test]
    fn native_cli_rejects_legacy_or_nonstandard_controller_start_state_transactionally()
    -> Result<()> {
        let directory = test_directory("tas-cli-profile")?;
        let rom_path = directory.path().join("game.nes");
        let rom = executable_nes_rom();
        std::fs::write(&rom_path, &rom)?;

        for name in ["legacy", "oversized", "zapper"] {
            let mut backend = load_backend_from_rom_source(
                ActiveSystem::Nes,
                &rom_path,
                &rom_path,
                Some(rom.clone()),
                BackendLoadConfig::default(),
            )?
            .backend;
            let start_state = if name == "legacy" {
                let mut start_state = backend.encode_state_bytes()?;
                start_state[8..12].copy_from_slice(&10_u32.to_le_bytes());
                start_state
            } else if name == "oversized" {
                let mut start_state = Vec::new();
                start_state.extend_from_slice(&zeff_nes_core::save_state::NES_SAVE_STATE_MAGIC);
                start_state.extend_from_slice(
                    &zeff_nes_core::save_state::NES_SAVE_STATE_FORMAT_VERSION.to_le_bytes(),
                );
                start_state.extend_from_slice(&u32::MAX.to_le_bytes());
                start_state
            } else {
                backend.set_zapper_state(true, false, false, None);
                backend.encode_state_bytes().expect("state should encode")
            };
            let project = project_for_rom_with_start_state(&rom, 1, &backend, start_state)?;
            let project_path = directory.path().join(format!("{name}.ztas"));
            project.save_atomic(&project_path)?;
            let before = std::fs::read(&project_path)?;

            let result = run_tas_project_headless(
                &rom_path,
                Vec::new(),
                &HeadlessOptions {
                    tas_project_path: Some(project_path.clone()),
                    ..HeadlessOptions::default()
                },
            );
            assert!(result.is_err(), "{name} start state should be rejected");
            assert_eq!(std::fs::read(project_path)?, before);
        }
        Ok(())
    }

    #[test]
    fn native_cli_rejects_non_direct_media_without_mutating_project() -> Result<()> {
        let directory = test_directory("tas-cli-fds")?;
        let rom_path = directory.path().join("game.nes");
        let disguised_path = directory.path().join("game.fds");
        let project_path = directory.path().join("movie.ztas");
        let rom = executable_nes_rom();
        std::fs::write(&rom_path, &rom)?;
        std::fs::write(&disguised_path, &rom)?;
        project_for_rom(&rom_path, &rom, 1)?.save_atomic(&project_path)?;
        let before = std::fs::read(&project_path)?;

        let result = run_tas_project_headless(
            &disguised_path,
            Vec::new(),
            &HeadlessOptions {
                tas_project_path: Some(project_path.clone()),
                ..HeadlessOptions::default()
            },
        );
        assert!(result.is_err());
        assert_eq!(std::fs::read(project_path)?, before);
        Ok(())
    }

    #[test]
    fn bounded_nes_media_reader_rejects_oversized_file() -> Result<()> {
        let directory = test_directory("tas-cli-oversized")?;
        let path = directory.path().join("oversized.nes");
        std::fs::File::create(&path)?.set_len(MAX_NES_CARTRIDGE_BYTES + 1)?;
        assert!(read_nes_cartridge_bounded(&path).is_err());
        Ok(())
    }

    #[test]
    fn canonical_nes_tas_loader_ignores_battery_sidecar() -> Result<()> {
        let directory = test_directory("tas-cli-isolated")?;
        let isolated_path = directory.path().join("isolated.nes");
        let baseline_path = directory.path().join("baseline.nes");
        let mut rom = executable_nes_rom();
        rom[6] = 0x02;
        rom[7] = 0x10;
        rom[8] = 1;
        std::fs::write(&isolated_path, &rom)?;
        std::fs::write(&baseline_path, &rom)?;

        let mut persistent_source = zeff_nes_core::emulator::Emulator::new(
            &rom,
            zeff_nes_core::emulator::DEFAULT_SAMPLE_RATE,
        )?;
        persistent_source.load_persistent_data(&vec![0xA5; 8 * 1024])?;
        let sidecar = persistent_source
            .dump_persistent_data()
            .context("battery fixture should expose persistent data")?;
        std::fs::write(isolated_path.with_extension("sav"), sidecar)?;

        let baseline = load_backend_from_rom_source(
            ActiveSystem::Nes,
            &baseline_path,
            &baseline_path,
            Some(rom.clone()),
            BackendLoadConfig::default(),
        )?
        .backend
        .encode_state_bytes()?;
        let isolated = load_backend_from_rom_source(
            ActiveSystem::Nes,
            &isolated_path,
            &isolated_path,
            Some(rom),
            BackendLoadConfig {
                nes_load_battery_sram: false,
                ..BackendLoadConfig::default()
            },
        )?
        .backend
        .encode_state_bytes()?;
        assert_eq!(isolated, baseline);
        Ok(())
    }

    #[test]
    fn nes_cartridge_scope_rejects_every_unowned_timeline_domain() -> Result<()> {
        let directory = test_directory("tas-cli-failure")?;
        let rom_path = directory.path().join("game.nes");
        let rom = executable_nes_rom();
        let base = project_for_rom(&rom_path, &rom, 1)?;

        let mut cases = Vec::new();
        let mut project = base.clone();
        let mut input = project.branch("main").unwrap().input_at(0);
        input.players[2].buttons = 1;
        project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
        cases.push(project);
        let mut project = base.clone();
        let mut input = project.branch("main").unwrap().input_at(0);
        input.zapper.enabled = true;
        project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
        cases.push(project);
        let mut project = base.clone();
        let mut input = project.branch("main").unwrap().input_at(0);
        input.tilt_x_bits = 1;
        project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
        cases.push(project);
        let mut project = base.clone();
        let mut camera_input = project.branch("main").unwrap().input_at(0);
        project.edit_transaction(|edit| {
            camera_input.camera = TasCameraInput::Blob(edit.insert_camera_asset(vec![1]));
            edit.set_input_range("main", 0, 1, camera_input)
        })?;
        cases.push(project);
        let mut project = base.clone();
        project.edit_transaction(|edit| {
            edit.replace_branch_events(
                "main",
                vec![zeff_emu_common::replay::ReplayEvent::FdsDiskSide { frame: 0, side: 0 }],
            )
        })?;
        cases.push(project);
        let mut replay_start = base.replay_start().clone();
        replay_start.wonder_swan_link_tick = Some(0);
        let branch = base.branch("main").unwrap();
        let project = TasProject::new(
            base.project_id(),
            base.identity().clone(),
            base.start_state().to_vec(),
            replay_start,
            TasInitialBranch {
                id: branch.id().to_owned(),
                name: branch.name().to_owned(),
                frame_count: branch.frame_count(),
                input_spans: branch.input_spans().to_vec(),
                events: branch.events().to_vec(),
            },
            base.assets().clone(),
        )?;
        cases.push(project);

        for project in cases {
            assert!(
                DirectNesTasExecutionLoader::validate_project_branch_scope(&project, "main")
                    .is_err()
            );
        }
        Ok(())
    }

    #[test]
    fn export_failure_leaves_durable_verification() -> Result<()> {
        let directory = test_directory("tas-cli-occupied-export")?;
        let rom_path = directory.path().join("game.nes");
        let project_path = directory.path().join("movie.ztas");
        let export_path = directory.path().join("occupied.zrpl");
        let rom = executable_nes_rom();
        std::fs::write(&rom_path, &rom)?;
        project_for_rom(&rom_path, &rom, 1)?.save_atomic(&project_path)?;
        std::fs::write(&export_path, b"keep")?;

        let result = run_tas_project_headless(
            &rom_path,
            Vec::new(),
            &HeadlessOptions {
                tas_project_path: Some(project_path.clone()),
                tas_export_path: Some(export_path.clone()),
                ..HeadlessOptions::default()
            },
        );
        let error = result.expect_err("an occupied replay export path must fail after saving");
        assert!(
            format!("{error:#}").contains("refusing to overwrite existing replay"),
            "unexpected export failure: {error:#}"
        );
        assert!(TasProject::load(&project_path)?.verification_is_current("main")?);
        assert_eq!(std::fs::read(export_path)?, b"keep");
        Ok(())
    }
}
