use anyhow::{Result, bail};

use super::{
    MAX_TAS_INPUT_PATTERN_SPANS, TasBranch, TasInputPattern, TasSpecialInputMask,
    TasSpecialTransform, digital_ranges::validate_ranges,
};

impl TasBranch {
    pub(crate) fn prepare_special_transform_ranges(
        &self,
        ranges: &[(u64, u64)],
        mask: TasSpecialInputMask,
        transform: TasSpecialTransform,
    ) -> Result<Vec<(u64, TasInputPattern)>> {
        validate_ranges(self, ranges)?;

        let mut prepared = Vec::with_capacity(ranges.len());
        let mut span_count = 0usize;
        for &(start, end) in ranges {
            let source = self.input_pattern(start, end - start)?;
            let transformed = source.with_special_transform(mask, transform)?;
            if transformed == source {
                continue;
            }
            span_count = span_count
                .checked_add(transformed.spans().len())
                .ok_or_else(|| anyhow::anyhow!("special input transform span count overflows"))?;
            if span_count > MAX_TAS_INPUT_PATTERN_SPANS {
                bail!(
                    "special input transform output exceeds the limit of {MAX_TAS_INPUT_PATTERN_SPANS} spans"
                );
            }
            prepared.push((start, transformed));
        }
        Ok(prepared)
    }
}

#[cfg(test)]
mod tests;
