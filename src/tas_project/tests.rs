use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use zeff_emu_common::media::{MediaEvent, MediaObjectId, MediaSlotId};
use zeff_emu_common::replay::{
    ReplayCheckpoint, ReplayEvent, ReplayFirmwareManifest, ReplayGameBoyLinkAction,
    ReplayGameBoyLinkCoordinatorOwner, ReplayGameBoyLinkCoordinatorState, ReplayGameBoyLinkEvent,
    ReplayGameBoyLinkState, ReplayJoypadFrame, ReplayMetadata, ReplayPlayer, ReplayRecorder,
    ReplayStartMetadata, ReplayWonderSwanLinkEvent, ReplayZapperFrame, decode_replay_event_stream,
    decode_replay_start_metadata, encode_replay_start_metadata,
};

use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};

use super::verification::TasExecutionSession;
use super::*;

fn project() -> TasProject {
    let start_state = vec![0xA5; 128];
    let camera = vec![0x10, 0x20, 0x30, 0x40];
    let camera_digest = TasDigest::from_bytes(&camera);
    let input = TasInputFrame {
        players: [
            TasControllerInput {
                buttons: 1,
                dpad: 2,
            },
            TasControllerInput {
                buttons: 3,
                dpad: 4,
            },
            TasControllerInput {
                buttons: 5,
                dpad: 6,
            },
            TasControllerInput {
                buttons: 7,
                dpad: 8,
            },
            TasControllerInput {
                buttons: 9,
                dpad: 10,
            },
        ],
        zapper: TasZapperInput {
            enabled: true,
            trigger: true,
            hit: false,
            screen_pos: Some([123, 87]),
        },
        tilt_x_bits: 0x3E80_0000,
        tilt_y_bits: 0xBE00_0000,
        camera: TasCameraInput::Blob(camera_digest),
    };
    let main = TasBranch {
        id: "main".to_owned(),
        name: "Main".to_owned(),
        comment: "base route".to_owned(),
        parent: None,
        frame_count: 12,
        input_spans: vec![TasInputSpan {
            start: 2,
            length: 2,
            input,
        }],
        events: vec![ReplayEvent::FdsDiskSide { frame: 6, side: 1 }],
        verification: None,
    };
    let alternative = TasBranch {
        id: "alternate".to_owned(),
        name: "Alternate".to_owned(),
        comment: String::new(),
        parent: Some(TasBranchOrigin {
            branch_id: "main".to_owned(),
            branch_movie_sha256: TasDigest([0x44; 32]),
            fork_cursor: 4,
        }),
        frame_count: 12,
        input_spans: vec![TasInputSpan {
            start: 8,
            length: 1,
            input: TasInputFrame {
                players: [TasControllerInput {
                    buttons: 0x80,
                    dpad: 0,
                }; 5],
                ..TasInputFrame::default()
            },
        }],
        events: Vec::new(),
        verification: None,
    };

    TasProject {
        project_id: "project-1".to_owned(),
        source_replay_sha256: None,
        identity: TasProjectIdentity {
            system: "nes".to_owned(),
            core_family: "zeff-nes".to_owned(),
            determinism_abi: "nes-sync-v1".to_owned(),
            source_media_sha256: TasDigest([0x11; 32]),
            effective_media_sha256: TasDigest([0x22; 32]),
            patches: vec![TasPatchIdentity {
                format: "bps".to_owned(),
                sha256: TasDigest([0x33; 32]),
            }],
            firmware: vec![TasFirmwareIdentity::Skipped {
                firmware_id: "fds-bios".to_owned(),
                compatibility_version: 1,
            }],
            devices: vec![TasDeviceIdentity {
                port: "p1".to_owned(),
                device: "gamepad".to_owned(),
                configuration_sha256: TasDigest([0x55; 32]),
            }],
            sync_config_sha256: TasDigest([0x66; 32]),
            persistent_state: TasExternalIdentity::Absent,
            rtc_state: TasExternalIdentity::Absent,
            sensor_state: TasExternalIdentity::ExternalSha256(TasDigest([0x77; 32])),
            cheats: TasExternalIdentity::Absent,
            state_format_compatibility_id: "nes-state-v7".to_owned(),
            start_state_sha256: TasDigest::from_bytes(&start_state),
        },
        start_state,
        replay_start: ReplayStartMetadata::default(),
        edit_generation: 3,
        rerecord_count: 2,
        active_branch_id: "main".to_owned(),
        project_comment: "project notes".to_owned(),
        branches: vec![main, alternative],
        markers: vec![TasMarker {
            id: "boss".to_owned(),
            branch_id: "main".to_owned(),
            cursor: 10,
            name: "Boss".to_owned(),
        }],
        annotations: vec![TasAnnotation {
            id: "lag".to_owned(),
            branch_id: "main".to_owned(),
            start: 3,
            length: 2,
            kind: "lag".to_owned(),
            text: "two frames".to_owned(),
        }],
        assets: BTreeMap::from([(camera_digest, camera)]),
    }
}

fn attach_current_verification(project: &mut TasProject, branch_id: &str) {
    let movie_hash = project.branch_movie_sha256(branch_id).unwrap();
    project
        .branches
        .iter_mut()
        .find(|branch| branch.id == branch_id)
        .unwrap()
        .verification = Some(TasVerificationProvenance {
        branch_movie_sha256: movie_hash,
        checkpoints: Vec::new(),
        final_state_sha256: Some(TasDigest([0xA7; 32])),
    });
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
fn edit_transaction_normalizes_input_and_invalidates_only_changed_branch() {
    let mut project = project();
    attach_current_verification(&mut project, "main");
    attach_current_verification(&mut project, "alternate");
    let alternate_verification = project.branches[1].verification.clone();
    let replacement = TasInputFrame {
        players: [TasControllerInput {
            buttons: 0x40,
            dpad: 0,
        }; 5],
        ..TasInputFrame::default()
    };

    let outcome = project
        .edit_transaction(|edit| edit.set_input_range("main", 3, 2, replacement))
        .unwrap();

    assert_eq!(outcome.edit_generation, 4);
    assert_eq!(outcome.rerecord_count, 3);
    assert_eq!(
        outcome.branch_impacts,
        vec![TasBranchEditImpact {
            branch_id: "main".to_owned(),
            kind: TasBranchEditImpactKind::Modified { earliest_cursor: 3 },
        }]
    );
    assert_eq!(project.branches[0].input_spans.len(), 2);
    assert_eq!(project.branches[0].input_spans[0].start, 2);
    assert_eq!(project.branches[0].input_spans[0].length, 1);
    assert_eq!(project.branches[0].input_spans[1].start, 3);
    assert_eq!(project.branches[0].input_spans[1].length, 2);
    assert!(project.branches[0].verification.is_none());
    assert_eq!(project.branches[1].verification, alternate_verification);
    assert_eq!(project.markers[0].cursor, 10);
    assert_eq!(project.annotations[0].start, 3);
}

#[test]
fn edit_transaction_merges_spans_and_omits_neutral_input() {
    let mut project = project();
    let original_input = project.branches[0].input_spans[0].input;

    let outcome = project
        .edit_transaction(|edit| edit.set_input_range("main", 4, 2, original_input))
        .unwrap();
    assert_eq!(
        outcome.branch_impacts[0].kind,
        TasBranchEditImpactKind::Modified { earliest_cursor: 4 }
    );
    assert_eq!(
        project.branches[0].input_spans,
        vec![TasInputSpan {
            start: 2,
            length: 4,
            input: original_input,
        }]
    );

    project
        .edit_transaction(|edit| edit.set_input_range("main", 2, 4, TasInputFrame::default()))
        .unwrap();
    assert!(project.branches[0].input_spans.is_empty());
}

#[test]
fn edit_transaction_presentation_changes_generation_but_not_movie_provenance() {
    let mut project = project();
    attach_current_verification(&mut project, "main");
    let movie_hash = project.branch_movie_sha256("main").unwrap();
    let input = project.branches[0].input_spans[0].input;
    let events = project.branches[0].events.clone();
    let verification = project.branches[0].verification.clone();

    let outcome = project
        .edit_transaction(|edit| {
            edit.rename_branch("main", "Renamed")?;
            edit.set_branch_comment("main", "new notes")?;
            edit.set_project_comment("new project notes");
            edit.set_active_branch("alternate")?;
            edit.set_input_range("main", 2, 2, input)?;
            edit.replace_branch_events("main", events)?;
            Ok(())
        })
        .unwrap();

    assert!(outcome.changed);
    assert_eq!(outcome.edit_generation, 4);
    assert_eq!(outcome.rerecord_count, 2);
    assert!(outcome.branch_impacts.is_empty());
    assert_eq!(project.branch_movie_sha256("main").unwrap(), movie_hash);
    assert_eq!(project.branches[0].verification, verification);
    assert_eq!(project.active_branch_id, "alternate");

    let before_reverted_edit = project.clone();
    let changed = TasInputFrame {
        players: [TasControllerInput {
            buttons: 0x20,
            dpad: 0,
        }; 5],
        ..TasInputFrame::default()
    };
    let outcome = project
        .edit_transaction(|edit| {
            edit.set_input_range("main", 0, 1, changed)?;
            edit.set_input_range("main", 0, 1, TasInputFrame::default())?;
            Ok(())
        })
        .unwrap();
    assert!(!outcome.changed);
    assert!(outcome.branch_impacts.is_empty());
    assert_eq!(project, before_reverted_edit);
}

#[test]
fn fork_transaction_captures_an_independent_full_snapshot() {
    let mut project = project();
    attach_current_verification(&mut project, "main");
    let parent_hash = project.branch_movie_sha256("main").unwrap();
    let parent_timeline = project.branches[0].clone();

    let outcome = project
        .edit_transaction(|edit| {
            edit.fork_branch("main", 6, "route-b", "Route B")?;
            edit.set_active_branch("route-b")?;
            Ok(())
        })
        .unwrap();

    assert_eq!(outcome.edit_generation, 4);
    assert_eq!(outcome.rerecord_count, 2);
    assert_eq!(
        outcome.branch_impacts,
        vec![TasBranchEditImpact {
            branch_id: "route-b".to_owned(),
            kind: TasBranchEditImpactKind::Created { fork_cursor: 6 },
        }]
    );
    let child = project.branch("route-b").unwrap();
    assert_eq!(child.frame_count, parent_timeline.frame_count);
    assert_eq!(child.input_spans, parent_timeline.input_spans);
    assert_eq!(child.events, parent_timeline.events);
    assert!(child.verification.is_none());
    assert_eq!(
        child.parent.as_ref().unwrap().branch_movie_sha256,
        parent_hash
    );
    assert_eq!(project.branch_movie_sha256("route-b").unwrap(), parent_hash);

    let child_snapshot = child.clone();
    let changed_parent_input = TasInputFrame {
        players: [TasControllerInput {
            buttons: 0x08,
            dpad: 0,
        }; 5],
        ..TasInputFrame::default()
    };
    project
        .edit_transaction(|edit| edit.set_input_range("main", 0, 1, changed_parent_input))
        .unwrap();
    assert_ne!(project.branch_movie_sha256("main").unwrap(), parent_hash);
    assert_eq!(project.branch("route-b").unwrap(), &child_snapshot);
    assert_eq!(
        TasProject::decode(&project.encode().unwrap()).unwrap(),
        project
    );
}

#[test]
fn mixed_edit_transaction_bumps_each_counter_at_most_once() {
    let mut project = project();
    let input_a = TasInputFrame {
        players: [TasControllerInput {
            buttons: 0x10,
            dpad: 0,
        }; 5],
        ..TasInputFrame::default()
    };
    let input_b = TasInputFrame {
        players: [TasControllerInput {
            buttons: 0x20,
            dpad: 0,
        }; 5],
        ..TasInputFrame::default()
    };

    project
        .edit_transaction(|edit| {
            edit.set_input_range("main", 0, 1, input_a)?;
            edit.set_input_range("main", 1, 1, input_b)?;
            edit.set_input_range("alternate", 0, 1, input_a)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(project.edit_generation, 4);
    assert_eq!(project.rerecord_count, 3);

    let outcome = project
        .edit_transaction(|edit| {
            edit.fork_branch("main", 4, "new-route", "New Route")?;
            edit.set_input_range("new-route", 5, 1, input_b)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(outcome.edit_generation, 5);
    assert_eq!(outcome.rerecord_count, 4);
}

#[test]
fn edit_transaction_canonicalizes_events_and_reports_the_first_changed_cursor() {
    let mut project = project();
    let outcome = project
        .edit_transaction(|edit| {
            edit.replace_branch_events(
                "main",
                vec![
                    ReplayEvent::FdsDiskSide { frame: 5, side: 0 },
                    ReplayEvent::FdsDiskSide { frame: 1, side: 1 },
                ],
            )
        })
        .unwrap();

    assert_eq!(project.branches[0].events[0].frame(), 1);
    assert_eq!(project.branches[0].events[1].frame(), 5);
    assert_eq!(
        outcome.branch_impacts[0].kind,
        TasBranchEditImpactKind::Modified { earliest_cursor: 1 }
    );
}

#[test]
fn edit_transaction_failures_are_fully_atomic() {
    let mut project = project();
    let original = project.clone();
    let error = project
        .edit_transaction(|edit| {
            edit.fork_branch("main", 4, "temporary", "Temporary")?;
            anyhow::bail!("injected failure")
        })
        .unwrap_err();
    assert!(error.to_string().contains("injected failure"));
    assert_eq!(project, original);

    assert!(
        project
            .edit_transaction(|edit| edit.fork_branch("main", 13, "past-end", "Past End"))
            .is_err()
    );
    assert_eq!(project, original);

    assert!(
        project
            .edit_transaction(|edit| edit.fork_branch("main", 4, "alternate", "Duplicate"))
            .is_err()
    );
    assert_eq!(project, original);

    assert!(
        project
            .edit_transaction(|edit| edit.set_input_range("main", u64::MAX, 2, Default::default()))
            .is_err()
    );
    assert_eq!(project, original);

    assert!(
        project
            .edit_transaction(|edit| {
                edit.replace_branch_events(
                    "main",
                    vec![ReplayEvent::FdsDiskSide { frame: 13, side: 0 }],
                )
            })
            .is_err()
    );
    assert_eq!(project, original);

    project.edit_generation = u64::MAX;
    let before_overflow = project.clone();
    assert!(
        project
            .edit_transaction(|edit| {
                edit.set_input_range(
                    "main",
                    0,
                    1,
                    TasInputFrame {
                        players: [TasControllerInput {
                            buttons: 1,
                            dpad: 0,
                        }; 5],
                        ..TasInputFrame::default()
                    },
                )
            })
            .is_err()
    );
    assert_eq!(project, before_overflow);

    let mut rerecord_overflow = original.clone();
    rerecord_overflow.rerecord_count = u64::MAX;
    let before_rerecord_overflow = rerecord_overflow.clone();
    assert!(
        rerecord_overflow
            .edit_transaction(|edit| {
                edit.set_input_range(
                    "main",
                    0,
                    1,
                    TasInputFrame {
                        players: [TasControllerInput {
                            buttons: 2,
                            dpad: 0,
                        }; 5],
                        ..TasInputFrame::default()
                    },
                )
            })
            .is_err()
    );
    assert_eq!(rerecord_overflow, before_rerecord_overflow);
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
        project.start_state[0] ^= 1;
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
fn cache_prefix_changes_only_after_edited_input_is_consumed() {
    let project = project();
    let before = project.seek_cache_identity("main", 2).unwrap();
    let after = project.seek_cache_identity("main", 3).unwrap();
    let mut edited = project.clone();
    edited.branches[0].input_spans[0].input.players[0].buttons ^= 0x40;

    assert_eq!(edited.seek_cache_identity("main", 2).unwrap(), before);
    assert_ne!(edited.seek_cache_identity("main", 3).unwrap(), after);
}

#[test]
fn cache_prefix_changes_only_after_an_edited_event() {
    let project = project();
    let before = project.seek_cache_identity("main", 6).unwrap();
    let after = project.seek_cache_identity("main", 7).unwrap();
    let mut edited = project.clone();
    if let ReplayEvent::FdsDiskSide { side, .. } = &mut edited.branches[0].events[0] {
        *side = 0;
    }

    assert_eq!(edited.seek_cache_identity("main", 6).unwrap(), before);
    assert_ne!(edited.seek_cache_identity("main", 7).unwrap(), after);
}

#[test]
fn branch_snapshots_do_not_depend_on_live_parent_content() {
    let project = project();
    let alternate = project.branch_movie_sha256("alternate").unwrap();
    let mut edited = project.clone();
    edited.branches[0].input_spans[0].input.players[0].buttons ^= 0x20;

    assert_ne!(
        edited.branch_movie_sha256("main").unwrap(),
        project.branch_movie_sha256("main").unwrap()
    );
    assert_eq!(edited.branch_movie_sha256("alternate").unwrap(), alternate);
}

#[test]
fn verification_provenance_becomes_stale_after_a_movie_edit() {
    let mut project = project();
    let movie_hash = project.branch_movie_sha256("main").unwrap();
    project.branches[0].verification = Some(TasVerificationProvenance {
        branch_movie_sha256: movie_hash,
        checkpoints: vec![TasVerificationCheckpoint {
            cursor: 6,
            state_sha256: TasDigest([0x99; 32]),
        }],
        final_state_sha256: Some(TasDigest([0xAA; 32])),
    });
    assert!(project.verification_is_current("main").unwrap());
    assert_eq!(
        TasProject::decode(&project.encode().unwrap()).unwrap(),
        project
    );
    project.branches[0].input_spans[0].input.players[0].buttons ^= 0x20;
    assert!(!project.verification_is_current("main").unwrap());
}

#[test]
fn decoder_rejects_corrupt_critical_entry() {
    let bytes = project().encode().unwrap();
    let corrupted = rewrite_zip(&bytes, |name, bytes| {
        if name == "start_state.bin" {
            bytes[0] ^= 0xFF;
        }
    });
    let error = TasProject::decode(&corrupted).unwrap_err().to_string();
    assert!(error.contains("SHA-256"), "error was: {error}");
}

#[test]
fn decoder_rejects_duplicate_and_unsafe_entries() {
    let duplicate = zip_with_duplicate_name();
    assert!(TasProject::decode(&duplicate).is_err());

    let traversal = zip_entries(&[
        ("manifest.json", b"{}"),
        ("integrity.json", b"{}"),
        ("start_state.bin", b""),
        ("../escape", b"bad"),
    ]);
    assert!(TasProject::decode(&traversal).is_err());
}

#[test]
fn event_stream_canonicalizes_and_rejects_trailing_bytes() {
    let stream = zeff_emu_common::replay::encode_replay_event_stream(&[
        ReplayEvent::FdsDiskSide { frame: 2, side: 0 },
        ReplayEvent::FdsDiskSide { frame: 1, side: 1 },
    ])
    .unwrap();
    let decoded = decode_replay_event_stream(&stream).unwrap();
    assert_eq!(decoded[0].frame(), 1);

    let mut trailing = stream;
    trailing.push(0);
    assert!(decode_replay_event_stream(&trailing).is_err());
}

#[test]
fn event_stream_roundtrips_every_current_event_domain() {
    let state = ReplayGameBoyLinkState {
        peer_present: false,
        pending_master_byte: None,
        pending_master_response: None,
        pending_master_completion_ready: false,
        queued_master_action: None,
        pending_passive_completion: None,
        serial_generation: 0,
    };
    let events = vec![
        ReplayEvent::FdsDiskSide { frame: 1, side: 1 },
        ReplayEvent::Media {
            frame: 1,
            sequence: 1,
            event: MediaEvent::Insert {
                slot: MediaSlotId::new("drive"),
                media_id: MediaObjectId::new("disc"),
                side: Some(0),
                write_protected: true,
            },
        },
        ReplayEvent::GameBoyLinkState { frame: 2, state },
        ReplayEvent::GameBoyLinkStateAtTick {
            frame: 3,
            tick: 10,
            state,
        },
        ReplayEvent::GameBoyLink {
            frame: 3,
            tick: 20,
            event: ReplayGameBoyLinkEvent::LocalMasterStart {
                transfer_id: 1,
                clock_period_t_cycles: 512,
                out_byte: 0x34,
                serial_generation: 1,
            },
        },
        ReplayEvent::WonderSwanLink {
            frame: 4,
            session_cycle: 30,
            event: ReplayWonderSwanLinkEvent::RemoteByte {
                generation: 1,
                baud_bps: 9_600,
                byte: 0x56,
            },
        },
    ];
    let bytes = zeff_emu_common::replay::encode_replay_event_stream(&events).unwrap();
    assert_eq!(decode_replay_event_stream(&bytes).unwrap(), events);
}

#[test]
fn atomic_save_preserves_backup_and_recovers() {
    let directory =
        std::env::temp_dir().join(format!("zeff-tas-save-{}-{}", std::process::id(), "backup"));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("movie.ztas");
    let original = project();
    original.save_atomic(&path).unwrap();

    let mut updated = original.clone();
    updated.project_comment = "updated".to_owned();
    updated.edit_generation += 1;
    updated.save_atomic(&path).unwrap();
    assert_eq!(TasProject::load(&path).unwrap(), updated);
    assert_eq!(
        TasProject::load(&TasProject::backup_path(&path).unwrap()).unwrap(),
        original
    );

    std::fs::write(&path, b"corrupt").unwrap();
    let (recovered, source) = TasProject::load_with_backup(&path).unwrap();
    assert_eq!(source, TasProjectLoadSource::Backup);
    assert_eq!(recovered, original);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn failed_atomic_save_keeps_the_previous_project() {
    let directory = std::env::temp_dir().join(format!(
        "zeff-tas-save-{}-{}",
        std::process::id(),
        "failure"
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("movie.ztas");
    let original = project();
    original.save_atomic(&path).unwrap();
    let mut invalid = original.clone();
    invalid.identity.start_state_sha256.0[0] ^= 1;

    assert!(invalid.save_atomic(&path).is_err());
    assert_eq!(TasProject::load(&path).unwrap(), original);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn zrpl_import_export_roundtrips_every_input_channel_and_verification() {
    let directory = zrpl_test_dir("roundtrip");
    let source_path = directory.join("source.zrpl");
    let output_path = directory.join("output.zrpl");
    let start_state = vec![0xC3; 257];
    let camera = vec![0x10, 0x20, 0x30, 0x40];
    let input = ReplayJoypadFrame {
        buttons: 1,
        dpad: 2,
        buttons_p2: 3,
        dpad_p2: 4,
        buttons_p3: 5,
        dpad_p3: 6,
        buttons_p4: 7,
        dpad_p4: 8,
        buttons_p5: 9,
        dpad_p5: 10,
        zapper: ReplayZapperFrame {
            enabled: true,
            trigger: true,
            hit: false,
            screen_pos: Some((0, 0)),
        },
        host_tilt: (f32::from_bits(0x7FC0_1234), f32::from_bits(0x8000_0000)),
        camera_frame: Some(camera.clone()),
    };
    let metadata = replay_metadata(Some([0x99; 32]));
    let mut recorder = ReplayRecorder::new_with_metadata(
        source_path.clone(),
        start_state.clone(),
        metadata.clone(),
    );
    recorder.record_joypad_frame(ReplayJoypadFrame::default());
    recorder.record_joypad_frame(input.clone());
    recorder.record_joypad_frame(input.clone());
    recorder.record_joypad_frame(ReplayJoypadFrame::default());
    recorder.record_joypad_frame(input.clone());
    recorder.finish().unwrap();
    let source_bytes = std::fs::read(&source_path).unwrap();

    let project =
        TasProject::import_zrpl(&source_path, replay_witness(&start_state, Some([0x99; 32])))
            .unwrap();
    assert_eq!(
        project.source_replay_sha256,
        Some(TasDigest::from_bytes(&source_bytes))
    );
    let persisted = TasProject::decode(&project.encode().unwrap()).unwrap();
    assert_eq!(persisted.source_replay_sha256, project.source_replay_sha256);
    assert_eq!(project.branches[0].frame_count, 5);
    assert_eq!(project.branches[0].input_spans.len(), 2);
    assert_eq!(project.branches[0].input_spans[0].length, 2);
    assert_eq!(project.assets.len(), 1);
    assert_eq!(project.replay_start.wonder_swan_link_tick, Some(456));
    assert!(project.verification_is_current("main").unwrap());

    let without_provenance = TasProject {
        source_replay_sha256: None,
        ..project.clone()
    };
    assert_eq!(
        project.sync_identity_sha256().unwrap(),
        without_provenance.sync_identity_sha256().unwrap()
    );
    assert_eq!(
        project.branch_movie_sha256("main").unwrap(),
        without_provenance.branch_movie_sha256("main").unwrap()
    );

    project
        .export_zrpl_without_execution_for_test("main", &output_path)
        .unwrap();
    let source = ReplayPlayer::load(&source_path).unwrap();
    let output = ReplayPlayer::load(&output_path).unwrap();
    assert_eq!(output.save_state(), source.save_state());
    assert_eq!(output.metadata(), source.metadata());
    assert_eq!(
        output.peek_joypad_frames(0, output.total_frames()),
        source.peek_joypad_frames(0, source.total_frames())
    );
    assert!(
        project
            .export_zrpl_without_execution_for_test("main", &output_path)
            .is_err()
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn zrpl_import_rejects_legacy_and_every_mismatched_witness_domain() {
    let directory = zrpl_test_dir("witness");
    let source_path = directory.join("source.zrpl");
    let start_state = vec![0x5A; 64];
    write_replay_fixture(&source_path, &start_state, Some([0x99; 32]));

    let mut witnesses = Vec::new();
    let mut system = replay_witness(&start_state, Some([0x99; 32]));
    system.identity.system = "gb".to_owned();
    witnesses.push(system);
    let mut core = replay_witness(&start_state, Some([0x99; 32]));
    core.identity.core_family = "GameBoy".to_owned();
    witnesses.push(core);
    let mut media = replay_witness(&start_state, Some([0x99; 32]));
    media.identity.effective_media_sha256.0[0] ^= 1;
    witnesses.push(media);
    let mut firmware = replay_witness(&start_state, Some([0x99; 32]));
    firmware.identity.firmware.clear();
    witnesses.push(firmware);
    let cheats = replay_witness(&start_state, None);
    witnesses.push(cheats);
    let mut state = replay_witness(&start_state, Some([0x99; 32]));
    state.identity.start_state_sha256.0[0] ^= 1;
    witnesses.push(state);

    for witness in witnesses {
        assert!(TasProject::import_zrpl(&source_path, witness).is_err());
    }

    let legacy_path = directory.join("legacy.zrpl");
    let mut legacy = std::fs::read(&source_path).unwrap();
    legacy[4..8].copy_from_slice(&1u32.to_le_bytes());
    std::fs::write(&legacy_path, legacy).unwrap();
    assert!(
        TasProject::import_zrpl(&legacy_path, replay_witness(&start_state, Some([0x99; 32])))
            .is_err()
    );

    let old_metadata_path = directory.join("old-metadata.zrpl");
    let mut old_metadata = std::fs::read(&source_path).unwrap();
    old_metadata[12..16].copy_from_slice(&2u32.to_le_bytes());
    std::fs::write(&old_metadata_path, old_metadata).unwrap();
    assert!(
        TasProject::import_zrpl(
            &old_metadata_path,
            replay_witness(&start_state, Some([0x99; 32]))
        )
        .is_err()
    );

    let empty_metadata_path = directory.join("empty-metadata.zrpl");
    ReplayRecorder::new(empty_metadata_path.clone(), start_state.clone())
        .finish()
        .unwrap();
    assert!(
        TasProject::import_zrpl(
            &empty_metadata_path,
            replay_witness(&start_state, Some([0x99; 32]))
        )
        .is_err()
    );

    let duplicate_checkpoint_path = directory.join("duplicate-checkpoint.zrpl");
    let mut duplicate_metadata = replay_metadata(Some([0x99; 32]));
    duplicate_metadata.checkpoints = vec![
        ReplayCheckpoint {
            frame: 0,
            state_sha256: [1; 32],
        },
        ReplayCheckpoint {
            frame: 0,
            state_sha256: [2; 32],
        },
    ];
    ReplayRecorder::new_with_metadata(
        duplicate_checkpoint_path.clone(),
        start_state.clone(),
        duplicate_metadata,
    )
    .finish()
    .unwrap();
    assert!(
        TasProject::import_zrpl(
            &duplicate_checkpoint_path,
            replay_witness(&start_state, Some([0x99; 32]))
        )
        .is_err()
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn zrpl_export_rejects_stale_verification_and_empty_camera_assets() {
    let directory = zrpl_test_dir("export-gates");
    let source_path = directory.join("source.zrpl");
    let start_state = vec![0x6B; 64];
    write_replay_fixture(&source_path, &start_state, None);
    let project =
        TasProject::import_zrpl(&source_path, replay_witness(&start_state, None)).unwrap();

    let mut stale = project.clone();
    stale.branches[0].input_spans[0].input.players[0].buttons ^= 2;
    let stale_path = directory.join("stale.zrpl");
    assert!(
        stale
            .export_zrpl_without_execution_for_test("main", &stale_path)
            .is_err()
    );
    assert!(!stale_path.exists());
    assert!(
        stale
            .export_zrpl_without_execution_for_test("missing", &stale_path)
            .is_err()
    );

    let mut empty_camera = project;
    let empty_digest = TasDigest::from_bytes(&[]);
    empty_camera.assets.clear();
    empty_camera.assets.insert(empty_digest, Vec::new());
    empty_camera.branches[0].input_spans[0].input.camera = TasCameraInput::Blob(empty_digest);
    empty_camera.branches[0].verification = None;
    let empty_path = directory.join("empty.zrpl");
    assert!(empty_camera.validate().is_ok());
    assert!(
        empty_camera
            .export_zrpl_without_execution_for_test("main", &empty_path)
            .is_err()
    );
    assert!(!empty_path.exists());
    std::fs::remove_dir_all(directory).unwrap();
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

#[test]
fn emulator_verification_is_two_pass_deterministic_and_exports_embedded_provenance()
-> anyhow::Result<()> {
    let directory = zrpl_test_dir("executed-verification");
    let rom_path = directory.join("test.nes");
    let (mut project, witness, rom) = executable_nes_project(&rom_path, 601)?;
    let edit_generation = project.edit_generation;
    let rerecord_count = project.rerecord_count;
    let mut loads = 0;

    let verification = project.verify_branch_with_factory("main", &witness, || {
        loads += 1;
        load_executable_nes_session(&rom_path, rom.clone(), &witness)
    })?;

    assert_eq!(loads, 2);
    assert_eq!(
        verification
            .checkpoints
            .iter()
            .map(|checkpoint| checkpoint.cursor)
            .collect::<Vec<_>>(),
        vec![300, 600]
    );
    assert!(verification.final_state_sha256.is_some());
    assert_eq!(project.edit_generation, edit_generation);
    assert_eq!(project.rerecord_count, rerecord_count);
    assert!(project.verification_is_current("main")?);

    let mut repeat_loads = 0;
    let repeated = project.verify_branch_with_factory("main", &witness, || {
        repeat_loads += 1;
        load_executable_nes_session(&rom_path, rom.clone(), &witness)
    })?;
    assert_eq!(repeat_loads, 2);
    assert_eq!(repeated, verification);

    let output_path = directory.join("verified.zrpl");
    let mut export_loads = 0;
    project.verify_and_export_zrpl_with_factory("main", &output_path, &witness, || {
        export_loads += 1;
        load_executable_nes_session(&rom_path, rom.clone(), &witness)
    })?;
    assert_eq!(export_loads, 2);
    let output = ReplayPlayer::load(&output_path)?;
    assert_eq!(output.total_frames(), 601);
    assert_eq!(output.metadata().checkpoints.len(), 2);
    assert_eq!(
        output.metadata().final_state_sha256,
        verification.final_state_sha256.map(|digest| digest.0)
    );

    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn emulator_verification_failures_are_transactional_and_require_complete_identity()
-> anyhow::Result<()> {
    let directory = zrpl_test_dir("executed-verification-failures");
    let rom_path = directory.join("test.nes");
    let (project, witness, rom) = executable_nes_project(&rom_path, 301)?;

    let mut witness_mismatches = Vec::new();
    let mut identity = witness.identity.clone();
    identity.system.push('2');
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.core_family.push('2');
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.determinism_abi.push('2');
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.source_media_sha256.0[0] ^= 1;
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.effective_media_sha256.0[0] ^= 1;
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.patches.push(TasPatchIdentity {
        format: "ips".to_owned(),
        sha256: TasDigest([1; 32]),
    });
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.firmware.push(TasFirmwareIdentity::Skipped {
        firmware_id: "test-firmware".to_owned(),
        compatibility_version: 1,
    });
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.devices[0].configuration_sha256.0[0] ^= 1;
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.sync_config_sha256.0[0] ^= 1;
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.persistent_state = TasExternalIdentity::ExternalSha256(TasDigest([2; 32]));
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.rtc_state = TasExternalIdentity::ExternalSha256(TasDigest([3; 32]));
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.sensor_state = TasExternalIdentity::ExternalSha256(TasDigest([4; 32]));
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.cheats = TasExternalIdentity::ExternalSha256(TasDigest([5; 32]));
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.state_format_compatibility_id.push('2');
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.start_state_sha256.0[0] ^= 1;
    witness_mismatches.push(identity);

    for identity in witness_mismatches {
        let mismatched_witness = TasExecutionWitness { identity };
        let mut witness_failure = project.clone();
        let before = witness_failure.encode()?;
        let result = witness_failure.verify_branch_with_factory(
            "main",
            &mismatched_witness,
            || -> anyhow::Result<TasExecutionSession> {
                panic!("backend must not load for witness mismatch")
            },
        );
        assert!(result.is_err());
        assert_eq!(witness_failure.encode()?, before);
    }

    let mut second_pass_failure = project.clone();
    let before = second_pass_failure.encode()?;
    let mut loads = 0;
    let result = second_pass_failure.verify_branch_with_factory("main", &witness, || {
        loads += 1;
        if loads == 2 {
            anyhow::bail!("injected second-pass backend failure");
        }
        load_executable_nes_session(&rom_path, rom.clone(), &witness)
    });
    assert!(result.is_err());
    assert_eq!(loads, 2);
    assert_eq!(second_pass_failure.encode()?, before);

    let mut first_session_identity_failure = project.clone();
    let before = first_session_identity_failure.encode()?;
    let mut mismatched_session_identity = witness.identity.clone();
    mismatched_session_identity.sync_config_sha256.0[0] ^= 1;
    let result =
        first_session_identity_failure.verify_branch_with_factory("main", &witness, || {
            Ok(TasExecutionSession::new(
                load_executable_nes_backend(&rom_path, rom.clone())?,
                mismatched_session_identity.clone(),
            ))
        });
    assert!(result.is_err());
    assert_eq!(first_session_identity_failure.encode()?, before);

    let mut second_session_identity_failure = project.clone();
    let before = second_session_identity_failure.encode()?;
    let mut loads = 0;
    let result =
        second_session_identity_failure.verify_branch_with_factory("main", &witness, || {
            loads += 1;
            let identity = if loads == 2 {
                mismatched_session_identity.clone()
            } else {
                witness.identity.clone()
            };
            Ok(TasExecutionSession::new(
                load_executable_nes_backend(&rom_path, rom.clone())?,
                identity,
            ))
        });
    assert!(result.is_err());
    assert_eq!(loads, 2);
    assert_eq!(second_session_identity_failure.encode()?, before);

    let failed_export_path = directory.join("failed-export.zrpl");
    let mut export_failure = project.clone();
    let before = export_failure.encode()?;
    let mut export_loads = 0;
    let result = export_failure.verify_and_export_zrpl_with_factory(
        "main",
        &failed_export_path,
        &witness,
        || {
            export_loads += 1;
            if export_loads == 2 {
                anyhow::bail!("injected temporary replay execution failure");
            }
            load_executable_nes_session(&rom_path, rom.clone(), &witness)
        },
    );
    assert!(result.is_err());
    assert_eq!(export_loads, 2);
    assert!(!failed_export_path.exists());
    assert!(
        std::fs::read_dir(&directory)?
            .filter_map(Result::ok)
            .all(|entry| !entry.file_name().to_string_lossy().contains(".tmp."))
    );
    assert_eq!(export_failure.encode()?, before);

    let existing_path = directory.join("existing.zrpl");
    std::fs::write(&existing_path, b"concurrent replay")?;
    let mut existing_failure = project.clone();
    let before = existing_failure.encode()?;
    let result = existing_failure.verify_and_export_zrpl_with_factory(
        "main",
        &existing_path,
        &witness,
        || -> anyhow::Result<TasExecutionSession> {
            panic!("backend must not load when export target already exists")
        },
    );
    assert!(result.is_err());
    assert_eq!(std::fs::read(&existing_path)?, b"concurrent replay");
    assert_eq!(existing_failure.encode()?, before);

    let mut wrong_backend = project.clone();
    let before = wrong_backend.encode()?;
    let mut wrong_rom = rom.clone();
    wrong_rom[16] ^= 1;
    let result = wrong_backend.verify_branch_with_factory("main", &witness, || {
        load_executable_nes_session(&rom_path, wrong_rom.clone(), &witness)
    });
    assert!(result.is_err());
    assert_eq!(wrong_backend.encode()?, before);

    let mut cheat_project = project.clone();
    cheat_project.identity.cheats = TasExternalIdentity::ExternalSha256(TasDigest([0xA5; 32]));
    let cheat_witness = TasExecutionWitness {
        identity: cheat_project.identity.clone(),
    };
    let before = cheat_project.encode()?;
    let result = cheat_project.verify_branch_with_factory("main", &cheat_witness, || {
        load_executable_nes_session(&rom_path, rom.clone(), &cheat_witness)
    });
    assert!(result.is_err());
    assert_eq!(cheat_project.encode()?, before);

    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn emulator_verification_preserves_valid_imported_schedule_and_rejects_bad_hashes()
-> anyhow::Result<()> {
    let directory = zrpl_test_dir("executed-verification-existing");
    let rom_path = directory.join("test.nes");
    let (mut project, witness, rom) = executable_nes_project(&rom_path, 301)?;
    let generated = project.verify_branch_with_factory("main", &witness, || {
        load_executable_nes_session(&rom_path, rom.clone(), &witness)
    })?;

    let mut bad = project.clone();
    bad.branches[0].verification.as_mut().unwrap().checkpoints[0]
        .state_sha256
        .0[0] ^= 1;
    let before = bad.encode()?;
    let result = bad.verify_branch_with_factory("main", &witness, || {
        load_executable_nes_session(&rom_path, rom.clone(), &witness)
    });
    assert!(result.is_err());
    assert_eq!(bad.encode()?, before);

    let mut stale = project.clone();
    stale.branches[0].input_spans[0].input.players[0].buttons ^= 2;
    assert!(!stale.verification_is_current("main")?);
    let refreshed = stale.verify_branch_with_factory("main", &witness, || {
        load_executable_nes_session(&rom_path, rom.clone(), &witness)
    })?;
    assert_ne!(refreshed.branch_movie_sha256, generated.branch_movie_sha256);
    assert_eq!(
        refreshed
            .checkpoints
            .iter()
            .map(|checkpoint| checkpoint.cursor)
            .collect::<Vec<_>>(),
        vec![300]
    );
    assert!(stale.verification_is_current("main")?);

    std::fs::remove_dir_all(directory)?;
    Ok(())
}

fn replay_witness(start_state: &[u8], cheat_sha256: Option<[u8; 32]>) -> TasZrplImportWitness {
    let mut identity = project().identity;
    identity.start_state_sha256 = TasDigest::from_bytes(start_state);
    identity.firmware = vec![
        TasFirmwareIdentity::Skipped {
            firmware_id: "d-skipped".to_owned(),
            compatibility_version: 4,
        },
        TasFirmwareIdentity::BuiltinOpenSource {
            firmware_id: "c-open".to_owned(),
            implementation: "open-v1".to_owned(),
            compatibility_version: 3,
            sha256: TasDigest([3; 32]),
        },
        TasFirmwareIdentity::Hle {
            firmware_id: "b-hle".to_owned(),
            implementation: "hle-v1".to_owned(),
            compatibility_version: 2,
        },
        TasFirmwareIdentity::External {
            firmware_id: "a-external".to_owned(),
            variant: Some("variant-a".to_owned()),
            sha256: TasDigest([1; 32]),
        },
    ];
    identity.cheats = cheat_sha256.map_or(TasExternalIdentity::Absent, |sha256| {
        TasExternalIdentity::ExternalSha256(TasDigest(sha256))
    });
    TasZrplImportWitness {
        project_id: "imported-project".to_owned(),
        identity,
    }
}

fn build_executable_nes_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
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

fn load_executable_nes_backend(
    rom_path: &std::path::Path,
    rom: Vec<u8>,
) -> anyhow::Result<EmuBackend> {
    Ok(load_backend_from_rom_source(
        ActiveSystem::Nes,
        rom_path,
        rom_path,
        Some(rom),
        BackendLoadConfig::default(),
    )?
    .backend)
}

fn load_executable_nes_session(
    rom_path: &std::path::Path,
    rom: Vec<u8>,
    witness: &TasExecutionWitness,
) -> anyhow::Result<TasExecutionSession> {
    Ok(TasExecutionSession::new(
        load_executable_nes_backend(rom_path, rom)?,
        witness.identity.clone(),
    ))
}

fn executable_nes_project(
    rom_path: &std::path::Path,
    frame_count: u64,
) -> anyhow::Result<(TasProject, TasExecutionWitness, Vec<u8>)> {
    let rom = build_executable_nes_rom();
    let backend = load_executable_nes_backend(rom_path, rom.clone())?;
    let start_state = backend.encode_state_bytes()?;
    let metadata = backend.replay_metadata();
    let effective_media_sha256 = TasDigest(
        metadata
            .rom_sha256
            .ok_or_else(|| anyhow::anyhow!("NES fixture is missing its ROM hash"))?,
    );
    let identity = TasProjectIdentity {
        system: metadata.system.unwrap(),
        core_family: metadata.core_family.unwrap(),
        determinism_abi: "nes-test-determinism-v1".to_owned(),
        source_media_sha256: effective_media_sha256,
        effective_media_sha256,
        patches: Vec::new(),
        firmware: Vec::new(),
        devices: vec![TasDeviceIdentity {
            port: "p1".to_owned(),
            device: "gamepad".to_owned(),
            configuration_sha256: TasDigest([0; 32]),
        }],
        sync_config_sha256: TasDigest([0; 32]),
        persistent_state: TasExternalIdentity::Absent,
        rtc_state: TasExternalIdentity::Absent,
        sensor_state: TasExternalIdentity::Absent,
        cheats: TasExternalIdentity::Absent,
        state_format_compatibility_id: "nes-test-state-v1".to_owned(),
        start_state_sha256: TasDigest::from_bytes(&start_state),
    };
    let witness = TasExecutionWitness {
        identity: identity.clone(),
    };
    let project = TasProject {
        project_id: "executable-nes".to_owned(),
        source_replay_sha256: None,
        identity,
        start_state,
        replay_start: ReplayStartMetadata::default(),
        edit_generation: 7,
        rerecord_count: 3,
        active_branch_id: "main".to_owned(),
        project_comment: String::new(),
        branches: vec![TasBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            comment: String::new(),
            parent: None,
            frame_count,
            input_spans: vec![TasInputSpan {
                start: 0,
                length: frame_count,
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
            verification: None,
        }],
        markers: Vec::new(),
        annotations: Vec::new(),
        assets: BTreeMap::new(),
    };
    project.validate()?;
    Ok((project, witness, rom))
}

fn replay_metadata(cheat_sha256: Option<[u8; 32]>) -> ReplayMetadata {
    ReplayMetadata {
        system: Some("nes".to_owned()),
        core_family: Some("zeff-nes".to_owned()),
        rom_sha256: Some([0x22; 32]),
        firmware: vec![
            ReplayFirmwareManifest::External {
                firmware_id: "a-external".to_owned(),
                variant: Some("variant-a".to_owned()),
                sha256: [1; 32],
            },
            ReplayFirmwareManifest::Hle {
                firmware_id: "b-hle".to_owned(),
                implementation: "hle-v1".to_owned(),
                compatibility_version: 2,
            },
            ReplayFirmwareManifest::BuiltinOpenSource {
                firmware_id: "c-open".to_owned(),
                implementation: "open-v1".to_owned(),
                compatibility_version: 3,
                sha256: [3; 32],
            },
            ReplayFirmwareManifest::Skipped {
                firmware_id: "d-skipped".to_owned(),
                compatibility_version: 4,
            },
        ],
        events: vec![ReplayEvent::FdsDiskSide { frame: 1, side: 1 }],
        cheat_sha256,
        final_state_sha256: Some([0xAB; 32]),
        wonder_swan_link_start_tick: Some(456),
        checkpoints: vec![
            ReplayCheckpoint {
                frame: 0,
                state_sha256: [0xCD; 32],
            },
            ReplayCheckpoint {
                frame: 5,
                state_sha256: [0xEF; 32],
            },
        ],
        ..ReplayMetadata::default()
    }
}

fn write_replay_fixture(path: &std::path::Path, start_state: &[u8], cheat: Option<[u8; 32]>) {
    let mut recorder = ReplayRecorder::new_with_metadata(
        path.to_path_buf(),
        start_state.to_vec(),
        replay_metadata(cheat),
    );
    recorder.record_joypad_frame(ReplayJoypadFrame {
        buttons: 1,
        camera_frame: Some(vec![1, 2, 3]),
        ..ReplayJoypadFrame::default()
    });
    recorder.record_joypad_frame(ReplayJoypadFrame::default());
    recorder.record_joypad_frame(ReplayJoypadFrame::default());
    recorder.finish().unwrap();
}

fn zrpl_test_dir(name: &str) -> std::path::PathBuf {
    let directory =
        std::env::temp_dir().join(format!("zeff-tas-zrpl-{}-{name}", std::process::id()));
    if directory.exists() {
        std::fs::remove_dir_all(&directory).unwrap();
    }
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

fn rewrite_zip(bytes: &[u8], mut edit: impl FnMut(&str, &mut Vec<u8>)) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        edit(&name, &mut bytes);
        entries.push((name, bytes));
    }

    let cursor = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    for (name, bytes) in entries {
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn zip_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    for (name, bytes) in entries {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn zip_with_duplicate_name() -> Vec<u8> {
    let mut bytes = zip_entries(&[
        ("branches/branch-a/events.bin", b"a"),
        ("branches/branch-b/events.bin", b"b"),
    ]);
    let source = b"branches/branch-b/events.bin";
    let replacement = b"branches/branch-a/events.bin";
    let mut replacements = 0;
    for offset in 0..=bytes.len() - source.len() {
        if bytes[offset..].starts_with(source) {
            bytes[offset..offset + source.len()].copy_from_slice(replacement);
            replacements += 1;
        }
    }
    assert!(replacements >= 2);
    bytes
}
