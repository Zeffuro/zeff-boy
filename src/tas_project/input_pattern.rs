use anyhow::{Result, bail};

use super::{MAX_PROJECT_FRAMES, MAX_PROJECT_INPUT_SPANS, TasBranch, TasInputFrame, TasInputSpan};

pub const MAX_TAS_INPUT_PATTERN_SPANS: usize = 4_096;
pub const MAX_TAS_INPUT_PATTERN_TILE_STEPS: usize = 8_192;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TasInputPattern {
    length: u64,
    spans: Vec<TasInputSpan>,
}

impl TasInputPattern {
    pub fn new(length: u64, spans: Vec<TasInputSpan>) -> Result<Self> {
        validate_pattern(length, &spans)?;
        Ok(Self { length, spans })
    }

    pub fn neutral(length: u64) -> Result<Self> {
        Self::new(length, Vec::new())
    }

    pub fn constant(length: u64, input: TasInputFrame) -> Result<Self> {
        if input == TasInputFrame::default() {
            return Self::neutral(length);
        }
        Self::new(
            length,
            vec![TasInputSpan {
                start: 0,
                length,
                input,
            }],
        )
    }

    pub fn length(&self) -> u64 {
        self.length
    }

    pub fn spans(&self) -> &[TasInputSpan] {
        &self.spans
    }

    pub fn tile_to_length(&self, length: u64) -> Result<Self> {
        validate_length(length)?;
        if self.spans.is_empty() {
            return Self::neutral(length);
        }
        if self.spans.len() == 1 && self.spans[0].start == 0 && self.spans[0].length == self.length
        {
            return Self::constant(length, self.spans[0].input);
        }

        let full_cycles = length / self.length;
        let remainder = length % self.length;
        let partial_steps = self.spans.partition_point(|span| span.start < remainder);
        let candidate_steps = usize::try_from(full_cycles)
            .ok()
            .and_then(|cycles| cycles.checked_mul(self.spans.len()))
            .and_then(|steps| steps.checked_add(partial_steps))
            .ok_or_else(|| anyhow::anyhow!("TAS input pattern tiling work overflows"))?;
        if candidate_steps > MAX_TAS_INPUT_PATTERN_TILE_STEPS {
            bail!(
                "TAS input pattern tiling requires {candidate_steps} candidate runs, above the limit of {MAX_TAS_INPUT_PATTERN_TILE_STEPS}"
            );
        }

        let mut tiled = Vec::with_capacity(candidate_steps.min(MAX_TAS_INPUT_PATTERN_SPANS));
        let cycle_count = full_cycles + u64::from(remainder != 0);
        for cycle in 0..cycle_count {
            let cycle_start = cycle
                .checked_mul(self.length)
                .ok_or_else(|| anyhow::anyhow!("TAS input pattern tiling offset overflows"))?;
            let span_count = if cycle < full_cycles {
                self.spans.len()
            } else {
                partial_steps
            };
            for span in &self.spans[..span_count] {
                let start = cycle_start
                    .checked_add(span.start)
                    .ok_or_else(|| anyhow::anyhow!("TAS input pattern tiling offset overflows"))?;
                let span_end = start
                    .checked_add(span.length)
                    .ok_or_else(|| anyhow::anyhow!("TAS input pattern tiling range overflows"))?;
                push_span(
                    &mut tiled,
                    TasInputSpan {
                        start,
                        length: span_end.min(length) - start,
                        input: span.input,
                    },
                    MAX_TAS_INPUT_PATTERN_SPANS,
                )?;
            }
        }
        Self::new(length, tiled)
    }
}

impl TasBranch {
    pub fn input_pattern(&self, start: u64, length: u64) -> Result<TasInputPattern> {
        validate_length(length)?;
        let end = start
            .checked_add(length)
            .ok_or_else(|| anyhow::anyhow!("TAS input pattern range overflows"))?;
        if end > self.frame_count {
            bail!("TAS input pattern range extends past branch end");
        }

        let first = self
            .input_spans
            .partition_point(|span| span.start.saturating_add(span.length) <= start);
        let mut spans = Vec::new();
        for span in &self.input_spans[first..] {
            if span.start >= end {
                break;
            }
            if spans.len() == MAX_TAS_INPUT_PATTERN_SPANS {
                bail!("TAS input pattern exceeds the limit of {MAX_TAS_INPUT_PATTERN_SPANS} spans");
            }
            let clipped_start = span.start.max(start);
            let clipped_end = (span.start + span.length).min(end);
            spans.push(TasInputSpan {
                start: clipped_start - start,
                length: clipped_end - clipped_start,
                input: span.input,
            });
        }
        TasInputPattern::new(length, spans)
    }
}

pub(super) fn replace_branch_input_pattern(
    branch: &mut TasBranch,
    start: u64,
    pattern: &TasInputPattern,
) -> Result<()> {
    let end = start
        .checked_add(pattern.length)
        .ok_or_else(|| anyhow::anyhow!("TAS input pattern replacement range overflows"))?;
    if end > branch.frame_count {
        bail!("TAS input pattern replacement extends past branch end");
    }

    let first = branch
        .input_spans
        .partition_point(|span| span.start + span.length <= start);
    let after = branch.input_spans.partition_point(|span| span.start < end);
    let capacity = branch
        .input_spans
        .len()
        .checked_add(pattern.spans.len())
        .and_then(|len| len.checked_add(2))
        .ok_or_else(|| anyhow::anyhow!("TAS input pattern replacement size overflows"))?;
    let mut spans = Vec::with_capacity(capacity.min(MAX_PROJECT_INPUT_SPANS));

    for span in &branch.input_spans[..first] {
        push_span(&mut spans, *span, MAX_PROJECT_INPUT_SPANS)?;
    }
    if let Some(span) = branch.input_spans.get(first)
        && span.start < start
    {
        push_span(
            &mut spans,
            TasInputSpan {
                length: start - span.start,
                ..*span
            },
            MAX_PROJECT_INPUT_SPANS,
        )?;
    }
    for span in &pattern.spans {
        push_span(
            &mut spans,
            TasInputSpan {
                start: start
                    .checked_add(span.start)
                    .ok_or_else(|| anyhow::anyhow!("TAS input pattern offset overflows"))?,
                ..*span
            },
            MAX_PROJECT_INPUT_SPANS,
        )?;
    }
    if let Some(span) = after
        .checked_sub(1)
        .and_then(|index| branch.input_spans.get(index))
    {
        let span_end = span.start + span.length;
        if span_end > end {
            push_span(
                &mut spans,
                TasInputSpan {
                    start: end,
                    length: span_end - end,
                    input: span.input,
                },
                MAX_PROJECT_INPUT_SPANS,
            )?;
        }
    }
    for span in &branch.input_spans[after..] {
        push_span(&mut spans, *span, MAX_PROJECT_INPUT_SPANS)?;
    }
    if spans != branch.input_spans {
        branch.input_spans = spans;
    }
    Ok(())
}

fn validate_length(length: u64) -> Result<()> {
    if length == 0 {
        bail!("TAS input pattern cannot be empty");
    }
    if length > MAX_PROJECT_FRAMES {
        bail!("TAS input pattern exceeds the {MAX_PROJECT_FRAMES}-frame limit");
    }
    Ok(())
}

fn validate_pattern(length: u64, spans: &[TasInputSpan]) -> Result<()> {
    validate_length(length)?;
    if spans.len() > MAX_TAS_INPUT_PATTERN_SPANS {
        bail!("TAS input pattern exceeds the limit of {MAX_TAS_INPUT_PATTERN_SPANS} spans");
    }
    let mut previous_end = 0;
    let mut previous_input = None;
    for span in spans {
        if span.length == 0 {
            bail!("TAS input pattern spans cannot be empty");
        }
        if span.input == TasInputFrame::default() {
            bail!("neutral TAS input pattern spans must be omitted");
        }
        let end = span
            .start
            .checked_add(span.length)
            .ok_or_else(|| anyhow::anyhow!("TAS input pattern span overflows"))?;
        if end > length {
            bail!("TAS input pattern span extends past pattern end");
        }
        if span.start < previous_end {
            bail!("TAS input pattern spans must be sorted and non-overlapping");
        }
        if span.start == previous_end && previous_input == Some(span.input) {
            bail!("adjacent identical TAS input pattern spans must be merged");
        }
        previous_end = end;
        previous_input = Some(span.input);
    }
    Ok(())
}

fn push_span(spans: &mut Vec<TasInputSpan>, span: TasInputSpan, max_spans: usize) -> Result<()> {
    if let Some(previous) = spans.last_mut()
        && previous.start + previous.length == span.start
        && previous.input == span.input
    {
        previous.length = previous
            .length
            .checked_add(span.length)
            .ok_or_else(|| anyhow::anyhow!("TAS input span merge overflows"))?;
        return Ok(());
    }
    if spans.len() == max_spans {
        bail!("TAS input pattern output exceeds the limit of {max_spans} spans");
    }
    spans.push(span);
    Ok(())
}
