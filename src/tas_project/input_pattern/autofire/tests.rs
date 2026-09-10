use super::*;
use crate::tas_project::{MAX_PROJECT_FRAMES, TasCameraInput, TasDigest};

fn at(pattern: &TasInputPattern, frame: u64) -> TasInputFrame {
    pattern
        .spans()
        .iter()
        .find(|span| (span.start..span.start + span.length).contains(&frame))
        .map_or_else(TasInputFrame::default, |span| span.input)
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

#[test]
fn sparse_autofire_matches_frame_oracle_and_preserves_other_channels() {
    let source = TasInputPattern::new(
        31,
        vec![
            TasInputSpan {
                start: 2,
                length: 3,
                input: varied_input(0xA5),
            },
            TasInputSpan {
                start: 9,
                length: 6,
                input: varied_input(0x5A),
            },
            TasInputSpan {
                start: 20,
                length: 11,
                input: varied_input(0x1F),
            },
        ],
    )
    .unwrap();
    for player in 0..5 {
        for period in 1..=6 {
            for on_frames in 0..=period {
                let result = source
                    .with_digital_autofire(player, 0x05, 0x0A, period, on_frames)
                    .unwrap();
                for frame in 0..source.length() {
                    let mut expected = at(&source, frame);
                    let pressed = frame % u64::from(period) < u64::from(on_frames);
                    for bit in [0, 2] {
                        expected.players[player].buttons &= !(1 << bit);
                        if pressed {
                            expected.players[player].buttons |= 1 << bit;
                        }
                    }
                    for bit in [1, 3] {
                        expected.players[player].dpad &= !(1 << bit);
                        if pressed {
                            expected.players[player].dpad |= 1 << bit;
                        }
                    }
                    assert_eq!(
                        at(&result, frame),
                        expected,
                        "player={player} period={period} on={on_frames} frame={frame}"
                    );
                }
            }
        }
    }
}

#[test]
fn autofire_anchors_to_the_selection_start_and_clips_final_cycle() {
    let result = TasInputPattern::neutral(8)
        .unwrap()
        .with_digital_autofire(0, 1, 0, 3, 2)
        .unwrap();
    let mut input = TasInputFrame::default();
    input.players[0].buttons = 1;
    assert_eq!(
        result.spans(),
        &[
            TasInputSpan {
                start: 0,
                length: 2,
                input
            },
            TasInputSpan {
                start: 3,
                length: 2,
                input
            },
            TasInputSpan {
                start: 6,
                length: 2,
                input
            },
        ]
    );
    assert_eq!(result.with_digital_autofire(0, 1, 0, 3, 2).unwrap(), result);
}

#[test]
fn constant_autofire_is_compact_for_maximum_length_and_clears_neutral_runs() {
    let source = TasInputPattern::neutral(MAX_PROJECT_FRAMES).unwrap();
    let held = source.with_digital_autofire(4, 0x20, 0, 60, 60).unwrap();
    assert_eq!(held.spans().len(), 1);
    assert_eq!(held.spans()[0].length, MAX_PROJECT_FRAMES);
    assert_eq!(
        held.with_digital_autofire(4, 0x20, 0, 60, 0).unwrap(),
        source
    );
}

#[test]
fn autofire_rejects_invalid_controls_and_excessive_work() {
    let source = TasInputPattern::neutral(MAX_PROJECT_FRAMES).unwrap();
    for (player, buttons, dpad, period, on_frames) in [
        (5, 1, 0, 2, 1),
        (0, 0, 0, 2, 1),
        (0, 1, 0, 0, 0),
        (0, 1, 0, 61, 1),
        (0, 1, 0, 2, 3),
    ] {
        assert!(
            source
                .with_digital_autofire(player, buttons, dpad, period, on_frames)
                .is_err()
        );
    }
    assert!(
        source
            .with_digital_autofire(0, 1, 0, 2, 1)
            .unwrap_err()
            .to_string()
            .contains("limit")
    );
    assert!(source.spans().is_empty());
    let boundary = TasInputPattern::neutral(MAX_TAS_INPUT_PATTERN_TILE_STEPS as u64)
        .unwrap()
        .with_digital_autofire(0, 1, 0, 2, 1)
        .unwrap();
    assert_eq!(boundary.spans().len(), MAX_TAS_INPUT_PATTERN_SPANS);
}
