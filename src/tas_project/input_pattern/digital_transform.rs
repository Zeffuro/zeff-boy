use crate::tas_project::TasControllerInput;
use anyhow::{Result, bail};

use super::{
    MAX_TAS_INPUT_PATTERN_SPANS, MAX_TAS_INPUT_PATTERN_TILE_STEPS, TasInputFrame, TasInputPattern,
    TasInputSpan, push_span,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TasDigitalInputMask {
    pub players: [TasControllerInput; 5],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TasDigitalTransform {
    Clear,
    Invert,
    Reverse,
}

#[derive(Clone, Copy)]
struct InputSegment {
    start: u64,
    end: u64,
    input: TasInputFrame,
}

impl TasInputPattern {
    pub fn with_digital_transform(
        &self,
        mask: TasDigitalInputMask,
        transform: TasDigitalTransform,
    ) -> Result<Self> {
        if mask_is_empty(mask) {
            bail!("digital input transform requires at least one selected control");
        }
        match transform {
            TasDigitalTransform::Clear => self.clear_masked_channels(mask),
            TasDigitalTransform::Invert => self.invert_masked_channels(mask),
            TasDigitalTransform::Reverse => self.reverse_masked_channels(mask),
        }
    }

    pub fn with_digital_overlay(&self, source: &Self, mask: TasDigitalInputMask) -> Result<Self> {
        if self.length != source.length {
            bail!("digital input overlay patterns must have equal lengths");
        }
        if mask_is_empty(mask) {
            bail!("digital input overlay requires at least one selected control");
        }
        let destination = self.complete_segments()?;
        let source = source.complete_segments()?;
        self.merge_masked_segments(&destination, &source, mask, overlay_channels)
    }

    fn clear_masked_channels(&self, mask: TasDigitalInputMask) -> Result<Self> {
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

    fn invert_masked_channels(&self, mask: TasDigitalInputMask) -> Result<Self> {
        let segments = self.complete_segments()?;
        let mut spans = Vec::with_capacity(segments.len().min(MAX_TAS_INPUT_PATTERN_SPANS));
        for segment in segments {
            let input = invert_channels(segment.input, mask);
            if input != TasInputFrame::default() {
                push_span(
                    &mut spans,
                    TasInputSpan {
                        start: segment.start,
                        length: segment.end - segment.start,
                        input,
                    },
                    MAX_TAS_INPUT_PATTERN_SPANS,
                )?;
            }
        }
        Self::new(self.length, spans)
    }

    fn reverse_masked_channels(&self, mask: TasDigitalInputMask) -> Result<Self> {
        let original = self.complete_segments()?;
        let reversed = original
            .iter()
            .rev()
            .map(|segment| InputSegment {
                start: self.length - segment.end,
                end: self.length - segment.start,
                input: segment.input,
            })
            .collect::<Vec<_>>();
        self.merge_masked_segments(&original, &reversed, mask, reverse_channels)
    }

    fn merge_masked_segments(
        &self,
        destination: &[InputSegment],
        source: &[InputSegment],
        mask: TasDigitalInputMask,
        merge: impl Fn(TasInputFrame, TasInputFrame, TasDigitalInputMask) -> TasInputFrame,
    ) -> Result<Self> {
        let mut spans = Vec::with_capacity(destination.len().min(MAX_TAS_INPUT_PATTERN_SPANS));
        let mut destination_index = 0;
        let mut source_index = 0;
        let mut steps = 0;
        while destination_index < destination.len() && source_index < source.len() {
            if steps == MAX_TAS_INPUT_PATTERN_TILE_STEPS {
                bail!(
                    "digital input transform exceeds the limit of {MAX_TAS_INPUT_PATTERN_TILE_STEPS} candidate runs"
                );
            }
            steps += 1;
            let destination = destination[destination_index];
            let source = source[source_index];
            let start = destination.start.max(source.start);
            let end = destination.end.min(source.end);
            let input = merge(destination.input, source.input, mask);
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
        Self::new(self.length, spans)
    }

    fn complete_segments(&self) -> Result<Vec<InputSegment>> {
        let mut segments = Vec::with_capacity(self.spans.len().saturating_mul(2).saturating_add(1));
        let mut cursor = 0;
        for span in &self.spans {
            if cursor < span.start {
                push_segment(&mut segments, cursor, span.start, TasInputFrame::default())?;
            }
            let end = span.start + span.length;
            push_segment(&mut segments, span.start, end, span.input)?;
            cursor = end;
        }
        if cursor < self.length {
            push_segment(&mut segments, cursor, self.length, TasInputFrame::default())?;
        }
        Ok(segments)
    }
}

fn mask_is_empty(mask: TasDigitalInputMask) -> bool {
    mask.players
        .iter()
        .all(|player| player.buttons == 0 && player.dpad == 0)
}

fn clear_channels(mut input: TasInputFrame, mask: TasDigitalInputMask) -> TasInputFrame {
    for (player, selected) in input.players.iter_mut().zip(mask.players) {
        player.buttons &= !selected.buttons;
        player.dpad &= !selected.dpad;
    }
    input
}

fn invert_channels(mut input: TasInputFrame, mask: TasDigitalInputMask) -> TasInputFrame {
    for (player, selected) in input.players.iter_mut().zip(mask.players) {
        player.buttons ^= selected.buttons;
        player.dpad ^= selected.dpad;
    }
    input
}

fn reverse_channels(
    mut input: TasInputFrame,
    mirrored: TasInputFrame,
    mask: TasDigitalInputMask,
) -> TasInputFrame {
    for ((player, mirrored), selected) in input
        .players
        .iter_mut()
        .zip(mirrored.players)
        .zip(mask.players)
    {
        player.buttons =
            (player.buttons & !selected.buttons) | (mirrored.buttons & selected.buttons);
        player.dpad = (player.dpad & !selected.dpad) | (mirrored.dpad & selected.dpad);
    }
    input
}

fn overlay_channels(
    mut destination: TasInputFrame,
    source: TasInputFrame,
    mask: TasDigitalInputMask,
) -> TasInputFrame {
    for ((player, source), selected) in destination
        .players
        .iter_mut()
        .zip(source.players)
        .zip(mask.players)
    {
        player.buttons = (player.buttons & !selected.buttons) | (source.buttons & selected.buttons);
        player.dpad = (player.dpad & !selected.dpad) | (source.dpad & selected.dpad);
    }
    destination
}

fn push_segment(
    segments: &mut Vec<InputSegment>,
    start: u64,
    end: u64,
    input: TasInputFrame,
) -> Result<()> {
    if segments.len() == MAX_TAS_INPUT_PATTERN_TILE_STEPS {
        bail!(
            "digital input transform exceeds the limit of {MAX_TAS_INPUT_PATTERN_TILE_STEPS} candidate runs"
        );
    }
    segments.push(InputSegment { start, end, input });
    Ok(())
}

#[cfg(test)]
mod tests;
