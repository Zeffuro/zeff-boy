use super::{
    PCE_ACTIVE_FRAME_HEIGHT, PCE_ACTIVE_FRAME_WIDTH, PceHardwareTopology, PcePresentedFrame,
};

pub const PCE_HOST_FRAME_WIDTH: usize = zeff_emu_common::system::PCE_SCREEN_SIZE.0 as usize;
pub const PCE_HOST_FRAME_HEIGHT: usize = zeff_emu_common::system::PCE_SCREEN_SIZE.1 as usize;
const PCE_HOST_FRAME_PIXELS: usize = PCE_HOST_FRAME_WIDTH * PCE_HOST_FRAME_HEIGHT;
pub const PCE_HOST_FRAME_RGBA_BYTES: usize =
    PCE_HOST_FRAME_PIXELS * zeff_emu_common::system::RGBA_BYTES_PER_PIXEL;
pub const PCE_HOST_FRAME_XRGB8888_BYTES: usize = PCE_HOST_FRAME_PIXELS * 4;
pub const PCE_HOST_FRAME_RGB565_BYTES: usize = PCE_HOST_FRAME_PIXELS * 2;
const OPAQUE_BLACK: [u8; 4] = [0, 0, 0, 0xFF];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PceNativeFrameDescriptor {
    first_row: usize,
    height: usize,
    width: usize,
}

impl PceNativeFrameDescriptor {
    #[inline]
    pub const fn width(self) -> usize {
        self.width
    }

    #[inline]
    pub const fn height(self) -> usize {
        self.height
    }
}

#[derive(Clone, Copy, Default)]
struct ProjectionRow {
    active_x_origin: usize,
    active_width: usize,
    pixel_clock_divisor: usize,
    active: bool,
}

pub fn project_full_raw_frame(
    frame: PcePresentedFrame<'_>,
    topology: PceHardwareTopology,
    output: &mut [u8],
) {
    project_full_frame::<Rgba8888>(frame, topology, output);
}

pub fn project_full_xrgb8888_frame(
    frame: PcePresentedFrame<'_>,
    topology: PceHardwareTopology,
    output: &mut [u8],
) {
    project_full_frame::<Xrgb8888>(frame, topology, output);
}

pub fn project_full_rgb565_frame(
    frame: PcePresentedFrame<'_>,
    topology: PceHardwareTopology,
    output: &mut [u8],
) {
    project_full_frame::<Rgb565>(frame, topology, output);
}

pub fn native_frame_descriptor(
    frame: PcePresentedFrame<'_>,
    topology: PceHardwareTopology,
) -> Option<PceNativeFrameDescriptor> {
    if topology != PceHardwareTopology::Base {
        return None;
    }
    let signal = frame.signal_bounds();
    let first_row = usize::from(signal.first_row());
    let row_end = usize::from(signal.row_end());
    let rows = frame.rows().get(first_row..row_end)?;
    let mut layout = None;
    for metadata in rows.iter().copied().filter(|metadata| metadata.is_active()) {
        let width = usize::from(metadata.active_width());
        let candidate = (
            usize::from(metadata.active_x_origin()),
            width,
            metadata.pixel_clock()?,
        );
        if width == 0 || width > PCE_HOST_FRAME_WIDTH {
            return None;
        }
        match layout {
            None => layout = Some(candidate),
            Some(layout) if layout == candidate => {}
            Some(_) => return None,
        }
    }
    let (_, width, _) = layout?;
    Some(PceNativeFrameDescriptor {
        first_row,
        height: row_end - first_row,
        width,
    })
}

pub fn project_native_xrgb8888_frame(
    frame: PcePresentedFrame<'_>,
    descriptor: PceNativeFrameDescriptor,
    output: &mut [u8],
) {
    project_native_frame::<Xrgb8888>(frame, descriptor, output);
}

pub fn project_native_raw_frame(
    frame: PcePresentedFrame<'_>,
    descriptor: PceNativeFrameDescriptor,
    output: &mut [u8],
) {
    project_native_frame::<Rgba8888>(frame, descriptor, output);
}

pub fn project_native_rgb565_frame(
    frame: PcePresentedFrame<'_>,
    descriptor: PceNativeFrameDescriptor,
    output: &mut [u8],
) {
    project_native_frame::<Rgb565>(frame, descriptor, output);
}

fn project_full_frame<F: HostPixelFormat>(
    frame: PcePresentedFrame<'_>,
    topology: PceHardwareTopology,
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
    let signal = frame.signal_bounds();
    let first_row = usize::from(signal.first_row());
    let row_end = usize::from(signal.row_end());
    project_rows_with_reuse::<F>(frame.rgba(), topology, &rows, first_row, row_end, output);
}

fn project_native_frame<F: HostPixelFormat>(
    frame: PcePresentedFrame<'_>,
    descriptor: PceNativeFrameDescriptor,
    output: &mut [u8],
) {
    assert_eq!(
        output.len(),
        descriptor.width * descriptor.height * F::BYTES
    );
    F::clear(output);
    for destination_y in 0..descriptor.height {
        let source_y = descriptor.first_row + destination_y;
        let Some(row) = frame
            .rows()
            .get(source_y)
            .copied()
            .filter(|row| row.is_active())
        else {
            continue;
        };
        debug_assert_eq!(usize::from(row.active_width()), descriptor.width);
        let source_row_start = source_y * PCE_ACTIVE_FRAME_WIDTH * 4;
        let source_pixels = frame.rgba()[source_row_start..source_row_start + descriptor.width * 4]
            .as_chunks::<4>()
            .0;
        let destination_row_start = destination_y * descriptor.width * F::BYTES;
        let destination_pixels = output
            [destination_row_start..destination_row_start + descriptor.width * F::BYTES]
            .chunks_mut(F::BYTES);
        for (source, destination) in source_pixels.iter().copied().zip(destination_pixels) {
            F::write(source, destination);
        }
    }
}

fn project_rows_with_reuse<F: HostPixelFormat>(
    source: &[u8],
    topology: PceHardwareTopology,
    rows: &[ProjectionRow],
    first_row: usize,
    row_end: usize,
    output: &mut [u8],
) {
    assert_eq!(output.len(), PCE_HOST_FRAME_PIXELS * F::BYTES);
    F::clear(output);
    if first_row >= row_end || row_end > rows.len() {
        return;
    }

    match topology {
        PceHardwareTopology::Base => project_base::<F>(
            source,
            PCE_ACTIVE_FRAME_WIDTH,
            rows,
            first_row,
            row_end,
            output,
        ),
        PceHardwareTopology::SuperGrafx => project_supergrafx::<F>(
            source,
            PCE_ACTIVE_FRAME_WIDTH,
            rows,
            first_row,
            row_end,
            output,
        ),
    }
}

fn project_base<F: HostPixelFormat>(
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
    let destination_row_bytes = PCE_HOST_FRAME_WIDTH * F::BYTES;
    let mut previous_source_y = None;
    for destination_y in 0..PCE_HOST_FRAME_HEIGHT {
        let source_y = first_row + destination_y * source_height / PCE_HOST_FRAME_HEIGHT;
        let destination_row_start = destination_y * destination_row_bytes;
        if previous_source_y == Some(source_y) {
            output.copy_within(
                destination_row_start - destination_row_bytes..destination_row_start,
                destination_row_start,
            );
            continue;
        }
        previous_source_y = Some(source_y);
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
        let source_pixels = source[source_row_start..source_row_start + source_width * 4]
            .as_chunks::<4>()
            .0;
        let destination_pixels = output
            [destination_row_start..destination_row_start + destination_row_bytes]
            .chunks_mut(F::BYTES);
        for (destination, &source_x) in destination_pixels.zip(source_x_by_destination.iter()) {
            F::write(source_pixels[source_x], destination);
        }
    }
}

fn project_supergrafx<F: HostPixelFormat>(
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

    let destination_row_bytes = PCE_HOST_FRAME_WIDTH * F::BYTES;
    let mut previous_source_y = None;
    for destination_y in 0..PCE_HOST_FRAME_HEIGHT {
        let source_y = first_row + destination_y * source_height / PCE_HOST_FRAME_HEIGHT;
        let destination_row_start = destination_y * destination_row_bytes;
        if previous_source_y == Some(source_y) {
            output.copy_within(
                destination_row_start - destination_row_bytes..destination_row_start,
                destination_row_start,
            );
            continue;
        }
        previous_source_y = Some(source_y);
        let Some(row) = rows.get(source_y).copied().filter(|row| row.active) else {
            continue;
        };
        let active_width = row.active_width.min(source_width);
        let Some((row_start, row_end)) = row.master_span(source_width) else {
            continue;
        };
        let source_row_start = source_y * source_width * 4;
        let source_pixels = source[source_row_start..source_row_start + source_width * 4]
            .as_chunks::<4>()
            .0;
        let destination_pixels = output
            [destination_row_start..destination_row_start + destination_row_bytes]
            .chunks_mut(F::BYTES);
        for (destination_x, destination) in destination_pixels.enumerate() {
            let master_position = frame_start + destination_x * frame_width / PCE_HOST_FRAME_WIDTH;
            if !(row_start..row_end).contains(&master_position) {
                continue;
            }
            let source_x =
                ((master_position - row_start) / row.pixel_clock_divisor).min(active_width - 1);
            F::write(source_pixels[source_x], destination);
        }
    }
}

trait HostPixelFormat {
    const BYTES: usize;

    fn clear(output: &mut [u8]);
    fn write(source: [u8; 4], destination: &mut [u8]);
}

struct Rgba8888;

impl HostPixelFormat for Rgba8888 {
    const BYTES: usize = 4;

    fn clear(output: &mut [u8]) {
        for pixel in output.as_chunks_mut::<4>().0 {
            *pixel = OPAQUE_BLACK;
        }
    }

    #[inline]
    fn write(source: [u8; 4], destination: &mut [u8]) {
        destination.copy_from_slice(&source);
    }
}

struct Xrgb8888;

impl HostPixelFormat for Xrgb8888 {
    const BYTES: usize = 4;

    fn clear(output: &mut [u8]) {
        output.fill(0);
    }

    #[inline]
    fn write(source: [u8; 4], destination: &mut [u8]) {
        destination.copy_from_slice(&[source[2], source[1], source[0], 0]);
    }
}

struct Rgb565;

impl HostPixelFormat for Rgb565 {
    const BYTES: usize = 2;

    fn clear(output: &mut [u8]) {
        output.fill(0);
    }

    #[inline]
    fn write(source: [u8; 4], destination: &mut [u8]) {
        let red = u16::from(source[0]);
        let green = u16::from(source[1]);
        let blue = u16::from(source[2]);
        let pixel = ((red >> 3) << 11) | ((green >> 2) << 5) | (blue >> 3);
        destination.copy_from_slice(&pixel.to_le_bytes());
    }
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
    use crate::hardware::{PceVideoRowMetadata, VcePixelClock};

    #[test]
    fn native_descriptor_packs_uniform_base_rows_without_resampling() {
        let mut source = vec![0; PCE_ACTIVE_FRAME_WIDTH * PCE_ACTIVE_FRAME_HEIGHT * 4];
        let mut rows = [PceVideoRowMetadata::default(); PCE_ACTIVE_FRAME_HEIGHT];
        for (line, row) in rows.iter_mut().enumerate().take(259).skip(17) {
            *row = PceVideoRowMetadata::test_active(32, 256, VcePixelClock::DivideByFour);
            for x in 0..256 {
                let offset = (line * PCE_ACTIVE_FRAME_WIDTH + x) * 4;
                source[offset..offset + 4].copy_from_slice(&[x as u8, line as u8, 0xA5, 0xFF]);
            }
        }
        rows[18] = PceVideoRowMetadata::default();
        let frame = PcePresentedFrame::test_frame(&source, &rows);
        let descriptor = native_frame_descriptor(frame, PceHardwareTopology::Base).unwrap();
        assert_eq!((descriptor.width(), descriptor.height()), (256, 242));

        let mut xrgb = vec![0xCC; descriptor.width() * descriptor.height() * 4];
        let mut rgba = vec![0xCC; descriptor.width() * descriptor.height() * 4];
        let mut rgb565 = vec![0xCC; descriptor.width() * descriptor.height() * 2];
        project_native_xrgb8888_frame(frame, descriptor, &mut xrgb);
        project_native_raw_frame(frame, descriptor, &mut rgba);
        project_native_rgb565_frame(frame, descriptor, &mut rgb565);

        assert_eq!(&xrgb[..4], &[0xA5, 17, 0, 0]);
        for (native, converted) in rgba.as_chunks::<4>().0.iter().zip(xrgb.as_chunks::<4>().0) {
            assert_eq!(native, &[converted[2], converted[1], converted[0], 0xFF]);
        }
        assert_eq!(&xrgb[255 * 4..256 * 4], &[0xA5, 17, 255, 0]);
        assert!(xrgb[256 * 4..512 * 4].iter().all(|&byte| byte == 0));
        let pixel = ((u16::from(17_u8) >> 2) << 5) | (0xA5 >> 3);
        assert_eq!(&rgb565[..2], &pixel.to_le_bytes());
    }

    #[test]
    fn native_descriptor_rejects_mixed_layouts_and_supergrafx() {
        let source = vec![0; PCE_ACTIVE_FRAME_WIDTH * PCE_ACTIVE_FRAME_HEIGHT * 4];
        let mut rows = [PceVideoRowMetadata::default(); PCE_ACTIVE_FRAME_HEIGHT];
        for row in rows.iter_mut().take(259).skip(17) {
            *row = PceVideoRowMetadata::test_active(32, 256, VcePixelClock::DivideByFour);
        }
        let frame = PcePresentedFrame::test_frame(&source, &rows);
        assert!(native_frame_descriptor(frame, PceHardwareTopology::SuperGrafx).is_none());
        rows[18] = PceVideoRowMetadata::test_active(32, 512, VcePixelClock::DivideByTwo);
        let frame = PcePresentedFrame::test_frame(&source, &rows);
        assert!(native_frame_descriptor(frame, PceHardwareTopology::Base).is_none());
        rows.fill(PceVideoRowMetadata::test_active(
            0,
            PCE_ACTIVE_FRAME_WIDTH as u16,
            VcePixelClock::DivideByFour,
        ));
        let frame = PcePresentedFrame::test_frame(&source, &rows);
        assert!(native_frame_descriptor(frame, PceHardwareTopology::Base).is_none());
    }

    fn project_rows_scalar<F: HostPixelFormat>(
        source: &[u8],
        topology: PceHardwareTopology,
        rows: &[ProjectionRow],
        first_row: usize,
        row_end: usize,
        output: &mut [u8],
    ) {
        F::clear(output);
        let source_height = row_end - first_row;
        let source_width = PCE_ACTIVE_FRAME_WIDTH;
        let frame_bounds = rows[first_row..row_end]
            .iter()
            .filter_map(|row| row.master_span(source_width))
            .fold(None::<(usize, usize)>, |bounds, (start, end)| {
                Some(match bounds {
                    None => (start, end),
                    Some((minimum, maximum)) => (minimum.min(start), maximum.max(end)),
                })
            });

        for destination_y in 0..PCE_HOST_FRAME_HEIGHT {
            let source_y = first_row + destination_y * source_height / PCE_HOST_FRAME_HEIGHT;
            let Some(row) = rows.get(source_y).copied().filter(|row| row.active) else {
                continue;
            };
            let active_width = row.active_width.min(source_width);
            if active_width == 0 {
                continue;
            }
            let source_row_start = source_y * source_width * 4;
            let destination_row_start = destination_y * PCE_HOST_FRAME_WIDTH * F::BYTES;
            for destination_x in 0..PCE_HOST_FRAME_WIDTH {
                let source_x = match topology {
                    PceHardwareTopology::Base => {
                        destination_x * active_width / PCE_HOST_FRAME_WIDTH
                    }
                    PceHardwareTopology::SuperGrafx => {
                        let Some((frame_start, frame_end)) = frame_bounds else {
                            continue;
                        };
                        let Some((row_start, row_end)) = row.master_span(source_width) else {
                            continue;
                        };
                        let master_position = frame_start
                            + destination_x * (frame_end - frame_start) / PCE_HOST_FRAME_WIDTH;
                        if !(row_start..row_end).contains(&master_position) {
                            continue;
                        }
                        ((master_position - row_start) / row.pixel_clock_divisor)
                            .min(active_width - 1)
                    }
                };
                let source_start = source_row_start + source_x * 4;
                let destination_start = destination_row_start + destination_x * F::BYTES;
                F::write(
                    source[source_start..source_start + 4].try_into().unwrap(),
                    &mut output[destination_start..destination_start + F::BYTES],
                );
            }
        }
    }

    #[test]
    fn packed_projections_match_rgba_conversion_for_both_topologies() {
        let mut source = vec![0; PCE_ACTIVE_FRAME_WIDTH * PCE_ACTIVE_FRAME_HEIGHT * 4];
        for (index, pixel) in source.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            *pixel = [index as u8, (index >> 3) as u8, (index >> 7) as u8, 0xFF];
        }
        let mut rows = [ProjectionRow::default(); PCE_ACTIVE_FRAME_HEIGHT];
        for (line, row) in rows[17..259].iter_mut().enumerate() {
            if line % 19 == 0 {
                continue;
            }
            let (active_width, pixel_clock_divisor) = match line % 3 {
                0 => (256, 4),
                1 => (320, 3),
                _ => (512, 2),
            };
            *row = ProjectionRow {
                active_x_origin: line % 11,
                active_width,
                pixel_clock_divisor,
                active: true,
            };
        }

        for topology in [PceHardwareTopology::Base, PceHardwareTopology::SuperGrafx] {
            let mut expected_rgba = vec![0; PCE_HOST_FRAME_RGBA_BYTES];
            let mut expected_xrgb = vec![0; PCE_HOST_FRAME_XRGB8888_BYTES];
            let mut expected_rgb565 = vec![0; PCE_HOST_FRAME_RGB565_BYTES];
            project_rows_scalar::<Rgba8888>(&source, topology, &rows, 17, 259, &mut expected_rgba);
            project_rows_scalar::<Xrgb8888>(&source, topology, &rows, 17, 259, &mut expected_xrgb);
            project_rows_scalar::<Rgb565>(&source, topology, &rows, 17, 259, &mut expected_rgb565);

            let mut rgba = vec![0xA5; PCE_HOST_FRAME_RGBA_BYTES];
            let mut xrgb = vec![0xA5; PCE_HOST_FRAME_XRGB8888_BYTES];
            let mut rgb565 = vec![0xA5; PCE_HOST_FRAME_RGB565_BYTES];
            project_rows_with_reuse::<Rgba8888>(&source, topology, &rows, 17, 259, &mut rgba);
            project_rows_with_reuse::<Xrgb8888>(&source, topology, &rows, 17, 259, &mut xrgb);
            project_rows_with_reuse::<Rgb565>(&source, topology, &rows, 17, 259, &mut rgb565);

            assert_eq!(rgba, expected_rgba);
            assert_eq!(xrgb, expected_xrgb);
            assert_eq!(rgb565, expected_rgb565);
        }
    }
}
