use super::*;
use crate::tas_project::{MAX_PROJECT_FRAMES, TasCameraInput, TasDigest};

fn at(pattern: &TasInputPattern, frame: u64) -> TasInputFrame {
    pattern
        .spans()
        .iter()
        .find(|span| (span.start..span.start + span.length).contains(&frame))
        .map_or_else(TasInputFrame::default, |span| span.input)
}

fn mask() -> TasDigitalInputMask {
    let mut mask = TasDigitalInputMask::default();
    mask.players[0] = TasControllerInput {
        buttons: 0b0000_0101,
        dpad: 0b0000_1010,
    };
    mask.players[3] = TasControllerInput {
        buttons: 0b1000_0000,
        dpad: 0b0100_0000,
    };
    mask
}

fn varied_input(seed: u8) -> TasInputFrame {
    let mut input = TasInputFrame::default();
    for (player, controller) in input.players.iter_mut().enumerate() {
        controller.buttons = seed.wrapping_add(player as u8);
        controller.dpad = seed.rotate_left(player as u32);
    }
    input.tilt_x_bits = 0.25f32.to_bits();
    input.tilt_y_bits = (-0.75f32).to_bits();
    input.zapper.enabled = true;
    input.zapper.screen_pos = Some([100, 120]);
    input.coleco[0].left_button = true;
    input.camera = TasCameraInput::Blob(TasDigest::from_bytes(&[seed; 32]));
    input
}

fn source() -> TasInputPattern {
    TasInputPattern::new(
        17,
        vec![
            TasInputSpan {
                start: 1,
                length: 2,
                input: varied_input(0xA5),
            },
            TasInputSpan {
                start: 5,
                length: 4,
                input: varied_input(0x5A),
            },
            TasInputSpan {
                start: 12,
                length: 3,
                input: varied_input(0x1F),
            },
        ],
    )
    .unwrap()
}

fn transform_frame(
    source: TasInputFrame,
    mirrored: TasInputFrame,
    mask: TasDigitalInputMask,
    transform: TasDigitalTransform,
) -> TasInputFrame {
    let mut expected = source;
    for player in 0..5 {
        for bit in 0..8 {
            let bit = 1 << bit;
            if mask.players[player].buttons & bit != 0 {
                let pressed = match transform {
                    TasDigitalTransform::Clear => false,
                    TasDigitalTransform::Invert => source.players[player].buttons & bit == 0,
                    TasDigitalTransform::Reverse => mirrored.players[player].buttons & bit != 0,
                };
                set_bit(&mut expected.players[player].buttons, bit, pressed);
            }
            if mask.players[player].dpad & bit != 0 {
                let pressed = match transform {
                    TasDigitalTransform::Clear => false,
                    TasDigitalTransform::Invert => source.players[player].dpad & bit == 0,
                    TasDigitalTransform::Reverse => mirrored.players[player].dpad & bit != 0,
                };
                set_bit(&mut expected.players[player].dpad, bit, pressed);
            }
        }
    }
    expected
}

fn set_bit(value: &mut u8, bit: u8, pressed: bool) {
    if pressed {
        *value |= bit;
    } else {
        *value &= !bit;
    }
}

fn overlay_frame(
    mut destination: TasInputFrame,
    source: TasInputFrame,
    mask: TasDigitalInputMask,
) -> TasInputFrame {
    for player in 0..5 {
        for bit in 0..8 {
            let bit = 1 << bit;
            if mask.players[player].buttons & bit != 0 {
                set_bit(
                    &mut destination.players[player].buttons,
                    bit,
                    source.players[player].buttons & bit != 0,
                );
            }
            if mask.players[player].dpad & bit != 0 {
                set_bit(
                    &mut destination.players[player].dpad,
                    bit,
                    source.players[player].dpad & bit != 0,
                );
            }
        }
    }
    destination
}

#[test]
fn transforms_match_a_naive_frame_oracle_and_preserve_special_channels() {
    let source = source();
    let mask = mask();
    for transform in [
        TasDigitalTransform::Clear,
        TasDigitalTransform::Invert,
        TasDigitalTransform::Reverse,
    ] {
        let transformed = source.with_digital_transform(mask, transform).unwrap();
        for frame in 0..source.length() {
            let original = at(&source, frame);
            let expected = transform_frame(
                original,
                at(&source, source.length() - 1 - frame),
                mask,
                transform,
            );
            assert_eq!(
                at(&transformed, frame),
                expected,
                "{transform:?} frame={frame}"
            );
        }
    }
}

#[test]
fn clear_and_invert_only_change_selected_bits() {
    let source = source();
    let mask = mask();
    let cleared = source
        .with_digital_transform(mask, TasDigitalTransform::Clear)
        .unwrap();
    let inverted = source
        .with_digital_transform(mask, TasDigitalTransform::Invert)
        .unwrap();
    for frame in 0..source.length() {
        let original = at(&source, frame);
        let clear = at(&cleared, frame);
        let invert = at(&inverted, frame);
        for player in 0..5 {
            assert_eq!(
                clear.players[player].buttons & !mask.players[player].buttons,
                original.players[player].buttons & !mask.players[player].buttons
            );
            assert_eq!(
                clear.players[player].dpad & !mask.players[player].dpad,
                original.players[player].dpad & !mask.players[player].dpad
            );
            assert_eq!(
                invert.players[player].buttons & !mask.players[player].buttons,
                original.players[player].buttons & !mask.players[player].buttons
            );
            assert_eq!(
                invert.players[player].dpad & !mask.players[player].dpad,
                original.players[player].dpad & !mask.players[player].dpad
            );
        }
        assert_eq!(clear.coleco, original.coleco);
        assert_eq!(clear.zapper, original.zapper);
        assert_eq!(clear.camera, original.camera);
        assert_eq!(
            (clear.tilt_x_bits, clear.tilt_y_bits),
            (original.tilt_x_bits, original.tilt_y_bits)
        );
    }
}

#[test]
fn reversal_handles_odd_boundaries_neutral_gaps_and_is_an_involution() {
    let source = TasInputPattern::new(
        9,
        vec![
            TasInputSpan {
                start: 0,
                length: 1,
                input: varied_input(1),
            },
            TasInputSpan {
                start: 3,
                length: 2,
                input: varied_input(2),
            },
            TasInputSpan {
                start: 8,
                length: 1,
                input: varied_input(3),
            },
        ],
    )
    .unwrap();
    let mask = mask();
    let reversed = source
        .with_digital_transform(mask, TasDigitalTransform::Reverse)
        .unwrap();
    for frame in 0..source.length() {
        let original = at(&source, frame);
        let mirrored = at(&source, source.length() - 1 - frame);
        assert_eq!(
            at(&reversed, frame),
            transform_frame(original, mirrored, mask, TasDigitalTransform::Reverse)
        );
    }
    assert_eq!(
        reversed
            .with_digital_transform(mask, TasDigitalTransform::Reverse)
            .unwrap(),
        source
    );
}

#[test]
fn overlay_matches_a_per_bit_oracle_across_unequal_sparse_runs() {
    let destination = TasInputPattern::new(
        17,
        vec![
            TasInputSpan {
                start: 0,
                length: 2,
                input: varied_input(0x11),
            },
            TasInputSpan {
                start: 4,
                length: 3,
                input: varied_input(0x22),
            },
            TasInputSpan {
                start: 10,
                length: 5,
                input: varied_input(0x33),
            },
        ],
    )
    .unwrap();
    let source = TasInputPattern::new(
        17,
        vec![
            TasInputSpan {
                start: 1,
                length: 1,
                input: varied_input(0xA5),
            },
            TasInputSpan {
                start: 3,
                length: 4,
                input: varied_input(0x5A),
            },
            TasInputSpan {
                start: 8,
                length: 2,
                input: varied_input(0x1F),
            },
            TasInputSpan {
                start: 15,
                length: 2,
                input: varied_input(0xC3),
            },
        ],
    )
    .unwrap();
    let mask = mask();
    let source_before = source.clone();
    let overlaid = destination.with_digital_overlay(&source, mask).unwrap();

    for frame in 0..destination.length() {
        let original = at(&destination, frame);
        assert_eq!(
            at(&overlaid, frame),
            overlay_frame(original, at(&source, frame), mask),
            "frame={frame}"
        );
        assert_eq!(at(&overlaid, frame).zapper, original.zapper);
        assert_eq!(at(&overlaid, frame).coleco, original.coleco);
        assert_eq!(at(&overlaid, frame).camera, original.camera);
        assert_eq!(
            (
                at(&overlaid, frame).tilt_x_bits,
                at(&overlaid, frame).tilt_y_bits
            ),
            (original.tilt_x_bits, original.tilt_y_bits)
        );
    }
    assert_eq!(source, source_before);
    assert_eq!(
        overlaid.with_digital_overlay(&source, mask).unwrap(),
        overlaid
    );
}

#[test]
fn overlay_rejects_invalid_inputs_and_enforces_sparse_limits() {
    let mask = mask();
    let destination = source();
    assert!(
        destination
            .with_digital_overlay(&TasInputPattern::neutral(16).unwrap(), mask)
            .is_err()
    );
    assert!(
        destination
            .with_digital_overlay(&source(), TasDigitalInputMask::default())
            .is_err()
    );

    let mut selected = TasInputFrame::default();
    selected.players[0].buttons = mask.players[0].buttons;
    let source = TasInputPattern::constant(MAX_PROJECT_FRAMES, selected).unwrap();
    let compact = TasInputPattern::neutral(MAX_PROJECT_FRAMES)
        .unwrap()
        .with_digital_overlay(&source, mask)
        .unwrap();
    assert_eq!(compact.spans().len(), 1);
    assert_eq!(compact.spans()[0].length, MAX_PROJECT_FRAMES);

    let mut repeated = Vec::new();
    for index in 0..MAX_TAS_INPUT_PATTERN_SPANS {
        repeated.push(TasInputSpan {
            start: index as u64 * 2,
            length: 1,
            input: selected,
        });
    }
    let destination = TasInputPattern::new(8_200, repeated).unwrap();
    let source = TasInputPattern::new(
        8_200,
        vec![TasInputSpan {
            start: 8_192,
            length: 1,
            input: selected,
        }],
    )
    .unwrap();
    assert!(
        destination
            .with_digital_overlay(&source, mask)
            .unwrap_err()
            .to_string()
            .contains("candidate")
    );

    let special = TasInputFrame {
        zapper: crate::tas_project::TasZapperInput {
            enabled: true,
            ..crate::tas_project::TasZapperInput::default()
        },
        ..TasInputFrame::default()
    };
    let destination = TasInputPattern::new(
        (MAX_TAS_INPUT_PATTERN_SPANS as u64 + 2) * 2,
        (0..(MAX_TAS_INPUT_PATTERN_SPANS / 2 + 1))
            .map(|index| TasInputSpan {
                start: index as u64 * 2 + 1,
                length: 1,
                input: special,
            })
            .collect(),
    )
    .unwrap();
    let source = TasInputPattern::constant(destination.length(), selected).unwrap();
    assert!(destination.with_digital_overlay(&source, mask).is_err());
}

#[test]
fn transforms_stay_compact_at_maximum_length_and_reject_empty_work_and_output_overflow() {
    let mask = mask();
    let source = TasInputPattern::neutral(MAX_PROJECT_FRAMES).unwrap();
    assert_eq!(
        source
            .with_digital_transform(mask, TasDigitalTransform::Clear)
            .unwrap(),
        source
    );
    let inverted = source
        .with_digital_transform(mask, TasDigitalTransform::Invert)
        .unwrap();
    assert_eq!(inverted.spans().len(), 1);
    assert_eq!(inverted.spans()[0].length, MAX_PROJECT_FRAMES);
    assert_eq!(
        inverted
            .with_digital_transform(mask, TasDigitalTransform::Invert)
            .unwrap(),
        source
    );
    assert!(
        source
            .with_digital_transform(TasDigitalInputMask::default(), TasDigitalTransform::Clear)
            .is_err()
    );

    let mut work_spans = Vec::new();
    let mut start = 1;
    for index in 0..MAX_TAS_INPUT_PATTERN_SPANS {
        let mut input = TasInputFrame::default();
        input.players[0].buttons = 1;
        work_spans.push(TasInputSpan {
            start,
            length: 1,
            input,
        });
        if index + 1 < MAX_TAS_INPUT_PATTERN_SPANS {
            start += 2 + (index % 2) as u64;
        }
    }
    let work = TasInputPattern::new(start + 1, work_spans).unwrap();
    assert!(
        work.with_digital_transform(mask, TasDigitalTransform::Reverse)
            .unwrap_err()
            .to_string()
            .contains("candidate")
    );

    let output = (0..(MAX_TAS_INPUT_PATTERN_SPANS / 2 + 1))
        .map(|index| TasInputSpan {
            start: index as u64 * 2 + 1,
            length: 1,
            input: TasInputFrame {
                players: [TasControllerInput {
                    buttons: 0,
                    dpad: 0,
                }; 5],
                zapper: crate::tas_project::TasZapperInput {
                    enabled: true,
                    ..crate::tas_project::TasZapperInput::default()
                },
                ..TasInputFrame::default()
            },
        })
        .collect();
    let output =
        TasInputPattern::new((MAX_TAS_INPUT_PATTERN_SPANS as u64 + 2) * 2, output).unwrap();
    assert!(
        output
            .with_digital_transform(mask, TasDigitalTransform::Invert)
            .is_err()
    );
}
