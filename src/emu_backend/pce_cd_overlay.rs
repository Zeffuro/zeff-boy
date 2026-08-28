use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use zeff_pce_core::hardware::{CD_RAW_SECTOR_BYTES, CdSourceError, CdTrackSource};

use crate::patching::{PpfPlanFallback, PpfPlanLimits, PpfPlanOutcome, plan_ppf_patch};

pub(crate) const PATCH_BYTES_LIMIT: usize = 16 * 1024 * 1024;
const PATCH_RECORDS_LIMIT: usize = 131_072;
const REPLACEMENT_BYTES_LIMIT: usize = 64 * 1024 * 1024;
const OVERLAY_SPANS_LIMIT: usize = 131_072;

#[derive(Clone, Copy)]
pub(crate) struct PatchOverlayLimits {
    ppf: PpfPlanLimits,
    max_spans: usize,
    max_replacement_bytes: usize,
}

impl PatchOverlayLimits {
    pub(crate) const BOUNDED: Self = Self {
        ppf: PpfPlanLimits::new(
            PATCH_BYTES_LIMIT,
            PATCH_RECORDS_LIMIT,
            REPLACEMENT_BYTES_LIMIT,
        ),
        max_spans: OVERLAY_SPANS_LIMIT,
        max_replacement_bytes: REPLACEMENT_BYTES_LIMIT,
    };

    #[cfg(test)]
    const fn new(ppf: PpfPlanLimits, max_spans: usize, max_replacement_bytes: usize) -> Self {
        Self {
            ppf,
            max_spans,
            max_replacement_bytes,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PatchOverlayFallback {
    Ppf(PpfPlanFallback),
    Spans { spans: usize, limit: usize },
    ReplacementBytes { bytes: usize, limit: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PatchOverlayApply {
    Applied,
    Fallback(PatchOverlayFallback),
}

pub(crate) enum PatchOverlayStack {
    Applied(Vec<(String, bool)>),
    Fallback,
}

#[derive(Clone)]
struct OverlaySpan {
    start: usize,
    data: Arc<[u8]>,
    data_start: usize,
    len: usize,
}

impl OverlaySpan {
    fn end(&self) -> usize {
        self.start + self.len
    }

    fn bytes(&self) -> &[u8] {
        &self.data[self.data_start..self.data_start + self.len]
    }
}

pub(crate) struct PatchOverlayBuilder {
    base: Arc<dyn CdTrackSource>,
    spans: BTreeMap<usize, OverlaySpan>,
    replacement_bytes: usize,
    limits: PatchOverlayLimits,
}

struct ConcatenatedTrackSource {
    sources: Arc<[Arc<dyn CdTrackSource>]>,
    starts: Arc<[usize]>,
    len: usize,
}

impl ConcatenatedTrackSource {
    fn new(sources: &[Arc<dyn CdTrackSource>]) -> Option<Self> {
        let mut starts = Vec::with_capacity(sources.len());
        let mut len = 0_usize;
        for source in sources {
            starts.push(len);
            len = len.checked_add(source.len())?;
        }
        Some(Self {
            sources: sources.to_vec().into(),
            starts: starts.into(),
            len,
        })
    }
}

impl CdTrackSource for ConcatenatedTrackSource {
    fn len(&self) -> usize {
        self.len
    }

    fn payload_hash(&self) -> [u8; 32] {
        [0; 32]
    }

    fn read_exact_at(&self, offset: usize, output: &mut [u8]) -> Result<(), CdSourceError> {
        checked_end(offset, output.len(), self.len)?;
        if output.len() > CD_RAW_SECTOR_BYTES {
            return Err(CdSourceError::ReadFailed);
        }
        let mut staging = [0; CD_RAW_SECTOR_BYTES];
        let staging = &mut staging[..output.len()];
        let mut cursor = offset;
        let mut destination = 0;
        while destination < staging.len() {
            let segment = self
                .starts
                .partition_point(|&start| start <= cursor)
                .saturating_sub(1);
            let source = self.sources.get(segment).ok_or(CdSourceError::ReadFailed)?;
            let local = cursor - self.starts[segment];
            let count = (source.len() - local).min(staging.len() - destination);
            if count == 0 {
                return Err(CdSourceError::ReadFailed);
            }
            source.read_exact_at(local, &mut staging[destination..destination + count])?;
            cursor += count;
            destination += count;
        }
        output.copy_from_slice(staging);
        Ok(())
    }

    fn visit_payload(
        &self,
        _sector_bytes: usize,
        _visitor: &mut dyn FnMut(&[u8]),
    ) -> Result<(), CdSourceError> {
        Err(CdSourceError::ReadFailed)
    }
}

impl PatchOverlayBuilder {
    pub(crate) fn new(base: Arc<dyn CdTrackSource>) -> Self {
        Self::with_limits(base, PatchOverlayLimits::BOUNDED)
    }

    pub(crate) fn for_tracks(sources: &[Arc<dyn CdTrackSource>]) -> Option<Self> {
        let base: Arc<dyn CdTrackSource> = Arc::new(ConcatenatedTrackSource::new(sources)?);
        Some(Self::new(base))
    }

    fn with_limits(base: Arc<dyn CdTrackSource>, limits: PatchOverlayLimits) -> Self {
        Self {
            base,
            spans: BTreeMap::new(),
            replacement_bytes: 0,
            limits,
        }
    }

    pub(crate) fn apply_ppf(&mut self, patch: &[u8]) -> anyhow::Result<PatchOverlayApply> {
        let outcome = plan_ppf_patch(
            patch,
            self.base.len() as u64,
            self.limits.ppf,
            &mut |offset, output| {
                let offset = usize::try_from(offset).context("PPF source offset is too large")?;
                self.read_current(offset, output).map_err(Into::into)
            },
        )?;
        let plan = match outcome {
            PpfPlanOutcome::Planned(plan) => plan,
            PpfPlanOutcome::Fallback(reason) => {
                return Ok(PatchOverlayApply::Fallback(PatchOverlayFallback::Ppf(
                    reason,
                )));
            }
        };
        let added_bytes = plan
            .writes()
            .iter()
            .try_fold(0_usize, |total, write| total.checked_add(write.bytes.len()));
        let replacement_bytes =
            added_bytes.and_then(|bytes| self.replacement_bytes.checked_add(bytes));
        let Some(replacement_bytes) = replacement_bytes else {
            return Ok(PatchOverlayApply::Fallback(
                PatchOverlayFallback::ReplacementBytes {
                    bytes: usize::MAX,
                    limit: self.limits.max_replacement_bytes,
                },
            ));
        };
        if replacement_bytes > self.limits.max_replacement_bytes {
            return Ok(PatchOverlayApply::Fallback(
                PatchOverlayFallback::ReplacementBytes {
                    bytes: replacement_bytes,
                    limit: self.limits.max_replacement_bytes,
                },
            ));
        }

        let mut spans = self.spans.clone();
        for write in plan.writes() {
            if write.bytes.is_empty() {
                continue;
            }
            let start = usize::try_from(write.offset).context("PPF target offset is too large")?;
            insert_span(&mut spans, start, write.bytes);
            if spans.len() > self.limits.max_spans {
                return Ok(PatchOverlayApply::Fallback(PatchOverlayFallback::Spans {
                    spans: spans.len(),
                    limit: self.limits.max_spans,
                }));
            }
        }
        self.spans = spans;
        self.replacement_bytes = replacement_bytes;
        Ok(PatchOverlayApply::Applied)
    }

    #[cfg(test)]
    pub(crate) fn finish(self) -> Arc<PatchOverlaySource> {
        Arc::new(PatchOverlaySource {
            len: self.base.len(),
            base: self.base,
            spans: self.spans.into_values().collect::<Vec<_>>().into(),
        })
    }

    pub(crate) fn finish_tracks(
        self,
        sources: Vec<Arc<dyn CdTrackSource>>,
    ) -> Option<Vec<Arc<dyn CdTrackSource>>> {
        let expected_len = sources
            .iter()
            .try_fold(0_usize, |total, source| total.checked_add(source.len()))?;
        if expected_len != self.base.len() {
            return None;
        }
        let spans = self.spans.into_values().collect::<Vec<_>>();
        let mut start = 0_usize;
        let mut output = Vec::with_capacity(sources.len());
        for base in sources {
            let end = start.checked_add(base.len())?;
            let local_spans = spans
                .iter()
                .filter_map(|span| clip_span(span, start, end))
                .collect::<Vec<_>>()
                .into();
            let source: Arc<dyn CdTrackSource> = Arc::new(PatchOverlaySource {
                len: base.len(),
                base,
                spans: local_spans,
            });
            output.push(source);
            start = end;
        }
        Some(output)
    }

    fn read_current(&self, offset: usize, output: &mut [u8]) -> Result<(), CdSourceError> {
        read_overlay(&self.base, self.spans.values(), offset, output)
    }
}

pub(crate) fn apply_ppf_stack(
    builder: &mut PatchOverlayBuilder,
    dir: &Path,
    mods: &[crate::mods::ModEntry],
) -> PatchOverlayStack {
    let enabled = mods
        .iter()
        .filter(|entry| entry.enabled)
        .collect::<Vec<_>>();
    if enabled.iter().any(|entry| {
        !Path::new(&entry.filename)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("ppf"))
    }) {
        return PatchOverlayStack::Fallback;
    }

    let mut applied = Vec::with_capacity(enabled.len());
    for entry in enabled {
        let Some(patch) = read_patch(&dir.join(&entry.filename)) else {
            return PatchOverlayStack::Fallback;
        };
        let Ok(result) = builder.apply_ppf(&patch) else {
            return PatchOverlayStack::Fallback;
        };
        if !matches!(result, PatchOverlayApply::Applied) {
            return PatchOverlayStack::Fallback;
        }
        applied.push((
            entry.filename.clone(),
            !crate::patching::ppf_has_source_validation(&patch),
        ));
    }
    PatchOverlayStack::Applied(applied)
}

pub(crate) fn log_ppf_overlay(applied: &[(String, bool)]) {
    let warnings = applied.iter().filter(|(_, warning)| *warning).count();
    for (filename, warning) in applied {
        log::info!("Applied PC Engine CD mod: {filename}");
        if *warning {
            log::warn!(
                "Mod warning: {filename}: patch has no source check; verify the exact disc revision"
            );
        }
    }
    log::info!(
        "Applied {} mod(s) to PC Engine CD set ({warnings} warnings)",
        applied.len()
    );
}

pub(crate) fn slice_source(
    base: Arc<dyn CdTrackSource>,
    start: usize,
    len: usize,
) -> Option<Arc<dyn CdTrackSource>> {
    start.checked_add(len).filter(|&end| end <= base.len())?;
    Some(Arc::new(SourceSlice { base, start, len }))
}

fn read_patch(path: &Path) -> Option<Vec<u8>> {
    let file = File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > PATCH_BYTES_LIMIT as u64 {
        return None;
    }
    let mut patch = Vec::with_capacity(metadata.len() as usize);
    file.take(PATCH_BYTES_LIMIT as u64 + 1)
        .read_to_end(&mut patch)
        .ok()?;
    (patch.len() <= PATCH_BYTES_LIMIT).then_some(patch)
}

struct SourceSlice {
    base: Arc<dyn CdTrackSource>,
    start: usize,
    len: usize,
}

impl CdTrackSource for SourceSlice {
    fn len(&self) -> usize {
        self.len
    }

    fn payload_hash(&self) -> [u8; 32] {
        [0; 32]
    }

    fn read_exact_at(&self, offset: usize, output: &mut [u8]) -> Result<(), CdSourceError> {
        checked_end(offset, output.len(), self.len)?;
        if output.len() > CD_RAW_SECTOR_BYTES {
            return Err(CdSourceError::ReadFailed);
        }
        let mut staging = [0; CD_RAW_SECTOR_BYTES];
        let staging = &mut staging[..output.len()];
        self.base.read_exact_at(self.start + offset, staging)?;
        output.copy_from_slice(staging);
        Ok(())
    }

    fn visit_payload(
        &self,
        sector_bytes: usize,
        visitor: &mut dyn FnMut(&[u8]),
    ) -> Result<(), CdSourceError> {
        if sector_bytes == 0
            || sector_bytes > CD_RAW_SECTOR_BYTES
            || !self.len.is_multiple_of(sector_bytes)
        {
            return Err(CdSourceError::ReadFailed);
        }
        let mut staging = [0; CD_RAW_SECTOR_BYTES];
        for offset in (0..self.len).step_by(sector_bytes) {
            let bytes = &mut staging[..sector_bytes];
            self.base.read_exact_at(self.start + offset, bytes)?;
            visitor(bytes);
        }
        Ok(())
    }
}

fn clip_span(span: &OverlaySpan, range_start: usize, range_end: usize) -> Option<OverlaySpan> {
    let start = span.start.max(range_start);
    let end = span.end().min(range_end);
    (start < end).then(|| OverlaySpan {
        start: start - range_start,
        data: span.data.clone(),
        data_start: span.data_start + start - span.start,
        len: end - start,
    })
}

#[derive(Clone)]
pub(crate) struct PatchOverlaySource {
    base: Arc<dyn CdTrackSource>,
    spans: Arc<[OverlaySpan]>,
    len: usize,
}

impl CdTrackSource for PatchOverlaySource {
    fn len(&self) -> usize {
        self.len
    }

    fn payload_hash(&self) -> [u8; 32] {
        [0; 32]
    }

    fn read_exact_at(&self, offset: usize, output: &mut [u8]) -> Result<(), CdSourceError> {
        checked_end(offset, output.len(), self.len)?;
        if output.len() > CD_RAW_SECTOR_BYTES {
            return Err(CdSourceError::ReadFailed);
        }
        let mut staging = [0; CD_RAW_SECTOR_BYTES];
        let staging = &mut staging[..output.len()];
        read_overlay(&self.base, self.spans.iter(), offset, staging)?;
        output.copy_from_slice(staging);
        Ok(())
    }

    fn visit_payload(
        &self,
        sector_bytes: usize,
        visitor: &mut dyn FnMut(&[u8]),
    ) -> Result<(), CdSourceError> {
        if sector_bytes == 0
            || sector_bytes > CD_RAW_SECTOR_BYTES
            || !self.len.is_multiple_of(sector_bytes)
        {
            return Err(CdSourceError::ReadFailed);
        }
        let mut offset = 0_usize;
        let mut invalid = false;
        let mut staging = [0; CD_RAW_SECTOR_BYTES];
        self.base.visit_payload(sector_bytes, &mut |bytes| {
            let Some(end) = offset.checked_add(bytes.len()) else {
                invalid = true;
                return;
            };
            if invalid || bytes.is_empty() || bytes.len() > sector_bytes || end > self.len {
                invalid = true;
                return;
            }
            let output = &mut staging[..bytes.len()];
            output.copy_from_slice(bytes);
            apply_spans(&self.spans, offset, output);
            visitor(output);
            offset = end;
        })?;
        if invalid || offset != self.len {
            return Err(CdSourceError::ReadFailed);
        }
        Ok(())
    }
}

fn insert_span(spans: &mut BTreeMap<usize, OverlaySpan>, start: usize, bytes: &[u8]) {
    let end = start + bytes.len();
    let mut overlaps = Vec::new();
    if let Some((&key, span)) = spans.range(..=start).next_back()
        && span.end() > start
    {
        overlaps.push(key);
    }
    overlaps.extend(spans.range(start..end).map(|(&key, _)| key));
    overlaps.sort_unstable();
    overlaps.dedup();

    for key in overlaps {
        let old = spans.remove(&key).unwrap();
        let old_end = old.end();
        if old.start < start {
            spans.insert(
                old.start,
                OverlaySpan {
                    start: old.start,
                    data: old.data.clone(),
                    data_start: old.data_start,
                    len: start - old.start,
                },
            );
        }
        if old_end > end {
            spans.insert(
                end,
                OverlaySpan {
                    start: end,
                    data: old.data,
                    data_start: old.data_start + end - old.start,
                    len: old_end - end,
                },
            );
        }
    }
    spans.insert(
        start,
        OverlaySpan {
            start,
            data: Arc::from(bytes),
            data_start: 0,
            len: bytes.len(),
        },
    );
}

fn read_overlay<'a>(
    base: &Arc<dyn CdTrackSource>,
    spans: impl Iterator<Item = &'a OverlaySpan>,
    offset: usize,
    output: &mut [u8],
) -> Result<(), CdSourceError> {
    checked_end(offset, output.len(), base.len())?;
    base.read_exact_at(offset, output)?;
    apply_span_iter(spans, offset, output);
    Ok(())
}

fn apply_spans(spans: &[OverlaySpan], offset: usize, output: &mut [u8]) {
    let first = spans.partition_point(|span| span.end() <= offset);
    apply_span_iter(spans[first..].iter(), offset, output);
}

fn apply_span_iter<'a>(
    spans: impl Iterator<Item = &'a OverlaySpan>,
    offset: usize,
    output: &mut [u8],
) {
    let end = offset + output.len();
    for span in spans {
        if span.start >= end {
            break;
        }
        let copy_start = span.start.max(offset);
        let copy_end = span.end().min(end);
        if copy_start < copy_end {
            let source_start = copy_start - span.start;
            let target_start = copy_start - offset;
            output[target_start..target_start + copy_end - copy_start]
                .copy_from_slice(&span.bytes()[source_start..source_start + copy_end - copy_start]);
        }
    }
}

fn checked_end(offset: usize, bytes: usize, source_len: usize) -> Result<usize, CdSourceError> {
    offset
        .checked_add(bytes)
        .filter(|&end| end <= source_len)
        .ok_or(CdSourceError::OutOfRange {
            offset,
            bytes,
            source_len,
        })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use sha2::{Digest, Sha256};
    use zeff_pce_core::hardware::{
        CD_USER_SECTOR_BYTES, CdDisc, CdDiscError, CdTrack, CdTrackMode,
    };

    use super::*;
    use crate::patching::{apply_ppf_patch, apply_ppf_patch_segments};

    struct SyntheticSource {
        data: Arc<[u8]>,
        hash: [u8; 32],
        reads: AtomicUsize,
        visits: AtomicUsize,
        fail_read: AtomicBool,
        fail_visit: AtomicBool,
    }

    impl SyntheticSource {
        fn new(data: Vec<u8>) -> Self {
            Self {
                hash: Sha256::digest(&data).into(),
                data: data.into(),
                reads: AtomicUsize::new(0),
                visits: AtomicUsize::new(0),
                fail_read: AtomicBool::new(false),
                fail_visit: AtomicBool::new(false),
            }
        }
    }

    impl CdTrackSource for SyntheticSource {
        fn len(&self) -> usize {
            self.data.len()
        }

        fn payload_hash(&self) -> [u8; 32] {
            self.hash
        }

        fn read_exact_at(&self, offset: usize, output: &mut [u8]) -> Result<(), CdSourceError> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            let end = checked_end(offset, output.len(), self.len())?;
            if self.fail_read.load(Ordering::Relaxed) {
                if let Some(first) = output.first_mut() {
                    *first = 0xee;
                }
                return Err(CdSourceError::ReadFailed);
            }
            output.copy_from_slice(&self.data[offset..end]);
            Ok(())
        }

        fn visit_payload(
            &self,
            sector_bytes: usize,
            visitor: &mut dyn FnMut(&[u8]),
        ) -> Result<(), CdSourceError> {
            self.visits.fetch_add(1, Ordering::Relaxed);
            if self.fail_visit.load(Ordering::Relaxed) {
                return Err(CdSourceError::ReadFailed);
            }
            for bytes in self.data.chunks_exact(sector_bytes) {
                visitor(bytes);
            }
            Ok(())
        }
    }

    fn ppf1(records: &[(u32, &[u8])]) -> Vec<u8> {
        let mut patch = b"PPF10\0".to_vec();
        patch.resize(56, 0);
        for (offset, bytes) in records {
            patch.extend_from_slice(&offset.to_le_bytes());
            patch.push(bytes.len() as u8);
            patch.extend_from_slice(bytes);
        }
        patch
    }

    fn ppf3(records: &[(u64, &[u8])], block: &[u8]) -> Vec<u8> {
        let mut patch = b"PPF30\x02".to_vec();
        patch.resize(56, 0);
        patch.extend_from_slice(&[0, 1, 0, 0]);
        patch.extend_from_slice(block);
        for (offset, bytes) in records {
            patch.extend_from_slice(&offset.to_le_bytes());
            patch.push(bytes.len() as u8);
            patch.extend_from_slice(bytes);
        }
        patch
    }

    fn builder_bytes(builder: &PatchOverlayBuilder) -> Vec<u8> {
        let mut bytes = vec![0; builder.base.len()];
        builder.read_current(0, &mut bytes).unwrap();
        bytes
    }

    #[test]
    fn overlapping_patches_match_owned_bytes_for_every_read() {
        let original = (0..64).map(|byte| byte as u8).collect::<Vec<_>>();
        let base = Arc::new(SyntheticSource::new(original.clone()));
        let first = ppf1(&[(7, &[0xa0, 0xa1, 0xa2, 0xa3]), (20, &[0xb0, 0xb1])]);
        let second = ppf1(&[(5, &[0xc0, 0xc1, 0xc2, 0xc3]), (9, &[0xd0, 0xd1])]);
        let mut builder = PatchOverlayBuilder::new(base);
        assert_eq!(
            builder.apply_ppf(&first).unwrap(),
            PatchOverlayApply::Applied
        );
        assert_eq!(
            builder.apply_ppf(&second).unwrap(),
            PatchOverlayApply::Applied
        );
        let source = builder.finish();

        let mut owned = original;
        apply_ppf_patch(&mut owned, &first).unwrap();
        apply_ppf_patch(&mut owned, &second).unwrap();
        for offset in 0..=owned.len() {
            for len in 0..=owned.len() - offset {
                let mut actual = vec![0; len];
                source.read_exact_at(offset, &mut actual).unwrap();
                assert_eq!(actual, owned[offset..offset + len]);
            }
        }
        assert!(
            source
                .spans
                .windows(2)
                .all(|pair| pair[0].end() <= pair[1].start)
        );
    }

    #[test]
    fn read_failure_and_oversized_read_leave_destination_unchanged() {
        let base = Arc::new(SyntheticSource::new(vec![0; 2 * CD_RAW_SECTOR_BYTES]));
        let mut builder = PatchOverlayBuilder::new(base.clone());
        builder.apply_ppf(&ppf1(&[(0, &[1, 2])])).unwrap();
        let source = builder.finish();
        base.fail_read.store(true, Ordering::Relaxed);
        let mut output = [0x55; 4];
        assert_eq!(
            source.read_exact_at(0, &mut output),
            Err(CdSourceError::ReadFailed)
        );
        assert_eq!(output, [0x55; 4]);

        base.fail_read.store(false, Ordering::Relaxed);
        let mut oversized = vec![0x66; CD_RAW_SECTOR_BYTES + 1];
        assert_eq!(
            source.read_exact_at(0, &mut oversized),
            Err(CdSourceError::ReadFailed)
        );
        assert!(oversized.iter().all(|&byte| byte == 0x66));
    }

    #[test]
    fn canonical_disc_scan_delegates_once_and_matches_owned_identity() {
        let original = (0..2 * CD_USER_SECTOR_BYTES)
            .map(|index| (index as u8).wrapping_mul(29).wrapping_add(3))
            .collect::<Vec<_>>();
        let patch = ppf1(&[(17, &[8, 7, 6, 5]), (2060, &[4, 3, 2, 1])]);
        let base = Arc::new(SyntheticSource::new(original.clone()));
        let mut builder = PatchOverlayBuilder::new(base.clone());
        assert_eq!(
            builder.apply_ppf(&patch).unwrap(),
            PatchOverlayApply::Applied
        );
        let overlay_disc = CdDisc::new(vec![
            CdTrack::from_index1_unverified_source(
                1,
                4,
                None,
                0,
                CdTrackMode::Mode1_2048,
                builder.finish(),
            )
            .unwrap(),
        ])
        .unwrap();

        let mut owned = original;
        apply_ppf_patch(&mut owned, &patch).unwrap();
        let owned_disc = CdDisc::new(vec![
            CdTrack::from_index1_data(1, 4, None, 0, CdTrackMode::Mode1_2048, owned).unwrap(),
        ])
        .unwrap();
        assert_eq!(overlay_disc, owned_disc);
        assert_eq!(
            overlay_disc.track(1).unwrap().payload_hash(),
            owned_disc.track(1).unwrap().payload_hash()
        );
        assert_eq!(base.visits.load(Ordering::Relaxed), 1);
        assert_eq!(base.reads.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn mixed_track_ranges_match_owned_cross_track_check_write_and_identity() {
        let first = (0..7 * CD_USER_SECTOR_BYTES)
            .map(|index| index as u8)
            .collect::<Vec<_>>();
        let second = (0..10 * CD_RAW_SECTOR_BYTES)
            .map(|index| (index as u8).wrapping_mul(3))
            .collect::<Vec<_>>();
        let third = (0..2 * CD_RAW_SECTOR_BYTES)
            .map(|index| (index as u8).wrapping_mul(7))
            .collect::<Vec<_>>();
        let bases = [first.clone(), second.clone(), third.clone()]
            .into_iter()
            .map(SyntheticSource::new)
            .map(Arc::new)
            .collect::<Vec<_>>();
        let sources = bases
            .iter()
            .cloned()
            .map(|source| source as Arc<dyn CdTrackSource>)
            .collect::<Vec<_>>();
        let mut joined = [first.clone(), second.clone(), third.clone()].concat();
        let boundary = first.len() + second.len();
        let block = joined[0x9320..0x9320 + 1024].to_vec();
        let patch = ppf3(
            &[(boundary as u64 - 2, &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66])],
            &block,
        );
        let mut builder = PatchOverlayBuilder::for_tracks(&sources).unwrap();
        assert_eq!(
            builder.apply_ppf(&patch).unwrap(),
            PatchOverlayApply::Applied
        );
        let overlay_sources = builder.finish_tracks(sources).unwrap();
        let overlay_disc = CdDisc::new(vec![
            CdTrack::from_index1_unverified_source(
                1,
                4,
                None,
                0,
                CdTrackMode::Mode1_2048,
                overlay_sources[0].clone(),
            )
            .unwrap(),
            CdTrack::from_index1_unverified_source(
                2,
                4,
                None,
                7,
                CdTrackMode::Mode1_2352,
                overlay_sources[1].clone(),
            )
            .unwrap(),
            CdTrack::from_index1_unverified_source(
                3,
                0,
                None,
                17,
                CdTrackMode::Audio,
                overlay_sources[2].clone(),
            )
            .unwrap(),
        ])
        .unwrap();

        let mut owned = vec![first, second, third];
        apply_ppf_patch_segments(&mut owned, &patch).unwrap();
        joined[boundary - 2..boundary + 4].copy_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        assert_eq!(owned.concat(), joined);
        let owned_disc = CdDisc::new(vec![
            CdTrack::from_index1_data(1, 4, None, 0, CdTrackMode::Mode1_2048, owned[0].clone())
                .unwrap(),
            CdTrack::from_index1_data(2, 4, None, 7, CdTrackMode::Mode1_2352, owned[1].clone())
                .unwrap(),
            CdTrack::from_index1_data(3, 0, None, 17, CdTrackMode::Audio, owned[2].clone())
                .unwrap(),
        ])
        .unwrap();

        assert_eq!(overlay_disc, owned_disc);
        assert_eq!(overlay_disc.content_hash(), owned_disc.content_hash());
        assert_eq!(
            overlay_disc
                .tracks()
                .iter()
                .map(CdTrack::payload_hash)
                .collect::<Vec<_>>(),
            owned_disc
                .tracks()
                .iter()
                .map(CdTrack::payload_hash)
                .collect::<Vec<_>>()
        );
        assert!(
            bases
                .iter()
                .all(|base| base.visits.load(Ordering::Relaxed) == 1)
        );
    }

    #[test]
    fn concatenated_read_failure_keeps_destination_unchanged() {
        let first = Arc::new(SyntheticSource::new(vec![1; 8]));
        let second = Arc::new(SyntheticSource::new(vec![2; 8]));
        second.fail_read.store(true, Ordering::Relaxed);
        let sources = vec![
            first as Arc<dyn CdTrackSource>,
            second as Arc<dyn CdTrackSource>,
        ];
        let source = ConcatenatedTrackSource::new(&sources).unwrap();
        let mut output = [0x55; 8];

        assert_eq!(
            source.read_exact_at(4, &mut output),
            Err(CdSourceError::ReadFailed)
        );
        assert_eq!(output, [0x55; 8]);
    }

    #[test]
    fn source_failure_prevents_disc_construction() {
        let base = Arc::new(SyntheticSource::new(vec![0; CD_USER_SECTOR_BYTES]));
        let mut builder = PatchOverlayBuilder::new(base.clone());
        builder.apply_ppf(&ppf1(&[(0, &[1])])).unwrap();
        base.fail_visit.store(true, Ordering::Relaxed);
        assert!(matches!(
            CdDisc::new(vec![
                CdTrack::from_index1_unverified_source(
                    1,
                    4,
                    None,
                    0,
                    CdTrackMode::Mode1_2048,
                    builder.finish(),
                )
                .unwrap()
            ]),
            Err(CdDiscError::Source {
                source: CdSourceError::ReadFailed,
                ..
            })
        ));
    }

    #[test]
    fn limits_return_typed_fallback_without_mutating_builder() {
        let base = Arc::new(SyntheticSource::new(vec![0; 64]));
        let limits = PatchOverlayLimits::new(PpfPlanLimits::new(1024, 8, 8), 1, 3);
        let mut builder = PatchOverlayBuilder::with_limits(base, limits);
        let fallback = builder.apply_ppf(&ppf1(&[(0, &[1, 2, 3, 4])])).unwrap();
        assert_eq!(
            fallback,
            PatchOverlayApply::Fallback(PatchOverlayFallback::ReplacementBytes {
                bytes: 4,
                limit: 3
            })
        );
        assert!(builder.spans.is_empty());

        let limits = PatchOverlayLimits::new(PpfPlanLimits::new(1024, 8, 8), 1, 8);
        let base = Arc::new(SyntheticSource::new(vec![0; 64]));
        let mut builder = PatchOverlayBuilder::with_limits(base, limits);
        let fallback = builder.apply_ppf(&ppf1(&[(0, &[1]), (3, &[2])])).unwrap();
        assert_eq!(
            fallback,
            PatchOverlayApply::Fallback(PatchOverlayFallback::Spans { spans: 2, limit: 1 })
        );
        assert!(builder.spans.is_empty());
    }

    #[test]
    fn source_validation_observes_prior_overlay() {
        let original = vec![0; 0x9320 + 1024];
        let base = Arc::new(SyntheticSource::new(original.clone()));
        let first = ppf1(&[(0x9324, &[7])]);
        let mut expected = original;
        apply_ppf_patch(&mut expected, &first).unwrap();
        let second = ppf3(&[(0, &[9])], &expected[0x9320..0x9320 + 1024]);
        let mut builder = PatchOverlayBuilder::new(base);
        assert_eq!(
            builder.apply_ppf(&first).unwrap(),
            PatchOverlayApply::Applied
        );
        assert_eq!(
            builder.apply_ppf(&second).unwrap(),
            PatchOverlayApply::Applied
        );
    }

    #[test]
    fn patch_byte_fallback_preserves_existing_overlay() {
        let base = Arc::new(SyntheticSource::new(vec![0; 64]));
        let first = ppf1(&[(3, &[1])]);
        let limits = PatchOverlayLimits::new(PpfPlanLimits::new(first.len(), 8, 32), 8, 32);
        let mut builder = PatchOverlayBuilder::with_limits(base, limits);
        assert_eq!(
            builder.apply_ppf(&first).unwrap(),
            PatchOverlayApply::Applied
        );
        let before = builder_bytes(&builder);
        let replacement_bytes = builder.replacement_bytes;
        let oversized = ppf1(&[(8, &[2, 3])]);

        assert_eq!(
            builder.apply_ppf(&oversized).unwrap(),
            PatchOverlayApply::Fallback(PatchOverlayFallback::Ppf(PpfPlanFallback::PatchBytes {
                bytes: oversized.len(),
                limit: first.len()
            }))
        );
        assert_eq!(builder_bytes(&builder), before);
        assert_eq!(builder.replacement_bytes, replacement_bytes);
    }

    #[test]
    fn failed_source_validation_preserves_overlay_for_later_patch() {
        let original = vec![0; 0x9320 + 1024];
        let base = Arc::new(SyntheticSource::new(original.clone()));
        let first = ppf1(&[(0x9324, &[7]), (2, &[5])]);
        let mut expected = original;
        apply_ppf_patch(&mut expected, &first).unwrap();
        let valid = ppf3(&[(3, &[9])], &expected[0x9320..0x9320 + 1024]);
        let mut mismatched_block = expected[0x9320..0x9320 + 1024].to_vec();
        mismatched_block[0] ^= 1;
        let mismatched = ppf3(&[(3, &[8])], &mismatched_block);
        let mut builder = PatchOverlayBuilder::new(base.clone());
        assert_eq!(
            builder.apply_ppf(&first).unwrap(),
            PatchOverlayApply::Applied
        );
        let before = builder_bytes(&builder);
        let replacement_bytes = builder.replacement_bytes;

        assert!(builder.apply_ppf(&mismatched).is_err());
        assert_eq!(builder_bytes(&builder), before);
        assert_eq!(builder.replacement_bytes, replacement_bytes);

        base.fail_read.store(true, Ordering::Relaxed);
        assert!(builder.apply_ppf(&valid).is_err());
        base.fail_read.store(false, Ordering::Relaxed);
        assert_eq!(builder_bytes(&builder), before);
        assert_eq!(builder.replacement_bytes, replacement_bytes);

        assert_eq!(
            builder.apply_ppf(&valid).unwrap(),
            PatchOverlayApply::Applied
        );
        assert_eq!(builder_bytes(&builder)[3], 9);
    }
}
