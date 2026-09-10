use anyhow::{Result, bail};

use super::{
    MAX_PROJECT_FRAMES, MAX_TAS_INPUT_PATTERN_SPANS, TasBranch, TasDigitalInputMask,
    TasDigitalTransform, TasInputPattern,
};

const MAX_DIGITAL_TRANSFORM_RANGES: usize = 64;

impl TasBranch {
    pub(crate) fn prepare_digital_transform_ranges(
        &self,
        ranges: &[(u64, u64)],
        mask: TasDigitalInputMask,
        transform: TasDigitalTransform,
    ) -> Result<Vec<(u64, TasInputPattern)>> {
        validate_ranges(self, ranges)?;

        let mut prepared = Vec::with_capacity(ranges.len());
        let mut span_count = 0usize;
        for &(start, end) in ranges {
            let source = self.input_pattern(start, end - start)?;
            let transformed = source.with_digital_transform(mask, transform)?;
            if transformed == source {
                continue;
            }
            span_count = span_count
                .checked_add(transformed.spans().len())
                .ok_or_else(|| anyhow::anyhow!("digital input transform span count overflows"))?;
            if span_count > MAX_TAS_INPUT_PATTERN_SPANS {
                bail!(
                    "digital input transform output exceeds the limit of {MAX_TAS_INPUT_PATTERN_SPANS} spans"
                );
            }
            prepared.push((start, transformed));
        }
        Ok(prepared)
    }
}

pub(super) fn validate_ranges(branch: &TasBranch, ranges: &[(u64, u64)]) -> Result<()> {
    if ranges.is_empty() || ranges.len() > MAX_DIGITAL_TRANSFORM_RANGES {
        bail!(
            "digital input transform requires 1..={MAX_DIGITAL_TRANSFORM_RANGES} canonical ranges"
        );
    }

    let mut previous_end = None;
    for &(start, end) in ranges {
        if start >= end {
            bail!("digital input transform ranges must be non-empty");
        }
        if end > MAX_PROJECT_FRAMES || end > branch.frame_count() {
            bail!("digital input transform range extends past branch end");
        }
        if previous_end.is_some_and(|previous_end| start <= previous_end) {
            bail!(
                "digital input transform ranges must be sorted, non-overlapping, and non-adjacent"
            );
        }
        previous_end = Some(end);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
