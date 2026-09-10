use anyhow::{Result, bail};

use super::{
    MAX_TAS_INPUT_PATTERN_SPANS, MAX_TAS_INPUT_PATTERN_TILE_STEPS, TasInputFrame, TasInputPattern,
    TasInputSpan, push_span,
};

impl TasInputPattern {
    pub fn with_digital_autofire(
        &self,
        player: usize,
        buttons_mask: u8,
        dpad_mask: u8,
        period: u8,
        on_frames: u8,
    ) -> Result<Self> {
        if player >= 5 || (buttons_mask | dpad_mask) == 0 {
            bail!("autofire requires a valid player and at least one digital control");
        }
        if !(1..=60).contains(&period) || on_frames > period {
            bail!("autofire requires a period of 1-60 frames and ON frames within that period");
        }
        let period = u64::from(period);
        let on_frames = u64::from(on_frames);
        let mut spans = Vec::new();
        let mut source_index = 0;
        let mut cursor = 0;
        let mut steps = 0;
        while cursor < self.length {
            if steps == MAX_TAS_INPUT_PATTERN_TILE_STEPS {
                bail!(
                    "autofire exceeds the limit of {MAX_TAS_INPUT_PATTERN_TILE_STEPS} runs; select fewer frames or use a longer period"
                );
            }
            steps += 1;
            while self
                .spans
                .get(source_index)
                .is_some_and(|span| span.start + span.length <= cursor)
            {
                source_index += 1;
            }
            let (mut input, source_end) = match self.spans.get(source_index) {
                Some(span) if span.start <= cursor => (span.input, span.start + span.length),
                Some(span) => (TasInputFrame::default(), span.start),
                None => (TasInputFrame::default(), self.length),
            };
            let phase = cursor % period;
            let pressed = phase < on_frames;
            let pattern_end = if on_frames == 0 || on_frames == period {
                self.length
            } else if pressed {
                cursor + on_frames - phase
            } else {
                cursor + period - phase
            };
            let end = source_end.min(pattern_end).min(self.length);
            let controller = &mut input.players[player];
            controller.buttons =
                (controller.buttons & !buttons_mask) | if pressed { buttons_mask } else { 0 };
            controller.dpad = (controller.dpad & !dpad_mask) | if pressed { dpad_mask } else { 0 };
            if input != TasInputFrame::default() {
                push_span(
                    &mut spans,
                    TasInputSpan {
                        start: cursor,
                        length: end - cursor,
                        input,
                    },
                    MAX_TAS_INPUT_PATTERN_SPANS,
                )?;
            }
            cursor = end;
        }
        Self::new(self.length, spans)
    }
}

#[cfg(test)]
mod tests;
