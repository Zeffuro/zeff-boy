use super::*;

#[test]
fn direct_game_gear_live_recording_forwards_only_p1_pad_and_start() {
    let root = crate::test_support::test_directory("tas-live-record-game-gear").unwrap();
    let session = live_session(root.path());
    let mut coordinator = TasControlCoordinator::new();
    let run_id = awaiting_decision(&mut coordinator, 81);
    let TasControlState::AwaitingDecision { project, .. } = &mut coordinator.state else {
        panic!("expected awaiting decision");
    };
    project.profile = TasExecutionProfile::DirectGameGearCartridge;

    let invalid = session
        .prepare_live_frame(TasInputFrame {
            players: [
                crate::tas_project::TasControllerInput {
                    buttons: 0x09,
                    dpad: 0x04,
                },
                crate::tas_project::TasControllerInput {
                    buttons: 1,
                    dpad: 0,
                },
                crate::tas_project::TasControllerInput::default(),
                crate::tas_project::TasControllerInput::default(),
                crate::tas_project::TasControllerInput::default(),
            ],
            ..TasInputFrame::default()
        })
        .unwrap();
    assert!(coordinator.begin_live_frame_advance(invalid).is_err());

    let prepared = session
        .prepare_live_frame(TasInputFrame {
            players: [
                crate::tas_project::TasControllerInput {
                    buttons: 0x09,
                    dpad: 0x04,
                },
                crate::tas_project::TasControllerInput::default(),
                crate::tas_project::TasControllerInput::default(),
                crate::tas_project::TasControllerInput::default(),
                crate::tas_project::TasControllerInput::default(),
            ],
            ..TasInputFrame::default()
        })
        .unwrap();
    let command = coordinator.begin_live_frame_advance(prepared).unwrap();
    assert!(matches!(
        command.into_parts_for_worker(WORKER_GENERATION),
        Some((WORKER_GENERATION, EmuCommand::AdvanceTasControl(request)))
            if request.profile == TasExecutionProfile::DirectGameGearCartridge
                && request.lease_id == 81
                && request.run_id == run_id
                && request.input.p1_buttons == 0x09
                && request.input.p1_dpad == 0x04
                && request.input.p2_buttons == 0
                && request.input.p2_dpad == 0
    ));
}
