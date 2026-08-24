use zeff_pce_core::hardware::{
    PCE_ACTIVE_FRAME_HEIGHT, PCE_ACTIVE_FRAME_WIDTH, PceHardwareTopology, PcePresentedFrame,
};

use crate::settings::{PceOverscanMode, PcePaletteMode};

use super::pce::{PCE_PRESENTED_HEIGHT, PCE_PRESENTED_RGBA_BYTES, PCE_PRESENTED_WIDTH};
use super::pce_palette::PCE_COMPOSITE_PALETTE;

pub(super) const OPAQUE_BLACK: [u8; 4] = [0, 0, 0, 0xFF];
const TV_SAFE_HEIGHT: usize = 224;
const TV_SAFE_MASTER_DOTS: usize = 960;

#[derive(Clone, Copy, Default)]
pub(super) struct ProjectionRow {
    pub(super) active_x_origin: usize,
    pub(super) active_width: usize,
    pub(super) pixel_clock_divisor: usize,
    pub(super) active: bool,
}

#[derive(Clone, Copy)]
struct ProjectionOptions {
    overscan: PceOverscanMode,
    palette: PcePaletteMode,
}

pub(super) fn project_presented_frame(
    frame: PcePresentedFrame<'_>,
    topology: PceHardwareTopology,
    overscan_mode: PceOverscanMode,
    palette_mode: PcePaletteMode,
    output: &mut [u8],
) {
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
    let bounds = visible_vertical_bounds(
        frame.active_bounds().map(|bounds| {
            (
                usize::from(bounds.first_row()),
                usize::from(bounds.row_end()),
            )
        }),
        overscan_mode,
    );
    match topology {
        PceHardwareTopology::Base => project_base_rgba_rows_with_options(
            frame.rgba(),
            PCE_ACTIVE_FRAME_WIDTH,
            &rows,
            bounds,
            ProjectionOptions {
                overscan: overscan_mode,
                palette: palette_mode,
            },
            output,
        ),
        PceHardwareTopology::SuperGrafx => project_sgx_rgba_rows_with_options(
            frame.rgba(),
            PCE_ACTIVE_FRAME_WIDTH,
            &rows,
            bounds,
            ProjectionOptions {
                overscan: overscan_mode,
                palette: palette_mode,
            },
            output,
        ),
    }
}

#[cfg(test)]
pub(super) fn project_base_rgba_rows(
    source: &[u8],
    source_width: usize,
    rows: &[ProjectionRow],
    active_bounds: Option<(usize, usize)>,
    output: &mut [u8],
) {
    project_base_rgba_rows_with_options(
        source,
        source_width,
        rows,
        active_bounds,
        ProjectionOptions {
            overscan: PceOverscanMode::Full,
            palette: PcePaletteMode::RawRgb,
        },
        output,
    );
}

fn project_base_rgba_rows_with_options(
    source: &[u8],
    source_width: usize,
    rows: &[ProjectionRow],
    active_bounds: Option<(usize, usize)>,
    options: ProjectionOptions,
    output: &mut [u8],
) {
    clear_output(output);
    let Some((first_row, row_end)) = active_bounds else {
        return;
    };
    let source_height = row_end.saturating_sub(first_row);
    if source_height == 0 {
        return;
    }

    for destination_y in 0..PCE_PRESENTED_HEIGHT {
        let source_y = first_row + destination_y * source_height / PCE_PRESENTED_HEIGHT;
        let Some(row) = rows.get(source_y).copied().filter(|row| row.active) else {
            continue;
        };
        let Some((source_x_offset, visible_width)) =
            base_visible_span(row, source_width, options.overscan)
        else {
            continue;
        };
        let source_row_start = source_y * source_width * 4;
        let destination_row_start = destination_y * PCE_PRESENTED_WIDTH * 4;
        for destination_x in 0..PCE_PRESENTED_WIDTH {
            let source_x = source_x_offset + destination_x * visible_width / PCE_PRESENTED_WIDTH;
            copy_pixel(
                source,
                source_row_start + source_x * 4,
                output,
                destination_row_start + destination_x * 4,
                options.palette,
            );
        }
    }
}

#[cfg(test)]
pub(super) fn project_sgx_rgba_rows(
    source: &[u8],
    source_width: usize,
    rows: &[ProjectionRow],
    active_bounds: Option<(usize, usize)>,
    output: &mut [u8],
) {
    project_sgx_rgba_rows_with_options(
        source,
        source_width,
        rows,
        active_bounds,
        ProjectionOptions {
            overscan: PceOverscanMode::Full,
            palette: PcePaletteMode::RawRgb,
        },
        output,
    );
}

fn project_sgx_rgba_rows_with_options(
    source: &[u8],
    source_width: usize,
    rows: &[ProjectionRow],
    active_bounds: Option<(usize, usize)>,
    options: ProjectionOptions,
    output: &mut [u8],
) {
    clear_output(output);
    let Some((first_row, row_end)) = active_bounds else {
        return;
    };
    let source_height = row_end.saturating_sub(first_row);
    if source_height == 0 {
        return;
    }
    let Some((mut frame_start, mut frame_end)) = rows[first_row..row_end]
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
    if options.overscan != PceOverscanMode::Full && frame_end - frame_start > TV_SAFE_MASTER_DOTS {
        let excess = frame_end - frame_start - TV_SAFE_MASTER_DOTS;
        frame_start += excess / 2;
        frame_end = frame_start + TV_SAFE_MASTER_DOTS;
    }
    if options.overscan == PceOverscanMode::Conservative {
        let inset = rows[first_row..row_end]
            .iter()
            .find(|row| row.active && row.pixel_clock_divisor != 0)
            .map_or(0, |row| 8 * row.pixel_clock_divisor);
        if frame_end.saturating_sub(frame_start) > 2 * inset {
            frame_start += inset;
            frame_end -= inset;
        }
    }
    let frame_width = frame_end - frame_start;

    for destination_y in 0..PCE_PRESENTED_HEIGHT {
        let source_y = first_row + destination_y * source_height / PCE_PRESENTED_HEIGHT;
        let Some(row) = rows.get(source_y).copied().filter(|row| row.active) else {
            continue;
        };
        let active_width = row.active_width.min(source_width);
        let Some((row_start, row_end)) = row.master_span(source_width) else {
            continue;
        };
        let source_row_start = source_y * source_width * 4;
        let destination_row_start = destination_y * PCE_PRESENTED_WIDTH * 4;
        for destination_x in 0..PCE_PRESENTED_WIDTH {
            let master_position = frame_start + destination_x * frame_width / PCE_PRESENTED_WIDTH;
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
                options.palette,
            );
        }
    }
}

fn visible_vertical_bounds(
    bounds: Option<(usize, usize)>,
    overscan_mode: PceOverscanMode,
) -> Option<(usize, usize)> {
    let (mut first, mut end) = bounds?;
    if overscan_mode != PceOverscanMode::Full && end.saturating_sub(first) > TV_SAFE_HEIGHT {
        let excess = end - first - TV_SAFE_HEIGHT;
        first += excess / 2;
        end = first + TV_SAFE_HEIGHT;
    }
    Some((first, end))
}

fn base_visible_span(
    row: ProjectionRow,
    source_width: usize,
    overscan_mode: PceOverscanMode,
) -> Option<(usize, usize)> {
    let active_width = row.active_width.min(source_width);
    if active_width == 0 {
        return None;
    }
    if overscan_mode == PceOverscanMode::Full {
        return Some((0, active_width));
    }
    let (mut safe_start, mut safe_width) = match row.pixel_clock_divisor {
        4 => (32, 240),
        3 => (48, 320),
        2 => (104, 480),
        _ => return Some((0, active_width)),
    };
    if overscan_mode == PceOverscanMode::Conservative {
        safe_start += 8;
        safe_width -= 16;
    }
    let active_start = row.active_x_origin;
    let active_end = active_start.checked_add(active_width)?;
    let visible_start = active_start.max(safe_start);
    let visible_end = active_end.min(safe_start + safe_width);
    (visible_start < visible_end)
        .then(|| (visible_start - active_start, visible_end - visible_start))
}

fn clear_output(output: &mut [u8]) {
    debug_assert_eq!(output.len(), PCE_PRESENTED_RGBA_BYTES);
    for pixel in output.as_chunks_mut::<4>().0 {
        pixel.copy_from_slice(&OPAQUE_BLACK);
    }
}

fn copy_pixel(
    source: &[u8],
    source_start: usize,
    output: &mut [u8],
    destination_start: usize,
    palette_mode: PcePaletteMode,
) {
    match palette_mode {
        PcePaletteMode::RawRgb => {
            output[destination_start..destination_start + 4]
                .copy_from_slice(&source[source_start..source_start + 4]);
        }
        PcePaletteMode::Composite => {
            let red = quantize_raw_component(source[source_start]);
            let green = quantize_raw_component(source[source_start + 1]);
            let blue = quantize_raw_component(source[source_start + 2]);
            let index =
                usize::from((u16::from(green) << 6) | (u16::from(red) << 3) | u16::from(blue)) * 3;
            output[destination_start..destination_start + 3]
                .copy_from_slice(&PCE_COMPOSITE_PALETTE[index..index + 3]);
            output[destination_start + 3] = 0xFF;
        }
    }
}

fn quantize_raw_component(component: u8) -> u8 {
    ((u16::from(component) * 7 + 127) / 255) as u8
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tv_safe_bounds_center_the_measured_224_line_window() {
        assert_eq!(
            visible_vertical_bounds(Some((12, 252)), PceOverscanMode::TvSafe),
            Some((20, 244))
        );
        assert_eq!(
            visible_vertical_bounds(Some((12, 220)), PceOverscanMode::TvSafe),
            Some((12, 220))
        );
    }

    #[test]
    fn tv_safe_width_tracks_the_three_vce_dot_clocks() {
        for (divisor, origin, active, expected) in [
            (4, 8, 288, (24, 240)),
            (3, 24, 376, (24, 320)),
            (2, 64, 536, (40, 480)),
        ] {
            assert_eq!(
                base_visible_span(
                    ProjectionRow {
                        active_width: active,
                        active_x_origin: origin,
                        pixel_clock_divisor: divisor,
                        active: true,
                    },
                    active,
                    PceOverscanMode::TvSafe,
                ),
                Some(expected)
            );
        }
    }

    #[test]
    fn conservative_crop_hides_one_character_inside_the_tv_safe_window() {
        let row = ProjectionRow {
            active_x_origin: 40,
            active_width: 352,
            pixel_clock_divisor: 3,
            active: true,
        };

        assert_eq!(
            base_visible_span(row, PCE_ACTIVE_FRAME_WIDTH, PceOverscanMode::Conservative),
            Some((16, 304))
        );

        assert_eq!(
            base_visible_span(
                ProjectionRow {
                    active_x_origin: 0,
                    active_width: 8,
                    pixel_clock_divisor: 4,
                    active: true,
                },
                PCE_ACTIVE_FRAME_WIDTH,
                PceOverscanMode::Conservative,
            ),
            None
        );
    }

    #[test]
    fn composite_palette_uses_the_preserved_huc6260_lookup() {
        let mut output = [0; 4];
        let raw = [0, 0, 255, 255];
        copy_pixel(&raw, 0, &mut output, 0, PcePaletteMode::Composite);
        assert_eq!(output, [9, 3, 181, 255]);

        let white = [255, 255, 255, 255];
        copy_pixel(&white, 0, &mut output, 0, PcePaletteMode::Composite);
        assert_eq!(output, [255, 255, 255, 255]);
    }

    #[test]
    fn tv_safe_projection_centers_the_low_resolution_active_span() {
        let mut source = vec![0; 288 * 4];
        for x in 0..288 {
            source[x * 4..x * 4 + 4].copy_from_slice(&[x.min(255) as u8, 0, 0, 0xFF]);
        }
        let rows = [ProjectionRow {
            active_x_origin: 8,
            active_width: 288,
            pixel_clock_divisor: 4,
            active: true,
        }];
        let mut output = vec![0; PCE_PRESENTED_RGBA_BYTES];
        project_base_rgba_rows_with_options(
            &source,
            288,
            &rows,
            Some((0, 1)),
            ProjectionOptions {
                overscan: PceOverscanMode::TvSafe,
                palette: PcePaletteMode::RawRgb,
            },
            &mut output,
        );
        assert_eq!(&output[..4], &[24, 0, 0, 0xFF]);
        let last = (PCE_PRESENTED_WIDTH - 1) * 4;
        assert_eq!(&output[last..last + 4], &[255, 0, 0, 0xFF]);
    }
}
