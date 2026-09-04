use zeff_emu_common::replay::{
    ReplayEvent, ReplayGameBoyLinkAction, ReplayGameBoyLinkCoordinatorOwner,
    ReplayGameBoyLinkCoordinatorState, ReplayGameBoyLinkState, ReplayStartMetadata,
    ReplayWonderSwanLinkEvent, decode_replay_start_metadata, encode_replay_start_metadata,
};

use super::super::*;
use super::project;

#[test]
fn coleco_input_is_semantic_and_neutral_input_stays_compatible_with_existing_projects() {
    let neutral = TasInputFrame::default();
    let neutral_json = serde_json::to_value(neutral).unwrap();
    assert!(neutral_json.get("coleco").is_none());
    assert_eq!(
        serde_json::from_value::<TasInputFrame>(neutral_json).unwrap(),
        neutral
    );

    let semantic = TasInputFrame {
        coleco: [
            TasColecoControllerInput {
                right: true,
                right_button: true,
                keypad: TasColecoKeypadKey::Star,
                ..TasColecoControllerInput::default()
            },
            TasColecoControllerInput {
                left: true,
                left_button: true,
                keypad: TasColecoKeypadKey::Nine,
                ..TasColecoControllerInput::default()
            },
        ],
        ..TasInputFrame::default()
    };
    let encoded = serde_json::to_value(semantic).unwrap();
    assert_eq!(encoded["coleco"][0]["keypad"], "star");
    assert_eq!(encoded["coleco"][1]["keypad"], "nine");
    assert_eq!(
        serde_json::from_value::<TasInputFrame>(encoded).unwrap(),
        semantic
    );
}

#[test]
fn project_package_roundtrips_complete_timeline_domains() {
    let project = project();
    let bytes = project.encode().expect("project should encode");
    assert_eq!(project.encode().unwrap(), bytes);
    let decoded = TasProject::decode(&bytes).expect("project should decode");

    assert_eq!(decoded, project);
    assert_eq!(decoded.branches[0].input_at(1), TasInputFrame::default());
    assert_eq!(decoded.branches[0].input_at(2).players[4].dpad, 10);
    assert_eq!(decoded.branches[0].input_at(2).tilt_x_bits, 0x3E80_0000);
    assert!(decoded.branches[0].input_at(2).zapper.trigger);
}

#[test]
fn presentation_does_not_change_movie_hash_but_sync_identity_does() {
    let project = project();
    let original_sync = project.sync_identity_sha256().unwrap();
    let original_movie = project.branch_movie_sha256("main").unwrap();
    let mut changed = project.clone();
    changed.project_comment = "different".to_owned();
    changed.branches[0].name = "Renamed".to_owned();
    changed.branches[0].comment = "different".to_owned();
    changed.markers[0].name = "Renamed marker".to_owned();
    changed.annotations[0].text = "different".to_owned();
    changed.edit_generation += 1;
    changed.rerecord_count += 1;

    assert_eq!(changed.sync_identity_sha256().unwrap(), original_sync);
    assert_eq!(changed.branch_movie_sha256("main").unwrap(), original_movie);

    changed.identity.determinism_abi = "nes-sync-v2".to_owned();
    assert_ne!(changed.sync_identity_sha256().unwrap(), original_sync);
    assert_ne!(changed.branch_movie_sha256("main").unwrap(), original_movie);
}

#[test]
fn project_validation_rejects_branch_ancestry_cycles() {
    let mut project = project();
    project.branches[0].parent = Some(TasBranchOrigin {
        branch_id: "alternate".to_owned(),
        branch_movie_sha256: TasDigest([0x51; 32]),
        fork_cursor: 0,
    });
    assert!(project.validate().is_err());
}

#[test]
fn every_sync_identity_domain_changes_the_sync_hash() {
    let project = project();
    let original = project.sync_identity_sha256().unwrap();
    macro_rules! assert_changes {
        ($change:expr) => {{
            let mut changed = project.clone();
            $change(&mut changed);
            assert_ne!(changed.sync_identity_sha256().unwrap(), original);
        }};
    }

    assert_changes!(|project: &mut TasProject| project.identity.system.push('2'));
    assert_changes!(|project: &mut TasProject| project.identity.core_family.push('2'));
    assert_changes!(|project: &mut TasProject| project.identity.determinism_abi.push('2'));
    assert_changes!(|project: &mut TasProject| project.identity.source_media_sha256.0[0] ^= 1);
    assert_changes!(|project: &mut TasProject| project.identity.effective_media_sha256.0[0] ^= 1);
    assert_changes!(|project: &mut TasProject| project.identity.patches[0].sha256.0[0] ^= 1);
    assert_changes!(|project: &mut TasProject| {
        if let TasFirmwareIdentity::Skipped {
            compatibility_version,
            ..
        } = &mut project.identity.firmware[0]
        {
            *compatibility_version += 1;
        }
    });
    assert_changes!(|project: &mut TasProject| project.identity.devices[0]
        .configuration_sha256
        .0[0] ^= 1);
    assert_changes!(|project: &mut TasProject| project.identity.sync_config_sha256.0[0] ^= 1);
    assert_changes!(
        |project: &mut TasProject| project.identity.persistent_state =
            TasExternalIdentity::ExternalSha256(TasDigest([1; 32]))
    );
    assert_changes!(|project: &mut TasProject| project.identity.rtc_state =
        TasExternalIdentity::ExternalSha256(TasDigest([2; 32])));
    assert_changes!(|project: &mut TasProject| project.identity.sensor_state =
        TasExternalIdentity::ExternalSha256(TasDigest([3; 32])));
    assert_changes!(|project: &mut TasProject| project.identity.cheats =
        TasExternalIdentity::ExternalSha256(TasDigest([4; 32])));
    assert_changes!(|project: &mut TasProject| project
        .identity
        .state_format_compatibility_id
        .push('2'));
    assert_changes!(|project: &mut TasProject| {
        std::sync::Arc::make_mut(&mut project.start_state)[0] ^= 1;
        project.identity.start_state_sha256 = TasDigest::from_bytes(&project.start_state);
    });
    assert_changes!(
        |project: &mut TasProject| project.replay_start.wonder_swan_link_tick = Some(1)
    );
}

#[test]
fn firmware_and_device_order_are_not_sync_semantics() {
    let mut project = project();
    project.identity.firmware.push(TasFirmwareIdentity::Hle {
        firmware_id: "other".to_owned(),
        implementation: "hle-v1".to_owned(),
        compatibility_version: 1,
    });
    project.identity.devices.push(TasDeviceIdentity {
        port: "p2".to_owned(),
        device: "gamepad".to_owned(),
        configuration_sha256: TasDigest([0x88; 32]),
    });
    let original = project.sync_identity_sha256().unwrap();
    let original_package = project.encode().unwrap();
    project.identity.firmware.reverse();
    project.identity.devices.reverse();
    assert_eq!(project.sync_identity_sha256().unwrap(), original);
    assert_eq!(project.encode().unwrap(), original_package);
}

#[test]
fn replay_start_metadata_roundtrips_link_coordinator_state() {
    let link_state = ReplayGameBoyLinkState {
        peer_present: true,
        pending_master_byte: Some(0x12),
        pending_master_response: None,
        pending_master_completion_ready: false,
        queued_master_action: None,
        pending_passive_completion: None,
        serial_generation: 7,
    };
    let metadata = ReplayStartMetadata {
        game_boy_link_state: Some(link_state),
        game_boy_link_tick: Some(123),
        wonder_swan_link_tick: Some(456),
        game_boy_link_coordinator_state: Some(ReplayGameBoyLinkCoordinatorState {
            transfer_id: 9,
            action: ReplayGameBoyLinkAction {
                out_byte: 0x12,
                clock_period_t_cycles: 512,
                serial_generation: 7,
            },
            owner: ReplayGameBoyLinkCoordinatorOwner::ReplayAwaitingReply,
            reply: None,
        }),
    };

    let bytes = encode_replay_start_metadata(&metadata).unwrap();
    assert_eq!(decode_replay_start_metadata(&bytes).unwrap(), metadata);
}

#[test]
fn replay_start_metadata_preserves_independent_state_and_tick() {
    let state = ReplayGameBoyLinkState {
        peer_present: false,
        pending_master_byte: None,
        pending_master_response: None,
        pending_master_completion_ready: false,
        queued_master_action: None,
        pending_passive_completion: None,
        serial_generation: 0,
    };
    let state_only = ReplayStartMetadata {
        game_boy_link_state: Some(state),
        ..ReplayStartMetadata::default()
    };
    let tick_only = ReplayStartMetadata {
        game_boy_link_tick: Some(123),
        ..ReplayStartMetadata::default()
    };

    for metadata in [state_only, tick_only] {
        let bytes = encode_replay_start_metadata(&metadata).unwrap();
        assert_eq!(decode_replay_start_metadata(&bytes).unwrap(), metadata);
    }
}

#[test]
fn project_rejects_link_coordinator_without_matching_branch_event() {
    let mut project = project();
    project.replay_start = ReplayStartMetadata {
        game_boy_link_state: Some(ReplayGameBoyLinkState {
            peer_present: true,
            pending_master_byte: Some(0x12),
            pending_master_response: None,
            pending_master_completion_ready: false,
            queued_master_action: None,
            pending_passive_completion: None,
            serial_generation: 7,
        }),
        game_boy_link_tick: Some(123),
        wonder_swan_link_tick: None,
        game_boy_link_coordinator_state: Some(ReplayGameBoyLinkCoordinatorState {
            transfer_id: 9,
            action: ReplayGameBoyLinkAction {
                out_byte: 0x12,
                clock_period_t_cycles: 512,
                serial_generation: 7,
            },
            owner: ReplayGameBoyLinkCoordinatorOwner::ReplayAwaitingReply,
            reply: None,
        }),
    };
    assert!(project.validate().is_err());
}

#[test]
fn tas_event_ranges_preserve_final_cursor_without_subframe_off_by_one() {
    let mut boundary = project();
    boundary.branches[0].frame_count = 3;
    boundary.branches[0].input_spans.clear();
    boundary.branches[0].events = vec![ReplayEvent::FdsDiskSide { frame: 3, side: 1 }];
    boundary.markers.clear();
    boundary.annotations.clear();
    assert!(boundary.validate().is_ok());

    boundary.branches[0].events = vec![ReplayEvent::WonderSwanLink {
        frame: 3,
        session_cycle: 0,
        event: ReplayWonderSwanLinkEvent::RemoteByte {
            generation: 1,
            baud_bps: 9_600,
            byte: 0x42,
        },
    }];
    assert!(boundary.validate().is_err());
}
