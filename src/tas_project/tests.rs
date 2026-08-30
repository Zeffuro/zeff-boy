use std::collections::BTreeMap;
use zeff_emu_common::replay::{
    ReplayEvent, ReplayGameBoyLinkAction, ReplayGameBoyLinkCoordinatorOwner,
    ReplayGameBoyLinkCoordinatorState, ReplayGameBoyLinkState, ReplayStartMetadata,
    ReplayWonderSwanLinkEvent, decode_replay_start_metadata, encode_replay_start_metadata,
};

use super::*;

mod branch_diff;
mod executable_verification;
mod format;
mod input_patterns;
mod timeline_edits;
mod zrpl;

pub(super) fn project() -> TasProject {
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

pub(super) fn zrpl_test_dir(name: &str) -> std::path::PathBuf {
    let directory =
        std::env::temp_dir().join(format!("zeff-tas-zrpl-{}-{name}", std::process::id()));
    if directory.exists() {
        std::fs::remove_dir_all(&directory).unwrap();
    }
    std::fs::create_dir_all(&directory).unwrap();
    directory
}
