use super::{
    PCE_ACTIVE_FRAME_HEIGHT, PCE_ACTIVE_FRAME_WIDTH, PceHardwareTopology, PcePresentedFrame,
};

pub const PCE_HOST_FRAME_WIDTH: usize = zeff_emu_common::system::PCE_SCREEN_SIZE.0 as usize;
pub const PCE_HOST_FRAME_HEIGHT: usize = zeff_emu_common::system::PCE_SCREEN_SIZE.1 as usize;
pub const PCE_HOST_FRAME_RGBA_BYTES: usize =
    PCE_HOST_FRAME_WIDTH * PCE_HOST_FRAME_HEIGHT * zeff_emu_common::system::RGBA_BYTES_PER_PIXEL;
const OPAQUE_BLACK: [u8; 4] = [0, 0, 0, 0xFF];

#[derive(Clone, Copy, Default)]
struct ProjectionRow {
    active_x_origin: usize,
    active_width: usize,
    pixel_clock_divisor: usize,
    active: bool,
}

/// Projects the full PCE signal window onto the fixed 4:3 host canvas.
pub fn project_full_raw_frame(
    frame: PcePresentedFrame<'_>,
    topology: PceHardwareTopology,
    output: &mut [u8],
) {
    assert_eq!(output.len(), PCE_HOST_FRAME_RGBA_BYTES);
    clear_output(output);

    let rows = std::array::from_fn::<_, PCE_ACTIVE_FRAME_HEIGHT, _>(|line| {
        let metadata = frame.rows()[line];
        ProjectionRow {
            active_x_origin: usize::from(metadata.active_x_origin()),
            active_width: usize::from(metadata.active_width()),
            pixel_clock_divisor: metadata
                .pixel_clock()
                .map_or(0, |clock| usize::from(clock.divisor())),
            active: metadata.is_active(),
        }
    });
    let signal = frame.signal_bounds();
    let first_row = usize::from(signal.first_row());
    let row_end = usize::from(signal.row_end());
    if first_row >= row_end || row_end > rows.len() {
        return;
    }

    match topology {
        PceHardwareTopology::Base => project_base(
            frame.rgba(),
            PCE_ACTIVE_FRAME_WIDTH,
            &rows,
            first_row,
            row_end,
            output,
        ),
        PceHardwareTopology::SuperGrafx => project_supergrafx(
            frame.rgba(),
            PCE_ACTIVE_FRAME_WIDTH,
            &rows,
            first_row,
            row_end,
            output,
        ),
    }
}

fn project_base(
    source: &[u8],
    source_width: usize,
    rows: &[ProjectionRow],
    first_row: usize,
    row_end: usize,
    output: &mut [u8],
) {
    let source_height = row_end - first_row;
    let mut mapped_visible_width = 0;
    let mut source_x_by_destination = [0usize; PCE_HOST_FRAME_WIDTH];
    for destination_y in 0..PCE_HOST_FRAME_HEIGHT {
        let source_y = first_row + destination_y * source_height / PCE_HOST_FRAME_HEIGHT;
        let Some(row) = rows.get(source_y).copied().filter(|row| row.active) else {
            continue;
        };
        let visible_width = row.active_width.min(source_width);
        if visible_width == 0 {
            continue;
        }
        if mapped_visible_width != visible_width {
            for (destination_x, source_x) in source_x_by_destination.iter_mut().enumerate() {
                *source_x = destination_x * visible_width / PCE_HOST_FRAME_WIDTH;
            }
            mapped_visible_width = visible_width;
        }
        let source_row_start = source_y * source_width * 4;
        let destination_row_start = destination_y * PCE_HOST_FRAME_WIDTH * 4;
        for (destination_x, &source_x) in source_x_by_destination.iter().enumerate() {
            copy_pixel(
                source,
                source_row_start + source_x * 4,
                output,
                destination_row_start + destination_x * 4,
            );
        }
    }
}

fn project_supergrafx(
    source: &[u8],
    source_width: usize,
    rows: &[ProjectionRow],
    first_row: usize,
    row_end: usize,
    output: &mut [u8],
) {
    let source_height = row_end - first_row;
    let Some((frame_start, frame_end)) = rows[first_row..row_end]
        .iter()
        .filter_map(|row| row.master_span(source_width))
        .fold(None::<(usize, usize)>, |bounds, (start, end)| {
            Some(match bounds {
                None => (start, end),
                Some((minimum, maximum)) => (minimum.min(start), maximum.max(end)),
            })
        })
    else {
        return;
    };
    let frame_width = frame_end - frame_start;
    if frame_width == 0 {
        return;
    }

    for destination_y in 0..PCE_HOST_FRAME_HEIGHT {
        let source_y = first_row + destination_y * source_height / PCE_HOST_FRAME_HEIGHT;
        let Some(row) = rows.get(source_y).copied().filter(|row| row.active) else {
            continue;
        };
        let active_width = row.active_width.min(source_width);
        let Some((row_start, row_end)) = row.master_span(source_width) else {
            continue;
        };
        let source_row_start = source_y * source_width * 4;
        let destination_row_start = destination_y * PCE_HOST_FRAME_WIDTH * 4;
        for destination_x in 0..PCE_HOST_FRAME_WIDTH {
            let master_position = frame_start + destination_x * frame_width / PCE_HOST_FRAME_WIDTH;
            if !(row_start..row_end).contains(&master_position) {
                continue;
            }
            let source_x =
                ((master_position - row_start) / row.pixel_clock_divisor).min(active_width - 1);
            copy_pixel(
                source,
                source_row_start + source_x * 4,
                output,
                destination_row_start + destination_x * 4,
            );
        }
    }
}

fn clear_output(output: &mut [u8]) {
    for pixel in output.as_chunks_mut::<4>().0 {
        pixel.copy_from_slice(&OPAQUE_BLACK);
    }
}

fn copy_pixel(source: &[u8], source_start: usize, output: &mut [u8], destination_start: usize) {
    output[destination_start..destination_start + 4]
        .copy_from_slice(&source[source_start..source_start + 4]);
}

impl ProjectionRow {
    fn master_span(self, source_width: usize) -> Option<(usize, usize)> {
        let active_width = self.active_width.min(source_width);
        if !self.active || active_width == 0 || self.pixel_clock_divisor == 0 {
            return None;
        }
        let start = self.active_x_origin.checked_mul(self.pixel_clock_divisor)?;
        let end = self
            .active_x_origin
            .checked_add(active_width)?
            .checked_mul(self.pixel_clock_divisor)?;
        Some((start, end))
    }
}
