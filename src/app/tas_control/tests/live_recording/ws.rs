use super::*;

#[test]
fn direct_wonderswan_live_recording_forwards_only_p1_controls() {
    let root = crate::test_support::test_directory("tas-live-record-wonderswan").unwrap();
    let session = live_session(root.path());
    let mut coordinator = TasControlCoordinator::new();
    let run_id = awaiting_decision(&mut coordinator, 85);
    let TasControlState::AwaitingDecision { project, .. } = &mut coordinator.state else {
        panic!("expected awaiting decision");
    };
    project.profile = TasExecutionProfile::DirectWsCartridge;

    let invalid = session
        .prepare_live_frame(crate::tas_project::TasInputFrame {
            players: [
                crate::tas_project::TasControllerInput {
                    buttons: 0x04,
                    dpad: 0,
                },
                crate::tas_project::TasControllerInput {
                    buttons: 1,
                    dpad: 0,
                },
                crate::tas_project::TasControllerInput::default(),
                crate::tas_project::TasControllerInput::default(),
                crate::tas_project::TasControllerInput::default(),
            ],
            ..Default::default()
        })
        .unwrap();
    assert!(coordinator.begin_live_frame_advance(invalid).is_err());

    let prepared = session
        .prepare_live_frame(crate::tas_project::TasInputFrame {
            players: [
                crate::tas_project::TasControllerInput {
                    buttons: 0xB9,
                    dpad: 0x06,
                },
                crate::tas_project::TasControllerInput::default(),
                crate::tas_project::TasControllerInput::default(),
                crate::tas_project::TasControllerInput::default(),
                crate::tas_project::TasControllerInput::default(),
            ],
            ..Default::default()
        })
        .unwrap();
    let command = coordinator.begin_live_frame_advance(prepared).unwrap();
    assert!(matches!(
        command.into_parts_for_worker(WORKER_GENERATION),
        Some((WORKER_GENERATION, EmuCommand::AdvanceTasControl(request)))
            if request.profile == TasExecutionProfile::DirectWsCartridge
                && request.lease_id == 85
                && request.run_id == run_id
                && request.input.p1_buttons == 0xB9
                && request.input.p1_dpad == 0x06
                && request.input.p2_buttons == 0
                && request.input.p2_dpad == 0
                && request.input.coleco == [crate::tas_project::TasColecoControllerInput::default(); 2]
                && request.input.zapper == Default::default()
    ));
}
