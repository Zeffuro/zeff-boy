use super::*;
use crate::emu_backend::loader::{DirectMdTasExecutionLoader, MdTasProfile};
use crate::tas_project::{TasControllerInput, TasInputFrame};
use zeff_emu_common::replay::ReplayPlayer;

#[test]
fn native_cli_md_two_pad_direct_and_selected_zip_projects_verify_and_export_both_profiles()
-> Result<()> {
    for pad in [MdTasProfile::ThreeButton, MdTasProfile::SixButton] {
        for zipped in [false, true] {
            let directory = crate::test_support::test_directory("md-cli-product")?;
            let source = directory
                .path()
                .join(if zipped { "game.zip" } else { "game.md" });
            let bytes = crate::netplay::md_fixture::rom();
            let loader = if zipped {
                crate::test_support::write_zip(
                    &source,
                    &[("first.md", &bytes), ("selected/game.gen", &bytes)],
                )?;
                DirectMdTasExecutionLoader::new_zip(
                    source.clone(),
                    source.join("selected/game.gen"),
                    pad,
                )
            } else {
                std::fs::write(&source, bytes)?;
                DirectMdTasExecutionLoader::new(source.clone(), pad)
            };
            let project_path = directory.path().join("movie.ztas");
            let replay_path = directory.path().join("movie.zrpl");
            let mut project = loader.create_project_file(&project_path)?;
            let (p1, p2) = if pad == MdTasProfile::ThreeButton {
                (0x41, 0x0a)
            } else {
                (0x15, 0x2e)
            };
            let mut input = TasInputFrame::default();
            input.players[0] = TasControllerInput {
                buttons: p1,
                dpad: 1,
            };
            input.players[1] = TasControllerInput {
                buttons: p2,
                dpad: 8,
            };
            project.edit_transaction(|edit| {
                edit.insert_frames("main", 1, 1)?;
                edit.set_input_range("main", 0, 2, input)
            })?;
            project.save_atomic(&project_path)?;
            run_tas_project_headless(
                &source,
                Vec::new(),
                &HeadlessOptions {
                    tas_project_path: Some(project_path.clone()),
                    tas_export_path: Some(replay_path.clone()),
                    ..Default::default()
                },
            )?;
            let saved = TasProject::load(&project_path)?;
            assert!(saved.verification_is_current("main")?);
            assert_eq!(saved.identity(), project.identity());
            let replay = ReplayPlayer::load(&replay_path)?;
            assert_eq!(replay.total_frames(), 2);
            let frames = replay.peek_joypad_frames(0, 2);
            assert!(frames.iter().all(|frame| frame.buttons == p1
                && frame.dpad == 1
                && frame.buttons_p2 == p2
                && frame.dpad_p2 == 8));
            let plan = select_private_tas_execution_loader_for_project(
                source.clone(),
                ActiveSystem::Md,
                Vec::new(),
                &saved,
            )?;
            let imported = plan.import_replay_file(
                &replay_path,
                &directory.path().join("imported.ztas"),
                false,
            )?;
            assert_eq!(imported.branch("main").unwrap().input_at(0), input);
        }
    }
    Ok(())
}

#[test]
fn native_cli_md_source_replacement_refuses_without_project_or_replay_publication() -> Result<()> {
    let directory = crate::test_support::test_directory("md-cli-media-change")?;
    let source = directory.path().join("game.md");
    let mut bytes = crate::netplay::md_fixture::rom();
    std::fs::write(&source, &bytes)?;
    let loader = DirectMdTasExecutionLoader::new(source.clone(), MdTasProfile::ThreeButton);
    let project_path = directory.path().join("movie.ztas");
    loader.create_project_file(&project_path)?;
    let before = std::fs::read(&project_path)?;
    let replay_path = directory.path().join("rejected.zrpl");
    bytes[0x3ff] ^= 1;
    std::fs::write(&source, bytes)?;
    assert!(
        run_tas_project_headless(
            &source,
            Vec::new(),
            &HeadlessOptions {
                tas_project_path: Some(project_path.clone()),
                tas_export_path: Some(replay_path.clone()),
                ..Default::default()
            }
        )
        .is_err()
    );
    assert_eq!(std::fs::read(project_path)?, before);
    assert!(!replay_path.exists());
    Ok(())
}
