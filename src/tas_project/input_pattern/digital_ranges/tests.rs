use std::collections::BTreeMap;

use zeff_emu_common::replay::{ReplayEvent, ReplayStartMetadata};

use super::*;
use crate::tas_project::{
    TasCameraInput, TasControllerInput, TasDeviceIdentity, TasDigest, TasExternalIdentity,
    TasFirmwareIdentity, TasInitialBranch, TasInputFrame, TasInputSpan, TasPatchIdentity,
    TasProject, TasProjectIdentity,
};

const CAMERA_BYTES: &[u8] = b"digital-range-camera";

fn camera() -> TasCameraInput {
    TasCameraInput::Blob(TasDigest::from_bytes(CAMERA_BYTES))
}

fn branch(
    frame_count: u64,
    input_spans: Vec<TasInputSpan>,
    events: Vec<ReplayEvent>,
) -> TasProject {
    let start_state = vec![0xA5; 128];
    let camera_digest = TasDigest::from_bytes(CAMERA_BYTES);
    TasProject::new(
        "digital_ranges",
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
        BTreeMap::from([(camera_digest, CAMERA_BYTES.to_vec())]),
    )
    .unwrap()
}
fn frame(seed: u8, camera: TasCameraInput) -> TasInputFrame {
    let mut input = TasInputFrame::default();
    for (index, player) in input.players.iter_mut().enumerate() {
        player.buttons = seed.wrapping_add(index as u8).rotate_left(index as u32);
        player.dpad = seed.wrapping_mul(3).rotate_right(index as u32);
    }
    input.tilt_x_bits = (seed as f32 * 0.25).to_bits();
    input.tilt_y_bits = (-(seed as f32) * 0.5).to_bits();
    input.zapper.enabled = seed & 1 != 0;
    input.zapper.trigger = seed & 2 != 0;
    input.zapper.hit = seed & 4 != 0;
    input.zapper.screen_pos = Some([seed as u16, seed.wrapping_add(1) as u16]);
    input.coleco[0].left_button = seed & 8 != 0;
    input.camera = camera;
    input
}

fn span(start: u64, input: TasInputFrame) -> TasInputSpan {
    TasInputSpan {
        start,
        length: 1,
        input,
    }
}

fn mask() -> TasDigitalInputMask {
    TasDigitalInputMask {
        players: [
            TasControllerInput {
                buttons: 0b1000_0001,
                dpad: 0b0100_0010,
            },
            TasControllerInput {
                buttons: 0b0100_0010,
                dpad: 0b0010_0100,
            },
            TasControllerInput {
                buttons: 0b0010_0100,
                dpad: 0b0001_1000,
            },
            TasControllerInput {
                buttons: 0b0001_1000,
                dpad: 0b1000_0001,
            },
            TasControllerInput {
                buttons: 0b0000_0110,
                dpad: 0b0100_0010,
            },
        ],
    }
}

fn at(pattern: &TasInputPattern, offset: u64) -> TasInputFrame {
    pattern
        .spans()
        .iter()
        .find(|span| (span.start..span.start + span.length).contains(&offset))
        .map_or_else(TasInputFrame::default, |span| span.input)
}

fn transformed(
    source: TasInputFrame,
    mirrored: TasInputFrame,
    mask: TasDigitalInputMask,
    transform: TasDigitalTransform,
) -> TasInputFrame {
    let mut expected = source;
    for ((player, mirrored), selected) in expected
        .players
        .iter_mut()
        .zip(mirrored.players)
        .zip(mask.players)
    {
        match transform {
            TasDigitalTransform::Clear => {
                player.buttons &= !selected.buttons;
                player.dpad &= !selected.dpad;
            }
            TasDigitalTransform::Invert => {
                player.buttons ^= selected.buttons;
                player.dpad ^= selected.dpad;
            }
            TasDigitalTransform::Reverse => {
                player.buttons =
                    (player.buttons & !selected.buttons) | (mirrored.buttons & selected.buttons);
                player.dpad = (player.dpad & !selected.dpad) | (mirrored.dpad & selected.dpad);
            }
        }
    }
    expected
}

#[test]
fn prepares_sparse_billion_frame_ranges_without_touching_gaps_events_or_special_channels() {
    let camera = camera();
    let starts = [10, 100, MAX_PROJECT_FRAMES - 3];
    let mut spans = Vec::new();
    for (range_index, start) in starts.into_iter().enumerate() {
        for offset in 0..3 {
            spans.push(span(
                start + offset,
                frame(0x30 + range_index as u8 * 7 + offset as u8, camera),
            ));
        }
    }
    let events = vec![ReplayEvent::FdsDiskSide {
        frame: 101,
        side: 1,
    }];
    let project = branch(MAX_PROJECT_FRAMES, spans, events.clone());
    let source = project.branch("main").unwrap();
    let source_spans = source.input_spans().to_vec();
    let ranges = starts.map(|start| (start, start + 3));

    for transform in [
        TasDigitalTransform::Clear,
        TasDigitalTransform::Invert,
        TasDigitalTransform::Reverse,
    ] {
        let prepared = source
            .prepare_digital_transform_ranges(&ranges, mask(), transform)
            .unwrap();
        assert_eq!(
            prepared.iter().map(|(start, _)| *start).collect::<Vec<_>>(),
            starts.to_vec()
        );
        for ((start, pattern), (_, end)) in prepared.iter().zip(ranges) {
            assert_eq!(pattern.length(), end - *start);
            for offset in 0..pattern.length() {
                let original = source.input_at(*start + offset);
                let mirrored = source.input_at(end - 1 - offset);
                assert_eq!(
                    at(pattern, offset),
                    transformed(original, mirrored, mask(), transform)
                );
            }
        }
    }

    assert_eq!(source.input_spans(), source_spans);
    assert_eq!(source.events(), events.as_slice());
    assert_eq!(source.input_at(50), TasInputFrame::default());
    assert_eq!(source.input_at(99), TasInputFrame::default());
    assert_eq!(
        source.input_at(MAX_PROJECT_FRAMES - 4),
        TasInputFrame::default()
    );
}

#[test]
fn omits_noop_ranges_and_rejects_noncanonical_or_out_of_bounds_ranges_without_mutation() {
    let project = branch(
        12,
        vec![span(
            3,
            TasInputFrame {
                players: [TasControllerInput {
                    buttons: 1,
                    dpad: 2,
                }; 5],
                ..TasInputFrame::default()
            },
        )],
        Vec::new(),
    );
    let source = project.branch("main").unwrap();
    let before = source.clone();
    let absent = TasDigitalInputMask {
        players: [TasControllerInput {
            buttons: 0x80,
            dpad: 0x80,
        }; 5],
    };
    assert!(
        source
            .prepare_digital_transform_ranges(&[(2, 5)], absent, TasDigitalTransform::Clear)
            .unwrap()
            .is_empty()
    );

    let invalid = [
        Vec::new(),
        vec![(0, 0)],
        vec![(5, 5)],
        vec![(0, 13)],
        vec![(6, 8), (2, 4)],
        vec![(2, 6), (5, 8)],
        vec![(2, 5), (5, 8)],
    ];
    for ranges in invalid {
        assert!(
            source
                .prepare_digital_transform_ranges(&ranges, mask(), TasDigitalTransform::Clear)
                .is_err()
        );
    }
    assert!(
        source
            .prepare_digital_transform_ranges(
                &[(0, MAX_PROJECT_FRAMES + 1)],
                mask(),
                TasDigitalTransform::Clear,
            )
            .is_err()
    );
    assert!(
        source
            .prepare_digital_transform_ranges(
                &(0..65)
                    .map(|index| (index * 2, index * 2 + 1))
                    .collect::<Vec<_>>(),
                mask(),
                TasDigitalTransform::Clear,
            )
            .is_err()
    );
    assert_eq!(*source, before);
}

#[test]
fn rejects_aggregate_prepared_pattern_spans_over_the_limit() {
    let camera = camera();
    let mut spans = Vec::new();
    let starts = [0, 3_000];
    for start in starts {
        for index in 0..1_025 {
            let mut input = frame(index as u8, camera);
            input.players[0].buttons = 0x40;
            input.players[0].dpad = 0;
            spans.push(span(start + index * 2, input));
        }
    }
    let project = branch(6_000, spans, Vec::new());
    let source = project.branch("main").unwrap();
    let before = source.clone();
    let selected = TasDigitalInputMask {
        players: [TasControllerInput {
            buttons: 1,
            dpad: 0,
        }; 5],
    };

    let error = source
        .prepare_digital_transform_ranges(
            &[(0, 2_049), (3_000, 5_049)],
            selected,
            TasDigitalTransform::Invert,
        )
        .unwrap_err();
    assert!(error.to_string().contains("4096 spans"));
    assert_eq!(*source, before);
}
