use super::*;
use crate::tas_project::{
    TasCameraInput, TasColecoControllerInput, TasControllerInput, TasDigest, TasInputFrame,
    TasInputSpan, TasZapperInput,
};

fn frame(seed: u8) -> TasInputFrame {
    TasInputFrame {
        players: [TasControllerInput {
            buttons: seed,
            dpad: seed.rotate_left(1),
        }; 5],
        coleco: [TasColecoControllerInput {
            left_button: seed & 1 != 0,
            right_button: seed & 2 != 0,
            ..TasColecoControllerInput::default()
        }; 2],
        zapper: TasZapperInput {
            enabled: seed & 1 != 0,
            trigger: seed & 2 != 0,
            hit: seed & 4 != 0,
            screen_pos: Some([seed as u16, seed.wrapping_add(1) as u16]),
        },
        tilt_x_bits: if seed & 1 == 0 {
            0x8000_0000
        } else {
            0x3F40_0000
        },
        tilt_y_bits: if seed & 2 == 0 {
            0xBF00_0000
        } else {
            0x7FC0_0123
        },
        camera: TasCameraInput::Blob(TasDigest([seed; 32])),
    }
}

fn span(start: u64, input: TasInputFrame) -> TasInputSpan {
    TasInputSpan {
        start,
        length: 1,
        input,
    }
}

fn at(pattern: &TasInputPattern, frame: u64) -> TasInputFrame {
    pattern
        .spans()
        .iter()
        .find(|span| (span.start..span.start + span.length).contains(&frame))
        .map_or_else(TasInputFrame::default, |span| span.input)
}

fn clear_expected(mut input: TasInputFrame, mask: TasSpecialInputMask) -> TasInputFrame {
    if mask.zapper {
        input.zapper = TasZapperInput::default();
    }
    if mask.tilt_x {
        input.tilt_x_bits = 0;
    }
    if mask.tilt_y {
        input.tilt_y_bits = 0;
    }
    if mask.camera {
        input.camera = TasCameraInput::None;
    }
    input
}

fn reverse_expected(
    mut input: TasInputFrame,
    mirrored: TasInputFrame,
    mask: TasSpecialInputMask,
) -> TasInputFrame {
    if mask.zapper {
        input.zapper = mirrored.zapper;
    }
    if mask.tilt_x {
        input.tilt_x_bits = mirrored.tilt_x_bits;
    }
    if mask.tilt_y {
        input.tilt_y_bits = mirrored.tilt_y_bits;
    }
    if mask.camera {
        input.camera = mirrored.camera;
    }
    input
}

#[test]
fn clear_changes_only_selected_special_channels_and_preserves_all_controller_data() {
    let source = TasInputPattern::new(6, vec![span(1, frame(1)), span(3, frame(2))]).unwrap();
    for mask in [
        TasSpecialInputMask {
            zapper: true,
            ..TasSpecialInputMask::default()
        },
        TasSpecialInputMask {
            tilt_x: true,
            ..TasSpecialInputMask::default()
        },
        TasSpecialInputMask {
            tilt_y: true,
            ..TasSpecialInputMask::default()
        },
        TasSpecialInputMask {
            camera: true,
            ..TasSpecialInputMask::default()
        },
        TasSpecialInputMask {
            zapper: true,
            tilt_x: true,
            camera: true,
            ..TasSpecialInputMask::default()
        },
    ] {
        let cleared = source
            .with_special_transform(mask, TasSpecialTransform::Clear)
            .unwrap();
        for cursor in 0..source.length() {
            let original = at(&source, cursor);
            let actual = at(&cleared, cursor);
            assert_eq!(actual, clear_expected(original, mask));
            assert_eq!(actual.players, original.players);
            assert_eq!(actual.coleco, original.coleco);
        }
    }
}

#[test]
fn reverse_mirrors_only_selected_channels_through_implicit_neutral_gaps() {
    let source = TasInputPattern::new(7, vec![span(1, frame(1)), span(4, frame(2))]).unwrap();
    let mask = TasSpecialInputMask {
        zapper: true,
        tilt_x: true,
        tilt_y: true,
        camera: true,
    };
    let reversed = source
        .with_special_transform(mask, TasSpecialTransform::Reverse)
        .unwrap();
    for cursor in 0..source.length() {
        let original = at(&source, cursor);
        let mirrored = at(&source, source.length() - 1 - cursor);
        let actual = at(&reversed, cursor);
        assert_eq!(actual, reverse_expected(original, mirrored, mask));
        assert_eq!(actual.players, original.players);
        assert_eq!(actual.coleco, original.coleco);
    }
    assert_eq!(
        at(&reversed, 2).camera,
        TasCameraInput::Blob(TasDigest([2; 32]))
    );
    assert_eq!(
        at(&reversed, 5).camera,
        TasCameraInput::Blob(TasDigest([1; 32]))
    );
    assert_eq!(at(&reversed, 2).tilt_x_bits, 0x8000_0000);
    assert_eq!(at(&reversed, 5).tilt_y_bits, 0xBF00_0000);
}

#[test]
fn no_op_clear_and_empty_mask_are_bounded_and_exact() {
    let source = TasInputPattern::neutral(9).unwrap();
    assert_eq!(
        source
            .with_special_transform(
                TasSpecialInputMask {
                    camera: true,
                    ..TasSpecialInputMask::default()
                },
                TasSpecialTransform::Clear,
            )
            .unwrap(),
        source
    );
    assert!(
        source
            .with_special_transform(TasSpecialInputMask::default(), TasSpecialTransform::Clear)
            .is_err()
    );

    let billion = TasInputPattern::new(
        crate::tas_project::MAX_PROJECT_FRAMES,
        vec![span(0, frame(1))],
    )
    .unwrap();
    let reversed = billion
        .with_special_transform(
            TasSpecialInputMask {
                zapper: true,
                ..TasSpecialInputMask::default()
            },
            TasSpecialTransform::Reverse,
        )
        .unwrap();
    assert_eq!(reversed.spans().len(), 2);
    assert_eq!(at(&reversed, 0).zapper, TasZapperInput::default());
    assert_eq!(
        at(&reversed, crate::tas_project::MAX_PROJECT_FRAMES - 1).zapper,
        frame(1).zapper
    );
}

#[test]
fn reverse_enforces_sparse_candidate_work_limit() {
    let mut spans = Vec::new();
    let mut start = 1;
    for index in 0..MAX_TAS_INPUT_PATTERN_SPANS {
        spans.push(span(start, frame(index as u8)));
        if index + 1 < MAX_TAS_INPUT_PATTERN_SPANS {
            start += 2 + (index % 2) as u64;
        }
    }
    let source = TasInputPattern::new(start + 2, spans).unwrap();
    let error = source
        .with_special_transform(
            TasSpecialInputMask {
                camera: true,
                ..TasSpecialInputMask::default()
            },
            TasSpecialTransform::Reverse,
        )
        .unwrap_err();
    assert!(error.to_string().contains("candidate"));
}
