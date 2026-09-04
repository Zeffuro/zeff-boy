use super::*;

#[test]
fn branch_scope_rejects_non_p1_input_events_and_nondefault_start_metadata() -> Result<()> {
    let (_directory, source_path, _) = write_direct_rom("tas-direct-gb-branch")?;
    let loader = DirectGbTasExecutionLoader::new(source_path, Vec::new());
    let project = loader.create_project()?;
    let mut p2 = project.clone();
    p2.edit_transaction(|edit| {
        edit.set_input_range(
            "main",
            0,
            1,
            TasInputFrame {
                players: [
                    TasControllerInput::default(),
                    TasControllerInput {
                        buttons: 1,
                        dpad: 0,
                    },
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                ],
                ..TasInputFrame::default()
            },
        )
    })?;
    assert!(DirectGbTasExecutionLoader::validate_project_branch_scope(&p2, "main").is_err());

    let mut high_bits = project.clone();
    high_bits.edit_transaction(|edit| {
        edit.set_input_range(
            "main",
            0,
            1,
            TasInputFrame {
                players: [
                    TasControllerInput {
                        buttons: 0x10,
                        dpad: 0,
                    },
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                ],
                ..TasInputFrame::default()
            },
        )
    })?;
    assert!(DirectGbTasExecutionLoader::validate_project_branch_scope(&high_bits, "main").is_err());

    let mut special_input = project.clone();
    special_input.edit_transaction(|edit| {
        edit.set_input_range(
            "main",
            0,
            1,
            TasInputFrame {
                zapper: crate::tas_project::TasZapperInput {
                    enabled: true,
                    ..crate::tas_project::TasZapperInput::default()
                },
                ..TasInputFrame::default()
            },
        )
    })?;
    assert!(
        DirectGbTasExecutionLoader::validate_project_branch_scope(&special_input, "main").is_err()
    );

    let mut event = project.clone();
    event.edit_transaction(|edit| {
        edit.replace_branch_events("main", vec![ReplayEvent::FdsDiskSide { frame: 0, side: 0 }])
    })?;
    assert!(DirectGbTasExecutionLoader::validate_project_branch_scope(&event, "main").is_err());

    let identity = project.identity().clone();
    let start_state = project.start_state().to_vec();
    let replay_start = ReplayStartMetadata {
        game_boy_link_tick: Some(0),
        ..ReplayStartMetadata::default()
    };
    let linked = TasProject::new(
        "linked",
        identity,
        start_state,
        replay_start,
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count: 1,
            input_spans: Vec::new(),
            events: Vec::new(),
        },
        BTreeMap::new(),
    )?;
    assert!(DirectGbTasExecutionLoader::validate_project_branch_scope(&linked, "main").is_err());
    Ok(())
}
