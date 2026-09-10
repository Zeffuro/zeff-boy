use anyhow::{Result, bail};

use super::{
    MAX_TAS_INPUT_PATTERN_SPANS, MAX_TAS_INPUT_PATTERN_TILE_STEPS, TasInputFrame, TasInputPattern,
    TasInputSpan, push_span,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TasSpecialInputMask {
    pub zapper: bool,
    pub tilt_x: bool,
    pub tilt_y: bool,
    pub camera: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TasSpecialTransform {
    Clear,
    Reverse,
}

#[derive(Clone, Copy)]
struct SpecialSegment {
    start: u64,
    end: u64,
    input: TasInputFrame,
}

impl TasInputPattern {
    pub fn with_special_transform(
        &self,
        mask: TasSpecialInputMask,
        transform: TasSpecialTransform,
    ) -> Result<Self> {
        if mask_is_empty(mask) {
            bail!("special input transform requires at least one selected channel");
        }
        match transform {
            TasSpecialTransform::Clear => self.clear_special_channels(mask),
            TasSpecialTransform::Reverse => self.reverse_special_channels(mask),
        }
    }

    fn clear_special_channels(&self, mask: TasSpecialInputMask) -> Result<Self> {
        let mut spans = Vec::with_capacity(self.spans.len());
        for span in &self.spans {
            let input = clear_channels(span.input, mask);
            if input != TasInputFrame::default() {
                push_span(
                    &mut spans,
                    TasInputSpan {
                        start: span.start,
                        length: span.length,
                        input,
                    },
                    MAX_TAS_INPUT_PATTERN_SPANS,
                )?;
            }
        }
        Self::new(self.length, spans)
    }

    fn reverse_special_channels(&self, mask: TasSpecialInputMask) -> Result<Self> {
        let original = complete_special_segments(self)?;
        let reversed = original
            .iter()
            .rev()
            .map(|segment| SpecialSegment {
                start: self.length - segment.end,
                end: self.length - segment.start,
                input: segment.input,
            })
            .collect::<Vec<_>>();
        merge_special_segments(self.length, &original, &reversed, mask)
    }
}

fn mask_is_empty(mask: TasSpecialInputMask) -> bool {
    !mask.zapper && !mask.tilt_x && !mask.tilt_y && !mask.camera
}

fn clear_channels(mut input: TasInputFrame, mask: TasSpecialInputMask) -> TasInputFrame {
    if mask.zapper {
        input.zapper = Default::default();
    }
    if mask.tilt_x {
        input.tilt_x_bits = 0;
    }
    if mask.tilt_y {
        input.tilt_y_bits = 0;
    }
    if mask.camera {
        input.camera = Default::default();
    }
    input
}

fn reverse_channels(
    mut destination: TasInputFrame,
    source: TasInputFrame,
    mask: TasSpecialInputMask,
) -> TasInputFrame {
    if mask.zapper {
        destination.zapper = source.zapper;
    }
    if mask.tilt_x {
        destination.tilt_x_bits = source.tilt_x_bits;
    }
    if mask.tilt_y {
        destination.tilt_y_bits = source.tilt_y_bits;
    }
    if mask.camera {
        destination.camera = source.camera;
    }
    destination
}

fn complete_special_segments(pattern: &TasInputPattern) -> Result<Vec<SpecialSegment>> {
    let mut segments = Vec::with_capacity(pattern.spans.len().saturating_mul(2).saturating_add(1));
    let mut cursor = 0;
    for span in &pattern.spans {
        if cursor < span.start {
            push_special_segment(&mut segments, cursor, span.start, TasInputFrame::default())?;
        }
        let end = span.start + span.length;
        push_special_segment(&mut segments, span.start, end, span.input)?;
        cursor = end;
    }
    if cursor < pattern.length {
        push_special_segment(
            &mut segments,
            cursor,
            pattern.length,
            TasInputFrame::default(),
        )?;
    }
    Ok(segments)
}

fn push_special_segment(
    segments: &mut Vec<SpecialSegment>,
    start: u64,
    end: u64,
    input: TasInputFrame,
) -> Result<()> {
    if segments.len() == MAX_TAS_INPUT_PATTERN_TILE_STEPS {
        bail!(
            "special input transform exceeds the limit of {MAX_TAS_INPUT_PATTERN_TILE_STEPS} candidate runs"
        );
    }
    segments.push(SpecialSegment { start, end, input });
    Ok(())
}

fn merge_special_segments(
    length: u64,
    destination: &[SpecialSegment],
    source: &[SpecialSegment],
    mask: TasSpecialInputMask,
) -> Result<TasInputPattern> {
    let mut spans = Vec::with_capacity(destination.len().min(MAX_TAS_INPUT_PATTERN_SPANS));
    let mut destination_index = 0;
    let mut source_index = 0;
    let mut steps = 0;
    while destination_index < destination.len() && source_index < source.len() {
        if steps == MAX_TAS_INPUT_PATTERN_TILE_STEPS {
            bail!(
                "special input transform exceeds the limit of {MAX_TAS_INPUT_PATTERN_TILE_STEPS} candidate runs"
            );
        }
        steps += 1;
        let destination = destination[destination_index];
        let source = source[source_index];
        let start = destination.start.max(source.start);
        let end = destination.end.min(source.end);
        let input = reverse_channels(destination.input, source.input, mask);
        if input != TasInputFrame::default() {
            push_span(
                &mut spans,
                TasInputSpan {
                    start,
                    length: end - start,
                    input,
                },
                MAX_TAS_INPUT_PATTERN_SPANS,
            )?;
        }
        if destination.end == end {
            destination_index += 1;
        }
        if source.end == end {
            source_index += 1;
        }
    }
    TasInputPattern::new(length, spans)
}

#[cfg(test)]
mod tests;
