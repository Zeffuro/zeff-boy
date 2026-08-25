use super::vce::{HuC6260, VcePixelClock};
use super::vdc::HuC6270;
use super::vdc_composite::{
    CompositedPixel, DisplayCompositionError, DisplayLayerLine, compose_vdc_output_scanline,
};
use super::vdc_render::{BackgroundRenderError, BackgroundScanlineStatus};
use super::vdc_scanline::VdcActiveDisplayLine;
use super::vdc_sprite_render::{SpritePixel, SpriteRenderError, SpriteScanlineStatus};
use super::vpc::{HuC6202, VpcVdc, VpcVdcPixel};
use anyhow::bail;
use zeff_emu_common::save_state::{StateReader, StateWriter};

pub const PCE_ACTIVE_FRAME_WIDTH: usize = 1024;
pub const PCE_ACTIVE_FRAME_HEIGHT: usize = 512;
pub const PCE_ACTIVE_FRAME_RGBA_BYTES: usize = PCE_ACTIVE_FRAME_WIDTH * PCE_ACTIVE_FRAME_HEIGHT * 4;
pub const PCE_ACTIVE_FRAME_UNUSED_RGBA: [u8; 4] = [0, 0, 0, 0xFF];
pub const PCE_SIGNAL_FIRST_ROW: u16 = 17;
pub const PCE_SIGNAL_ROW_END: u16 = 259;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PceVideoRowMetadata {
    active_x_origin: u16,
    active_width: u16,
    pixel_clock: Option<VcePixelClock>,
    background: Option<PceBackgroundLineDebug>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PceBackgroundLineDebug {
    scroll_x: u16,
    virtual_y: u16,
    first_bat_word: u16,
}

impl PceBackgroundLineDebug {
    #[inline]
    pub const fn scroll_x(self) -> u16 {
        self.scroll_x
    }

    #[inline]
    pub const fn virtual_y(self) -> u16 {
        self.virtual_y
    }

    #[inline]
    pub const fn first_bat_word(self) -> u16 {
        self.first_bat_word
    }
}

impl PceVideoRowMetadata {
    #[inline]
    pub const fn is_active(self) -> bool {
        self.pixel_clock.is_some()
    }

    #[inline]
    pub const fn active_width(self) -> u16 {
        self.active_width
    }

    #[inline]
    pub const fn active_x_origin(self) -> u16 {
        self.active_x_origin
    }

    #[inline]
    pub const fn pixel_clock(self) -> Option<VcePixelClock> {
        self.pixel_clock
    }

    #[inline]
    pub const fn background(self) -> Option<PceBackgroundLineDebug> {
        self.background
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PceVideoActiveBounds {
    first_row: u16,
    row_end: u16,
    maximum_width: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PceVideoSignalBounds {
    first_row: u16,
    row_end: u16,
}

impl PceVideoSignalBounds {
    #[inline]
    pub const fn first_row(self) -> u16 {
        self.first_row
    }

    #[inline]
    pub const fn row_end(self) -> u16 {
        self.row_end
    }

    #[inline]
    pub const fn height(self) -> u16 {
        self.row_end - self.first_row
    }
}

impl PceVideoActiveBounds {
    #[inline]
    pub const fn first_row(self) -> u16 {
        self.first_row
    }

    #[inline]
    pub const fn row_end(self) -> u16 {
        self.row_end
    }

    #[inline]
    pub const fn height(self) -> u16 {
        self.row_end - self.first_row
    }

    #[inline]
    pub const fn maximum_width(self) -> u16 {
        self.maximum_width
    }
}

#[derive(Clone, Copy)]
pub struct PcePresentedFrame<'a> {
    rgba: &'a [u8],
    rows: &'a [PceVideoRowMetadata; PCE_ACTIVE_FRAME_HEIGHT],
    active_bounds: Option<PceVideoActiveBounds>,
    signal_bounds: PceVideoSignalBounds,
}

impl<'a> PcePresentedFrame<'a> {
    #[inline]
    pub const fn rgba(self) -> &'a [u8] {
        self.rgba
    }

    #[inline]
    pub const fn storage_dimensions(self) -> (usize, usize) {
        (PCE_ACTIVE_FRAME_WIDTH, PCE_ACTIVE_FRAME_HEIGHT)
    }

    #[inline]
    pub const fn rows(self) -> &'a [PceVideoRowMetadata; PCE_ACTIVE_FRAME_HEIGHT] {
        self.rows
    }

    #[inline]
    pub const fn active_bounds(self) -> Option<PceVideoActiveBounds> {
        self.active_bounds
    }

    #[inline]
    pub const fn signal_bounds(self) -> PceVideoSignalBounds {
        self.signal_bounds
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PceVideoRenderError {
    ActiveLineOutOfBounds { line: u16 },
    ActiveWidthOutOfBounds { width: u16 },
    ActiveSpanOutOfBounds { vdc: VpcVdc, start: u16, width: u16 },
    Background(BackgroundRenderError),
    Sprite(SpriteRenderError),
    Composition(DisplayCompositionError),
}

#[derive(Debug)]
pub struct PceActiveOnlyVideoFrame {
    framebuffer: Box<[u8]>,
    rows: Box<[PceVideoRowMetadata; PCE_ACTIVE_FRAME_HEIGHT]>,
    background: Box<[u8]>,
    sprites: Box<[Option<SpritePixel>]>,
    composited: Box<[CompositedPixel]>,
    vdc_one: Box<[VpcVdcPixel]>,
    vdc_two: Box<[VpcVdcPixel]>,
}

impl Default for PceActiveOnlyVideoFrame {
    fn default() -> Self {
        Self::new()
    }
}

impl PceActiveOnlyVideoFrame {
    pub fn new() -> Self {
        let mut frame = Self {
            framebuffer: vec![0; PCE_ACTIVE_FRAME_RGBA_BYTES].into_boxed_slice(),
            rows: Box::new([PceVideoRowMetadata::default(); PCE_ACTIVE_FRAME_HEIGHT]),
            background: vec![0; PCE_ACTIVE_FRAME_WIDTH].into_boxed_slice(),
            sprites: vec![None; PCE_ACTIVE_FRAME_WIDTH].into_boxed_slice(),
            composited: vec![CompositedPixel::default(); PCE_ACTIVE_FRAME_WIDTH].into_boxed_slice(),
            vdc_one: vec![VpcVdcPixel::new(0); PCE_ACTIVE_FRAME_WIDTH].into_boxed_slice(),
            vdc_two: vec![VpcVdcPixel::new(0); PCE_ACTIVE_FRAME_WIDTH].into_boxed_slice(),
        };
        frame.begin_frame();
        frame
    }

    #[inline]
    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    #[inline]
    pub const fn dimensions(&self) -> (usize, usize) {
        (PCE_ACTIVE_FRAME_WIDTH, PCE_ACTIVE_FRAME_HEIGHT)
    }

    #[inline]
    pub fn row_metadata(&self, line: usize) -> Option<PceVideoRowMetadata> {
        self.rows.get(line).copied()
    }

    pub(crate) fn presented_frame(&self) -> PcePresentedFrame<'_> {
        let mut first_row = None;
        let mut row_end = 0;
        let mut maximum_width = 0;
        for (line, metadata) in self.rows.iter().copied().enumerate() {
            if metadata.is_active() {
                first_row.get_or_insert(line as u16);
                row_end = line as u16 + 1;
                maximum_width = maximum_width.max(metadata.active_width());
            }
        }
        PcePresentedFrame {
            rgba: &self.framebuffer,
            rows: &self.rows,
            active_bounds: first_row.map(|first_row| PceVideoActiveBounds {
                first_row,
                row_end,
                maximum_width,
            }),
            signal_bounds: PceVideoSignalBounds {
                first_row: PCE_SIGNAL_FIRST_ROW,
                row_end: PCE_SIGNAL_ROW_END,
            },
        }
    }

    pub fn begin_frame(&mut self) {
        for pixel in self.framebuffer.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&PCE_ACTIVE_FRAME_UNUSED_RGBA);
        }
        self.rows.fill(PceVideoRowMetadata::default());
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.framebuffer);
        for row in self.rows.iter().copied() {
            writer.write_u16(row.active_x_origin);
            writer.write_u16(row.active_width);
            writer.write_u8(match row.pixel_clock {
                None => 0,
                Some(VcePixelClock::DivideByFour) => 1,
                Some(VcePixelClock::DivideByThree) => 2,
                Some(VcePixelClock::DivideByTwo) => 3,
            });
        }
    }

    pub(super) fn read_state(&mut self, reader: &mut StateReader<'_>) -> anyhow::Result<()> {
        let mut restored = Self::new();
        reader.read_exact(&mut restored.framebuffer)?;
        for row in restored.rows.iter_mut() {
            let active_x_origin = reader.read_u16()?;
            let active_width = reader.read_u16()?;
            let pixel_clock = match reader.read_u8()? {
                0 => None,
                1 => Some(VcePixelClock::DivideByFour),
                2 => Some(VcePixelClock::DivideByThree),
                3 => Some(VcePixelClock::DivideByTwo),
                tag => bail!("invalid VCE pixel-clock tag in video frame save-state: {tag}"),
            };
            if usize::from(active_x_origin) >= PCE_ACTIVE_FRAME_WIDTH && pixel_clock.is_some() {
                bail!("invalid active video origin in save-state: {active_x_origin}");
            }
            if usize::from(active_width) > PCE_ACTIVE_FRAME_WIDTH {
                bail!("invalid active video width in save-state: {active_width}");
            }
            *row = PceVideoRowMetadata {
                active_x_origin,
                active_width,
                pixel_clock,
                background: None,
            };
        }
        *self = restored;
        Ok(())
    }

    pub fn render_active_line(
        &mut self,
        vdc: &mut HuC6270,
        vce: &HuC6260,
        display: VdcActiveDisplayLine,
        destination_line: u16,
        pixel_clock: VcePixelClock,
    ) -> Result<(), PceVideoRenderError> {
        let line = usize::from(destination_line);
        if line >= PCE_ACTIVE_FRAME_HEIGHT {
            return Err(PceVideoRenderError::ActiveLineOutOfBounds {
                line: destination_line,
            });
        }
        let width = usize::from(display.source_width());
        if width > PCE_ACTIVE_FRAME_WIDTH {
            return Err(PceVideoRenderError::ActiveWidthOutOfBounds {
                width: display.source_width(),
            });
        }

        let sprite_status = render_vdc_output_line(
            vdc,
            display,
            &mut self.background[..width],
            &mut self.sprites[..width],
            &mut self.vdc_one[..width],
        )?;
        resolve_palette(vce, &self.vdc_one[..width], &mut self.composited[..width]);

        let row_start = line * PCE_ACTIVE_FRAME_WIDTH * 4;
        let row_end = row_start + PCE_ACTIVE_FRAME_WIDTH * 4;
        let row = &mut self.framebuffer[row_start..row_end];
        for pixel in row.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&PCE_ACTIVE_FRAME_UNUSED_RGBA);
        }
        for (source, destination) in self.composited[..width]
            .iter()
            .zip(row.as_chunks_mut::<4>().0)
        {
            let [red, green, blue] = source.rgb8();
            destination.copy_from_slice(&[red, green, blue, 0xFF]);
        }
        self.rows[line] = PceVideoRowMetadata {
            active_x_origin: display.source_start(),
            active_width: display.source_width(),
            pixel_clock: Some(pixel_clock),
            background: background_line_debug(display),
        };
        vdc.latch_full_active_span_sprite_status(display, sprite_status);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_supergrafx_active_line(
        &mut self,
        vdc_one: &mut HuC6270,
        vdc_two: &mut HuC6270,
        vpc: &HuC6202,
        vce: &HuC6260,
        display_one: Option<VdcActiveDisplayLine>,
        display_two: Option<VdcActiveDisplayLine>,
        destination_line: u16,
        pixel_clock: VcePixelClock,
    ) -> Result<(), PceVideoRenderError> {
        let line = usize::from(destination_line);
        if line >= PCE_ACTIVE_FRAME_HEIGHT {
            return Err(PceVideoRenderError::ActiveLineOutOfBounds {
                line: destination_line,
            });
        }
        let span_one = display_span(VpcVdc::One, display_one)?;
        let span_two = display_span(VpcVdc::Two, display_two)?;
        let Some((union_start, union_end)) = union_span(span_one, span_two) else {
            return Ok(());
        };
        let width = union_end - union_start;

        self.vdc_one.fill(VpcVdcPixel::new(0x100));
        self.vdc_two.fill(VpcVdcPixel::new(0x100));
        let status_one = match (display_one, span_one) {
            (Some(display), Some((start, end))) => Some(render_vdc_output_line(
                vdc_one,
                display,
                &mut self.background[..end - start],
                &mut self.sprites[..end - start],
                &mut self.vdc_one[start..end],
            )?),
            _ => None,
        };
        let status_two = match (display_two, span_two) {
            (Some(display), Some((start, end))) => Some(render_vdc_output_line(
                vdc_two,
                display,
                &mut self.background[..end - start],
                &mut self.sprites[..end - start],
                &mut self.vdc_two[start..end],
            )?),
            _ => None,
        };

        for (local_x, physical_x) in (union_start..union_end).enumerate() {
            let selected = vpc.select_pixel(
                physical_x as u16,
                self.vdc_one[physical_x],
                self.vdc_two[physical_x],
            );
            let palette_index = selected.palette_index();
            self.composited[local_x] = CompositedPixel::new(
                palette_index,
                vce.palette()[usize::from(palette_index)].rgb8(),
            );
        }

        let row_start = line * PCE_ACTIVE_FRAME_WIDTH * 4;
        let row_end = row_start + PCE_ACTIVE_FRAME_WIDTH * 4;
        let row = &mut self.framebuffer[row_start..row_end];
        for pixel in row.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&PCE_ACTIVE_FRAME_UNUSED_RGBA);
        }
        for (source, destination) in self.composited[..width]
            .iter()
            .zip(row.as_chunks_mut::<4>().0)
        {
            let [red, green, blue] = source.rgb8();
            destination.copy_from_slice(&[red, green, blue, 0xFF]);
        }
        self.rows[line] = PceVideoRowMetadata {
            active_x_origin: union_start as u16,
            active_width: width as u16,
            pixel_clock: Some(pixel_clock),
            background: None,
        };
        if let (Some(display), Some(status)) = (display_one, status_one) {
            vdc_one.latch_full_active_span_sprite_status(display, status);
        }
        if let (Some(display), Some(status)) = (display_two, status_two) {
            vdc_two.latch_full_active_span_sprite_status(display, status);
        }
        Ok(())
    }
}

fn background_line_debug(display: VdcActiveDisplayLine) -> Option<PceBackgroundLineDebug> {
    let state = display.background();
    if !state.enabled() {
        return None;
    }
    let height = state.height_tiles() * 8;
    let virtual_y = (state.scroll_y() + usize::from(display.display_line()) % height) % height;
    let first_bat_word = (virtual_y / 8) * state.width_tiles() + state.scroll_x() / 8;
    Some(PceBackgroundLineDebug {
        scroll_x: state.scroll_x() as u16,
        virtual_y: virtual_y as u16,
        first_bat_word: first_bat_word as u16,
    })
}

fn render_vdc_output_line(
    vdc: &HuC6270,
    display: VdcActiveDisplayLine,
    background: &mut [u8],
    sprites: &mut [Option<SpritePixel>],
    output: &mut [VpcVdcPixel],
) -> Result<SpriteScanlineStatus, PceVideoRenderError> {
    let display_line = usize::from(display.display_line());
    let background_status = vdc
        .render_background_scanline(&display.background(), display_line, background)
        .map_err(PceVideoRenderError::Background)?;
    let sprite_status = vdc
        .render_sprite_scanline(&display.sprites(), display_line, sprites)
        .map_err(PceVideoRenderError::Sprite)?;
    let background = match background_status {
        BackgroundScanlineStatus::Disabled => DisplayLayerLine::Disabled,
        BackgroundScanlineStatus::Rendered => DisplayLayerLine::Rendered(background),
    };
    let sprites = match sprite_status {
        SpriteScanlineStatus::Disabled => DisplayLayerLine::Disabled,
        SpriteScanlineStatus::Rendered(_) => DisplayLayerLine::Rendered(sprites),
    };
    compose_vdc_output_scanline(background, sprites, output)
        .map_err(PceVideoRenderError::Composition)?;
    Ok(sprite_status)
}

fn resolve_palette(vce: &HuC6260, source: &[VpcVdcPixel], output: &mut [CompositedPixel]) {
    for (source, output) in source.iter().copied().zip(output) {
        let palette_index = source.palette_index();
        *output = CompositedPixel::new(
            palette_index,
            vce.palette()[usize::from(palette_index)].rgb8(),
        );
    }
}

fn display_span(
    vdc: VpcVdc,
    display: Option<VdcActiveDisplayLine>,
) -> Result<Option<(usize, usize)>, PceVideoRenderError> {
    let Some(display) = display else {
        return Ok(None);
    };
    let start = usize::from(display.source_start());
    let width = usize::from(display.source_width());
    let end = start + width;
    if end > PCE_ACTIVE_FRAME_WIDTH {
        return Err(PceVideoRenderError::ActiveSpanOutOfBounds {
            vdc,
            start: display.source_start(),
            width: display.source_width(),
        });
    }
    Ok(Some((start, end)))
}

fn union_span(one: Option<(usize, usize)>, two: Option<(usize, usize)>) -> Option<(usize, usize)> {
    match (one, two) {
        (None, None) => None,
        (Some(span), None) | (None, Some(span)) => Some(span),
        (Some((one_start, one_end)), Some((two_start, two_end))) => {
            Some((one_start.min(two_start), one_end.max(two_end)))
        }
    }
}
