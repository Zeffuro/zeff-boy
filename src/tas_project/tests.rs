use std::collections::BTreeMap;

use zeff_emu_common::replay::{ReplayEvent, ReplayStartMetadata};

use super::*;

mod branch_classification;
mod branch_deletion;
mod branch_diff;
mod cache_persistence;
mod edit_transactions;
mod executable_verification;
mod format;
mod identity_validation;
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
        coleco: [
            TasColecoControllerInput {
                up: true,
                right: false,
                down: false,
                left: true,
                left_button: true,
                right_button: false,
                keypad: TasColecoKeypadKey::Pound,
            },
            TasColecoControllerInput::default(),
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
        start_state: start_state.into(),
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

pub(super) fn zrpl_test_dir(name: &str) -> std::path::PathBuf {
    let directory =
        std::env::temp_dir().join(format!("zeff-tas-zrpl-{}-{name}", std::process::id()));
    if directory.exists() {
        std::fs::remove_dir_all(&directory).unwrap();
    }
    std::fs::create_dir_all(&directory).unwrap();
    directory
}
