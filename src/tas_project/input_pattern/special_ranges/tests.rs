use std::collections::BTreeMap;

use zeff_emu_common::replay::{ReplayEvent, ReplayStartMetadata};

use super::*;
use crate::tas_project::{
    TasControllerInput, TasDeviceIdentity, TasDigest, TasExternalIdentity, TasFirmwareIdentity,
    TasInitialBranch, TasInputFrame, TasInputSpan, TasPatchIdentity, TasProject,
    TasProjectIdentity, TasZapperInput,
};

fn project(
    frame_count: u64,
    input_spans: Vec<TasInputSpan>,
    events: Vec<ReplayEvent>,
) -> TasProject {
    let start_state = vec![0xA5; 128];
    TasProject::new(
        "special_ranges",
        TasProjectIdentity {
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
            sensor_state: TasExternalIdentity::Absent,
            cheats: TasExternalIdentity::Absent,
            state_format_compatibility_id: "nes-state-v7".to_owned(),
            start_state_sha256: TasDigest::from_bytes(&start_state),
        },
        start_state,
        ReplayStartMetadata::default(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count,
            input_spans,
            events,
        },
        BTreeMap::new(),
    )
    .unwrap()
}

fn input(seed: u8) -> TasInputFrame {
    TasInputFrame {
        players: [TasControllerInput {
            buttons: seed,
            dpad: seed.rotate_left(1),
        }; 5],
        zapper: TasZapperInput {
            enabled: true,
            trigger: seed & 1 != 0,
            hit: seed & 2 != 0,
            screen_pos: Some([seed as u16, seed.wrapping_add(1) as u16]),
        },
        tilt_x_bits: if seed & 1 == 0 {
            0x8000_0000
        } else {
            0x3F00_0000
        },
        tilt_y_bits: 0xBF00_0000,
        ..TasInputFrame::default()
    }
}

fn span(start: u64, input: TasInputFrame) -> TasInputSpan {
    TasInputSpan {
        start,
        length: 1,
        input,
    }
}

#[test]
fn prepares_special_ranges_without_mutating_sparse_billion_frame_source_or_events() {
    let starts = [10, 100, crate::tas_project::MAX_PROJECT_FRAMES - 2];
    let events = vec![ReplayEvent::FdsDiskSide {
        frame: 101,
        side: 1,
    }];
    let project = project(
        crate::tas_project::MAX_PROJECT_FRAMES,
        starts
            .into_iter()
            .enumerate()
            .map(|(index, start)| span(start, input(index as u8 + 1)))
            .collect(),
        events.clone(),
    );
    let branch = project.branch("main").unwrap();
    let before = branch.clone();
    let ranges = starts.map(|start| (start, start + 2));
    let prepared = branch
        .prepare_special_transform_ranges(
            &ranges,
            TasSpecialInputMask {
                zapper: true,
                tilt_x: true,
                ..TasSpecialInputMask::default()
            },
            TasSpecialTransform::Clear,
        )
        .unwrap();
    assert_eq!(
        prepared.iter().map(|(start, _)| *start).collect::<Vec<_>>(),
        starts.to_vec()
    );
    for (index, (start, pattern)) in prepared.iter().enumerate() {
        let original = branch.input_at(*start);
        let transformed = pattern.spans()[0].input;
        assert_eq!(transformed.players, original.players, "range {index}");
        assert_eq!(transformed.coleco, original.coleco, "range {index}");
        assert_eq!(transformed.zapper, TasZapperInput::default());
        assert_eq!(transformed.tilt_x_bits, 0);
        assert_eq!(transformed.tilt_y_bits, original.tilt_y_bits);
    }
    assert_eq!(*branch, before);
    assert_eq!(branch.events(), events.as_slice());
    assert_eq!(branch.input_at(50), TasInputFrame::default());
}

#[test]
fn special_range_prepare_reuses_canonical_bounds_and_omits_noops() {
    let project = project(12, vec![span(3, input(1))], Vec::new());
    let branch = project.branch("main").unwrap();
    let before = branch.clone();
    assert!(
        branch
            .prepare_special_transform_ranges(
                &[(2, 5)],
                TasSpecialInputMask {
                    camera: true,
                    ..TasSpecialInputMask::default()
                },
                TasSpecialTransform::Clear,
            )
            .unwrap()
            .is_empty()
    );
    for ranges in [
        Vec::new(),
        vec![(0, 0)],
        vec![(4, 4)],
        vec![(0, 13)],
        vec![(6, 8), (2, 4)],
        vec![(2, 6), (5, 8)],
        vec![(2, 5), (5, 8)],
    ] {
        assert!(
            branch
                .prepare_special_transform_ranges(
                    &ranges,
                    TasSpecialInputMask {
                        zapper: true,
                        ..TasSpecialInputMask::default()
                    },
                    TasSpecialTransform::Clear,
                )
                .is_err()
        );
    }
    assert!(
        branch
            .prepare_special_transform_ranges(
                &(0..65)
                    .map(|index| (index * 2, index * 2 + 1))
                    .collect::<Vec<_>>(),
                TasSpecialInputMask {
                    zapper: true,
                    ..TasSpecialInputMask::default()
                },
                TasSpecialTransform::Clear,
            )
            .is_err()
    );
    assert_eq!(*branch, before);
}

#[test]
fn special_range_prepare_rejects_aggregate_output_over_the_limit() {
    let starts = [0, 5_000];
    let mut spans = Vec::new();
    for start in starts {
        for index in 0..1_025 {
            let mut frame = input(index as u8);
            frame.players[0].buttons = 0x40;
            frame.tilt_x_bits = index as u32 + 1;
            spans.push(span(start + index * 2, frame));
        }
    }
    let project = project(10_000, spans, Vec::new());
    let branch = project.branch("main").unwrap();
    let before = branch.clone();
    let error = branch
        .prepare_special_transform_ranges(
            &[(0, 4_098), (5_000, 9_098)],
            TasSpecialInputMask {
                tilt_x: true,
                ..TasSpecialInputMask::default()
            },
            TasSpecialTransform::Reverse,
        )
        .unwrap_err();
    assert!(error.to_string().contains("4096 spans"));
    assert_eq!(*branch, before);
}
