use super::super::*;

fn pixel(frame: &[u8], x: usize, y: usize) -> [u8; 4] {
    frame[(y * PCE_PRESENTED_WIDTH + x) * 4..][..4]
        .try_into()
        .unwrap()
}

#[test]
fn projection_maps_variable_active_rows_without_sampling_padding() {
    let mut source = vec![0; 4 * 2 * 4];
    let colors = [
        [1, 0, 0, 0xFF],
        [2, 0, 0, 0xFF],
        [3, 0, 0, 0xFF],
        [4, 0, 0, 0xFF],
        [5, 0, 0, 0xFF],
        [6, 0, 0, 0xFF],
        [0xEE, 0, 0, 0xFF],
        [0xEF, 0, 0, 0xFF],
    ];
    for (pixel, color) in source.as_chunks_mut::<4>().0.iter_mut().zip(colors) {
        pixel.copy_from_slice(&color);
    }
    let rows = [
        ProjectionRow {
            active_x_origin: 0,
            active_width: 4,
            pixel_clock_divisor: 1,
            active: true,
        },
        ProjectionRow {
            active_x_origin: 0,
            active_width: 2,
            pixel_clock_divisor: 1,
            active: true,
        },
    ];
    let mut output = vec![0; PCE_PRESENTED_RGBA_BYTES];

    project_sgx_rgba_rows(&source, 4, &rows, Some((0, 2)), &mut output);

    assert_eq!(pixel(&output, 0, 0), colors[0]);
    assert_eq!(pixel(&output, 639, 239), colors[3]);
    assert_eq!(pixel(&output, 159, 240), colors[4]);
    assert_eq!(pixel(&output, 160, 240), colors[5]);
    assert_eq!(pixel(&output, 319, 240), colors[5]);
    assert_eq!(pixel(&output, 320, 240), OPAQUE_BLACK);
    assert_eq!(pixel(&output, 639, 479), OPAQUE_BLACK);
}

#[test]
fn base_projection_scales_each_active_row_without_black_side_bars() {
    let mut source = vec![0; 4 * 2 * 4];
    let colors = [
        [1, 0, 0, 0xFF],
        [2, 0, 0, 0xFF],
        [3, 0, 0, 0xFF],
        [4, 0, 0, 0xFF],
        [5, 0, 0, 0xFF],
        [6, 0, 0, 0xFF],
        [0xEE, 0, 0, 0xFF],
        [0xEF, 0, 0, 0xFF],
    ];
    for (pixel, color) in source.as_chunks_mut::<4>().0.iter_mut().zip(colors) {
        pixel.copy_from_slice(&color);
    }
    let rows = [
        ProjectionRow {
            active_x_origin: 0,
            active_width: 4,
            pixel_clock_divisor: 4,
            active: true,
        },
        ProjectionRow {
            active_x_origin: 7,
            active_width: 2,
            pixel_clock_divisor: 2,
            active: true,
        },
    ];
    let mut output = vec![0; PCE_PRESENTED_RGBA_BYTES];

    project_base_rgba_rows(&source, 4, &rows, Some((0, 2)), &mut output);

    assert_eq!(pixel(&output, 0, 0), colors[0]);
    assert_eq!(pixel(&output, 639, 0), colors[3]);
    assert_eq!(pixel(&output, 0, 479), colors[4]);
    assert_eq!(pixel(&output, 319, 479), colors[4]);
    assert_eq!(pixel(&output, 320, 479), colors[5]);
    assert_eq!(pixel(&output, 639, 479), colors[5]);
}

#[test]
fn base_projection_preserves_the_complete_programmed_active_span() {
    const ACTIVE_WIDTH: usize = 352;
    const ACTIVE_HEIGHT: usize = 240;
    let mut source = vec![0; PCE_ACTIVE_FRAME_WIDTH * ACTIVE_HEIGHT * 4];
    for y in 0..ACTIVE_HEIGHT {
        for x in 0..ACTIVE_WIDTH {
            let offset = (y * PCE_ACTIVE_FRAME_WIDTH + x) * 4;
            source[offset..offset + 4].copy_from_slice(&[x as u8, y as u8, 0x40, 0xFF]);
        }
    }
    let rows = vec![
        ProjectionRow {
            active_x_origin: 0,
            active_width: ACTIVE_WIDTH,
            pixel_clock_divisor: 4,
            active: true,
        };
        ACTIVE_HEIGHT
    ];
    let mut output = vec![0; PCE_PRESENTED_RGBA_BYTES];

    project_base_rgba_rows(
        &source,
        PCE_ACTIVE_FRAME_WIDTH,
        &rows,
        Some((0, ACTIVE_HEIGHT)),
        &mut output,
    );

    assert_eq!(pixel(&output, 0, 0), [0, 0, 0x40, 0xFF]);
    assert_eq!(pixel(&output, 639, 479), [95, 239, 0x40, 0xFF]);
}

#[test]
fn projection_aligns_rows_in_one_master_dot_domain() {
    let mut source = vec![0; 4 * 2 * 4];
    let colors = [
        [1, 0, 0, 0xFF],
        [2, 0, 0, 0xFF],
        [0xA0, 0, 0, 0xFF],
        [4, 0, 0, 0xFF],
        [5, 0, 0, 0xFF],
        [6, 0, 0, 0xFF],
        [0xB0, 0, 0, 0xFF],
        [8, 0, 0, 0xFF],
    ];
    for (pixel, color) in source.as_chunks_mut::<4>().0.iter_mut().zip(colors) {
        pixel.copy_from_slice(&color);
    }
    let rows = [
        ProjectionRow {
            active_x_origin: 0,
            active_width: 4,
            pixel_clock_divisor: 4,
            active: true,
        },
        ProjectionRow {
            active_x_origin: 2,
            active_width: 4,
            pixel_clock_divisor: 2,
            active: true,
        },
    ];
    let mut output = vec![0; PCE_PRESENTED_RGBA_BYTES];

    project_sgx_rgba_rows(&source, 4, &rows, Some((0, 2)), &mut output);

    assert_eq!(pixel(&output, 320, 0), colors[2]);
    assert_eq!(pixel(&output, 320, 479), colors[6]);
    assert_eq!(pixel(&output, 0, 479), OPAQUE_BLACK);
    assert_eq!(pixel(&output, 639, 479), OPAQUE_BLACK);
}

#[test]
fn projection_keeps_empty_and_inactive_rows_opaque_black() {
    let source = vec![0x7F; 4 * 2 * 4];
    let rows = [
        ProjectionRow {
            active_x_origin: 0,
            active_width: 4,
            pixel_clock_divisor: 1,
            active: false,
        },
        ProjectionRow {
            active_x_origin: 0,
            active_width: 4,
            pixel_clock_divisor: 1,
            active: true,
        },
    ];
    let mut output = vec![0; PCE_PRESENTED_RGBA_BYTES];

    project_sgx_rgba_rows(&source, 4, &rows, None, &mut output);
    assert!(
        output
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == OPAQUE_BLACK)
    );

    project_sgx_rgba_rows(&source, 4, &rows, Some((0, 2)), &mut output);
    assert_eq!(pixel(&output, 10, 10), OPAQUE_BLACK);
    assert_eq!(pixel(&output, 10, 470), [0x7F; 4]);
}

#[test]
fn fixed_signal_window_preserves_224_margins_and_distinguishes_239_from_240() {
    let first = usize::from(zeff_pce_core::hardware::PCE_SIGNAL_FIRST_ROW);
    let end = usize::from(zeff_pce_core::hardware::PCE_SIGNAL_ROW_END);

    let project = |active_start: usize, active_end: usize, final_color: [u8; 4]| {
        let mut source = vec![0; zeff_pce_core::hardware::PCE_ACTIVE_FRAME_HEIGHT * 4];
        for pixel in source.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&OPAQUE_BLACK);
        }
        let rows = std::array::from_fn::<_, { zeff_pce_core::hardware::PCE_ACTIVE_FRAME_HEIGHT }, _>(
            |line| ProjectionRow {
                active_x_origin: 0,
                active_width: 1,
                pixel_clock_divisor: 4,
                active: (active_start..active_end).contains(&line),
            },
        );
        for line in active_start..active_end {
            source[line * 4..line * 4 + 4].copy_from_slice(&[0x40, line as u8, 0, 0xFF]);
        }
        source[(active_end - 1) * 4..active_end * 4].copy_from_slice(&final_color);
        let mut base = vec![0; PCE_PRESENTED_RGBA_BYTES];
        let mut supergrafx = vec![0; PCE_PRESENTED_RGBA_BYTES];
        project_base_rgba_rows(&source, 1, &rows, Some((first, end)), &mut base);
        project_sgx_rgba_rows(&source, 1, &rows, Some((first, end)), &mut supergrafx);
        assert_eq!(base, supergrafx);
        base
    };

    let mode_224 = project(28, 252, [0xE0, 0, 0, 0xFF]);
    assert_eq!(pixel(&mode_224, 0, 0), OPAQUE_BLACK);
    assert_ne!(pixel(&mode_224, 0, 22), OPAQUE_BLACK);
    assert_eq!(pixel(&mode_224, 0, PCE_PRESENTED_HEIGHT - 1), OPAQUE_BLACK);

    let mode_239 = project(20, 260, [0xEF, 0, 0, 0xFF]);
    assert_ne!(
        pixel(&mode_239, 0, PCE_PRESENTED_HEIGHT - 1),
        [0xEF, 0, 0, 0xFF]
    );

    let mode_240 = project(19, 259, [0xF0, 0, 0, 0xFF]);
    assert_eq!(
        pixel(&mode_240, 0, PCE_PRESENTED_HEIGHT - 1),
        [0xF0, 0, 0, 0xFF]
    );
    assert_ne!(mode_239, mode_240);
}
