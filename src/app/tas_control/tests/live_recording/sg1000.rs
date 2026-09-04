use super::*;

#[test]
fn direct_sg1000_live_recording_forwards_only_two_standard_pads() {
    let root = crate::test_support::test_directory("tas-live-record-sg1000").unwrap();
    let session = live_session(root.path());
    let mut coordinator = TasControlCoordinator::new();
    let run_id = awaiting_decision(&mut coordinator, 82);
    let TasControlState::AwaitingDecision { project, .. } = &mut coordinator.state else {
        panic!("expected awaiting decision");
    };
    project.profile = TasExecutionProfile::DirectSg1000Cartridge;

    let invalid = session
        .prepare_live_frame(crate::tas_project::TasInputFrame {
            players: [
                crate::tas_project::TasControllerInput {
                    buttons: 0x04,
                    dpad: 0,
                },
                crate::tas_project::TasControllerInput::default(),
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
                    buttons: 0x01,
                    dpad: 0x04,
                },
                crate::tas_project::TasControllerInput {
                    buttons: 0x02,
                    dpad: 0x08,
                },
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
            if request.profile == TasExecutionProfile::DirectSg1000Cartridge
                && request.lease_id == 82
                && request.run_id == run_id
                && request.input.p1_buttons == 0x01
                && request.input.p1_dpad == 0x04
                && request.input.p2_buttons == 0x02
                && request.input.p2_dpad == 0x08
                && request.input.coleco == [crate::tas_project::TasColecoControllerInput::default(); 2]
                && request.input.zapper == Default::default()
    ));
}
