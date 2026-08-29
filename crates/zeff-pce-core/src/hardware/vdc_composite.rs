use super::vce::HuC6260;
use super::vdc_sprite_render::{SpriteBackgroundPriority, SpritePixel};
use super::vpc::VpcVdcPixel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayLayerLine<'a, T> {
    Disabled,
    Rendered(&'a [T]),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompositedPixel {
    palette_index: u16,
    rgb8: [u8; 3],
}

impl CompositedPixel {
    #[inline]
    pub const fn palette_index(self) -> u16 {
        self.palette_index
    }

    #[inline]
    pub const fn rgb8(self) -> [u8; 3] {
        self.rgb8
    }

    #[inline]
    pub(crate) const fn new(palette_index: u16, rgb8: [u8; 3]) -> Self {
        Self {
            palette_index,
            rgb8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayCompositionError {
    BackgroundLengthMismatch { expected: usize, actual: usize },
    SpriteLengthMismatch { expected: usize, actual: usize },
}

impl HuC6260 {
    pub fn compose_scanline(
        &self,
        background: DisplayLayerLine<'_, u8>,
        sprites: DisplayLayerLine<'_, Option<SpritePixel>>,
        output: &mut [CompositedPixel],
    ) -> Result<(), DisplayCompositionError> {
        validate_layer_lengths(background, sprites, output.len())?;
        for (index, output_pixel) in output.iter_mut().enumerate() {
            let palette_index = vdc_output_pixel(background, sprites, index).palette_index();
            *output_pixel = CompositedPixel::new(palette_index, self.resolve_color(palette_index));
        }
        Ok(())
    }
}

pub fn compose_vdc_output_scanline(
    background: DisplayLayerLine<'_, u8>,
    sprites: DisplayLayerLine<'_, Option<SpritePixel>>,
    output: &mut [VpcVdcPixel],
) -> Result<(), DisplayCompositionError> {
    validate_layer_lengths(background, sprites, output.len())?;
    for (index, output_pixel) in output.iter_mut().enumerate() {
        *output_pixel = vdc_output_pixel(background, sprites, index);
    }
    Ok(())
}

fn validate_layer_lengths(
    background: DisplayLayerLine<'_, u8>,
    sprites: DisplayLayerLine<'_, Option<SpritePixel>>,
    expected: usize,
) -> Result<(), DisplayCompositionError> {
    if let DisplayLayerLine::Rendered(pixels) = background
        && pixels.len() != expected
    {
        return Err(DisplayCompositionError::BackgroundLengthMismatch {
            expected,
            actual: pixels.len(),
        });
    }
    if let DisplayLayerLine::Rendered(pixels) = sprites
        && pixels.len() != expected
    {
        return Err(DisplayCompositionError::SpriteLengthMismatch {
            expected,
            actual: pixels.len(),
        });
    }
    Ok(())
}

fn vdc_output_pixel(
    background: DisplayLayerLine<'_, u8>,
    sprites: DisplayLayerLine<'_, Option<SpritePixel>>,
    index: usize,
) -> VpcVdcPixel {
    let background_index = match background {
        DisplayLayerLine::Disabled => 0,
        DisplayLayerLine::Rendered(pixels) => pixels[index],
    };
    let sprite = match sprites {
        DisplayLayerLine::Disabled => None,
        DisplayLayerLine::Rendered(pixels) => pixels[index],
    };
    let palette_index = match sprite {
        Some(pixel) if pixel.background_priority() == SpriteBackgroundPriority::Sprite => {
            pixel.palette_index()
        }
        _ if background_index != 0 => u16::from(background_index),
        Some(pixel) => pixel.palette_index(),
        None => 0,
    };
    VpcVdcPixel::new(palette_index)
}
