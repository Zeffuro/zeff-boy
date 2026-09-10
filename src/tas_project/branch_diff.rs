use std::{
    cmp::Ordering,
    ops::Range,
    sync::atomic::{AtomicBool, Ordering as AtomicOrdering},
};

use anyhow::{Result, bail};
use zeff_emu_common::replay::ReplayEvent;

use super::{
    TasBranch, TasDigest, TasInputFrame, TasInputSpan, TasProject,
    identity::hash_complete_branch_parts,
};

pub const MAX_BRANCH_DIFF_INPUT_SPANS_SCANNED: usize = 200_000;
pub const MAX_BRANCH_DIFF_EVENTS_SCANNED: usize = 200_000;
pub const MAX_BRANCH_DIFF_RETAINED_HUNKS: usize = 128;
pub(crate) const MAX_BACKGROUND_BRANCH_DIFF_INPUT_SPANS: usize =
    MAX_BRANCH_DIFF_INPUT_SPANS_SCANNED;
pub(crate) const MAX_BACKGROUND_BRANCH_DIFF_EVENTS: usize = MAX_BRANCH_DIFF_EVENTS_SCANNED;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TasBranchDiffLimits {
    pub max_input_spans_scanned: usize,
    pub max_events_scanned: usize,
    pub max_input_hunks: usize,
    pub max_event_hunks: usize,
}

impl Default for TasBranchDiffLimits {
    fn default() -> Self {
        Self {
            max_input_spans_scanned: MAX_BRANCH_DIFF_INPUT_SPANS_SCANNED,
            max_events_scanned: MAX_BRANCH_DIFF_EVENTS_SCANNED,
            max_input_hunks: MAX_BRANCH_DIFF_RETAINED_HUNKS,
            max_event_hunks: MAX_BRANCH_DIFF_RETAINED_HUNKS,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TasBranchDiffSide {
    Source,
    Target,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TasInputDiffHunk {
    pub start: u64,
    pub length: u64,
    pub source_input: TasInputFrame,
    pub target_input: TasInputFrame,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TasTimelineTailDiff {
    pub longer_side: TasBranchDiffSide,
    pub start: u64,
    pub length: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TasEventDiffKind {
    Changed,
    SourceOnly,
    TargetOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TasEventDiffHunk {
    pub kind: TasEventDiffKind,
    pub source_event_indices: Range<usize>,
    pub target_event_indices: Range<usize>,
    pub first_frame: u64,
    pub last_frame: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TasBranchDiff {
    pub source_movie_sha256: TasDigest,
    pub target_movie_sha256: TasDigest,
    pub source_frame_count: u64,
    pub target_frame_count: u64,
    pub input_hunks: Vec<TasInputDiffHunk>,
    pub timeline_tail: Option<TasTimelineTailDiff>,
    pub event_hunks: Vec<TasEventDiffHunk>,
    pub omitted_input_hunks: usize,
    pub omitted_event_hunks: usize,
}

#[derive(Clone, Debug)]
struct TasBranchDiffTimeline {
    frame_count: u64,
    input_spans: Vec<TasInputSpan>,
    events: Vec<ReplayEvent>,
}

#[derive(Clone, Debug)]
pub(crate) struct TasBranchDiffSnapshot {
    sync_identity_sha256: TasDigest,
    source: TasBranchDiffTimeline,
    target: Option<TasBranchDiffTimeline>,
}

impl TasBranchDiff {
    pub fn is_truncated(&self) -> bool {
        self.omitted_input_hunks != 0 || self.omitted_event_hunks != 0
    }

    pub fn is_identical(&self) -> bool {
        self.source_movie_sha256 == self.target_movie_sha256
            && self.input_hunks.is_empty()
            && self.timeline_tail.is_none()
            && self.event_hunks.is_empty()
            && !self.is_truncated()
    }
}

impl TasBranchDiffSnapshot {
    pub(crate) fn diff(&self, cancellation: &AtomicBool) -> Result<Option<TasBranchDiff>> {
        let target = self.target.as_ref().unwrap_or(&self.source);
        match diff_branch_timelines(
            self.sync_identity_sha256,
            timeline_view(&self.source),
            timeline_view(target),
            self.target.is_none(),
            TasBranchDiffLimits::default(),
            Some(cancellation),
        ) {
            Ok(diff) => Ok(diff),
            Err(_error) if cancellation.load(AtomicOrdering::Acquire) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

impl TasProject {
    // Equal-key event runs stay intact because their stable order is significant.
    pub fn diff_branches(
        &self,
        source_branch_id: &str,
        target_branch_id: &str,
        limits: TasBranchDiffLimits,
    ) -> Result<TasBranchDiff> {
        validate_limits(limits)?;
        let source = self
            .branch(source_branch_id)
            .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {source_branch_id:?}"))?;
        let target = self
            .branch(target_branch_id)
            .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {target_branch_id:?}"))?;
        diff_branch_timelines(
            self.sync_identity_sha256_from_validated()?,
            branch_view(source),
            branch_view(target),
            source_branch_id == target_branch_id,
            limits,
            None,
        )?
        .ok_or_else(|| anyhow::anyhow!("synchronous TAS branch diff was cancelled"))
    }

    pub(crate) fn branch_diff_snapshot_from_validated(
        &self,
        source_branch_id: &str,
        target_branch_id: &str,
    ) -> Result<TasBranchDiffSnapshot> {
        let source = self
            .branch(source_branch_id)
            .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {source_branch_id:?}"))?;
        let target = self
            .branch(target_branch_id)
            .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {target_branch_id:?}"))?;
        enforce_background_snapshot_limits(source, target)?;
        let source = timeline_from_branch(source);
        let target = (source_branch_id != target_branch_id).then(|| timeline_from_branch(target));
        Ok(TasBranchDiffSnapshot {
            sync_identity_sha256: self.sync_identity_sha256_from_validated()?,
            source,
            target,
        })
    }
}

#[derive(Clone, Copy)]
struct TasBranchDiffTimelineView<'a> {
    frame_count: u64,
    input_spans: &'a [TasInputSpan],
    events: &'a [ReplayEvent],
}

fn branch_view(branch: &TasBranch) -> TasBranchDiffTimelineView<'_> {
    TasBranchDiffTimelineView {
        frame_count: branch.frame_count,
        input_spans: &branch.input_spans,
        events: &branch.events,
    }
}

fn timeline_from_branch(branch: &TasBranch) -> TasBranchDiffTimeline {
    TasBranchDiffTimeline {
        frame_count: branch.frame_count,
        input_spans: branch.input_spans.clone(),
        events: branch.events.clone(),
    }
}

fn timeline_view(timeline: &TasBranchDiffTimeline) -> TasBranchDiffTimelineView<'_> {
    TasBranchDiffTimelineView {
        frame_count: timeline.frame_count,
        input_spans: &timeline.input_spans,
        events: &timeline.events,
    }
}

fn diff_branch_timelines(
    sync_identity_sha256: TasDigest,
    source: TasBranchDiffTimelineView<'_>,
    target: TasBranchDiffTimelineView<'_>,
    source_equals_target: bool,
    limits: TasBranchDiffLimits,
    cancellation: Option<&AtomicBool>,
) -> Result<Option<TasBranchDiff>> {
    validate_limits(limits)?;
    check_cancelled(cancellation)?;
    let common_frame_count = source.frame_count.min(target.frame_count);
    enforce_scan_limits(source, target, common_frame_count, limits, cancellation)?;

    let source_movie_sha256 = hash_complete_branch_parts(
        sync_identity_sha256,
        source.frame_count,
        source.input_spans,
        source.events,
    )?;
    check_cancelled(cancellation)?;
    let target_movie_sha256 = if source_equals_target {
        source_movie_sha256
    } else {
        hash_complete_branch_parts(
            sync_identity_sha256,
            target.frame_count,
            target.input_spans,
            target.events,
        )?
    };
    check_cancelled(cancellation)?;
    let (input_hunks, omitted_input_hunks) = diff_inputs(
        source,
        target,
        common_frame_count,
        limits.max_input_hunks,
        cancellation,
    )?;
    let timeline_tail = timeline_tail(source.frame_count, target.frame_count);
    let (event_hunks, omitted_event_hunks) = diff_events(
        source.events,
        target.events,
        limits.max_event_hunks,
        cancellation,
    )?;
    check_cancelled(cancellation)?;

    Ok(Some(TasBranchDiff {
        source_movie_sha256,
        target_movie_sha256,
        source_frame_count: source.frame_count,
        target_frame_count: target.frame_count,
        input_hunks,
        timeline_tail,
        event_hunks,
        omitted_input_hunks,
        omitted_event_hunks,
    }))
}

fn validate_limits(limits: TasBranchDiffLimits) -> Result<()> {
    if limits.max_input_spans_scanned > MAX_BRANCH_DIFF_INPUT_SPANS_SCANNED {
        bail!(
            "TAS branch diff input scan limit exceeds the hard maximum of {MAX_BRANCH_DIFF_INPUT_SPANS_SCANNED} spans"
        );
    }
    if limits.max_events_scanned > MAX_BRANCH_DIFF_EVENTS_SCANNED {
        bail!(
            "TAS branch diff event scan limit exceeds the hard maximum of {MAX_BRANCH_DIFF_EVENTS_SCANNED} events"
        );
    }
    if limits.max_input_hunks > MAX_BRANCH_DIFF_RETAINED_HUNKS
        || limits.max_event_hunks > MAX_BRANCH_DIFF_RETAINED_HUNKS
    {
        bail!(
            "TAS branch diff hunk limit exceeds the hard maximum of {MAX_BRANCH_DIFF_RETAINED_HUNKS} per domain"
        );
    }
    Ok(())
}

fn enforce_scan_limits(
    source: TasBranchDiffTimelineView<'_>,
    target: TasBranchDiffTimelineView<'_>,
    common_frame_count: u64,
    limits: TasBranchDiffLimits,
    cancellation: Option<&AtomicBool>,
) -> Result<()> {
    check_cancelled(cancellation)?;
    let source_input_spans = source
        .input_spans
        .partition_point(|span| span.start < common_frame_count);
    let target_input_spans = target
        .input_spans
        .partition_point(|span| span.start < common_frame_count);
    let input_spans = source_input_spans
        .checked_add(target_input_spans)
        .ok_or_else(|| anyhow::anyhow!("TAS branch diff input scan size overflows"))?;
    if input_spans > limits.max_input_spans_scanned {
        bail!(
            "TAS branch diff requires scanning {input_spans} input spans, above the configured limit of {}",
            limits.max_input_spans_scanned
        );
    }

    let events = source
        .events
        .len()
        .checked_add(target.events.len())
        .ok_or_else(|| anyhow::anyhow!("TAS branch diff event scan size overflows"))?;
    if events > limits.max_events_scanned {
        bail!(
            "TAS branch diff requires scanning {events} events, above the configured limit of {}",
            limits.max_events_scanned
        );
    }
    Ok(())
}

fn enforce_background_snapshot_limits(source: &TasBranch, target: &TasBranch) -> Result<()> {
    let input_spans = source
        .input_spans
        .len()
        .checked_add(target.input_spans.len())
        .ok_or_else(|| anyhow::anyhow!("TAS branch diff snapshot input span count overflows"))?;
    if input_spans > MAX_BACKGROUND_BRANCH_DIFF_INPUT_SPANS {
        bail!(
            "TAS branch diff complete snapshot requires {input_spans} input spans, above the limit of {MAX_BACKGROUND_BRANCH_DIFF_INPUT_SPANS}"
        );
    }
    let events = source
        .events
        .len()
        .checked_add(target.events.len())
        .ok_or_else(|| anyhow::anyhow!("TAS branch diff snapshot event count overflows"))?;
    if events > MAX_BACKGROUND_BRANCH_DIFF_EVENTS {
        bail!(
            "TAS branch diff complete snapshot requires {events} events, above the limit of {MAX_BACKGROUND_BRANCH_DIFF_EVENTS}"
        );
    }
    Ok(())
}

fn timeline_tail(source_frames: u64, target_frames: u64) -> Option<TasTimelineTailDiff> {
    match source_frames.cmp(&target_frames) {
        Ordering::Greater => Some(TasTimelineTailDiff {
            longer_side: TasBranchDiffSide::Source,
            start: target_frames,
            length: source_frames - target_frames,
        }),
        Ordering::Less => Some(TasTimelineTailDiff {
            longer_side: TasBranchDiffSide::Target,
            start: source_frames,
            length: target_frames - source_frames,
        }),
        Ordering::Equal => None,
    }
}

fn diff_inputs(
    source: TasBranchDiffTimelineView<'_>,
    target: TasBranchDiffTimelineView<'_>,
    frame_count: u64,
    max_hunks: usize,
    cancellation: Option<&AtomicBool>,
) -> Result<(Vec<TasInputDiffHunk>, usize)> {
    let mut hunks = Vec::with_capacity(max_hunks);
    let mut omitted = 0usize;
    let mut pending = None;
    let mut source_index = 0usize;
    let mut target_index = 0usize;
    let mut cursor = 0u64;
    let mut segments = 0usize;
    while cursor < frame_count {
        if segments.is_multiple_of(256) {
            check_cancelled(cancellation)?;
        }
        segments += 1;
        let (source_input, source_end) =
            input_segment(source.input_spans, &mut source_index, cursor, frame_count);
        let (target_input, target_end) =
            input_segment(target.input_spans, &mut target_index, cursor, frame_count);
        let end = source_end.min(target_end);
        debug_assert!(end > cursor);
        if source_input == target_input {
            flush_input_hunk(&mut pending, &mut hunks, &mut omitted, max_hunks);
        } else {
            queue_input_hunk(
                &mut pending,
                TasInputDiffHunk {
                    start: cursor,
                    length: end - cursor,
                    source_input,
                    target_input,
                },
                &mut hunks,
                &mut omitted,
                max_hunks,
            );
        }
        cursor = end;
    }
    flush_input_hunk(&mut pending, &mut hunks, &mut omitted, max_hunks);
    Ok((hunks, omitted))
}

fn input_segment(
    spans: &[TasInputSpan],
    index: &mut usize,
    cursor: u64,
    frame_count: u64,
) -> (TasInputFrame, u64) {
    while spans
        .get(*index)
        .is_some_and(|span| span.start + span.length <= cursor)
    {
        *index += 1;
    }
    let Some(span) = spans.get(*index) else {
        return (TasInputFrame::default(), frame_count);
    };
    if span.start <= cursor {
        (span.input, (span.start + span.length).min(frame_count))
    } else {
        (TasInputFrame::default(), span.start.min(frame_count))
    }
}

fn queue_input_hunk(
    pending: &mut Option<TasInputDiffHunk>,
    next: TasInputDiffHunk,
    hunks: &mut Vec<TasInputDiffHunk>,
    omitted: &mut usize,
    max_hunks: usize,
) {
    if let Some(previous) = pending
        && previous.start + previous.length == next.start
        && previous.source_input == next.source_input
        && previous.target_input == next.target_input
    {
        previous.length += next.length;
        return;
    }
    flush_input_hunk(pending, hunks, omitted, max_hunks);
    *pending = Some(next);
}

fn flush_input_hunk(
    pending: &mut Option<TasInputDiffHunk>,
    hunks: &mut Vec<TasInputDiffHunk>,
    omitted: &mut usize,
    max_hunks: usize,
) {
    let Some(hunk) = pending.take() else {
        return;
    };
    if hunks.len() < max_hunks {
        hunks.push(hunk);
    } else {
        *omitted = omitted.saturating_add(1);
    }
}

fn diff_events(
    source: &[ReplayEvent],
    target: &[ReplayEvent],
    max_hunks: usize,
    cancellation: Option<&AtomicBool>,
) -> Result<(Vec<TasEventDiffHunk>, usize)> {
    let mut hunks = Vec::with_capacity(max_hunks);
    let mut omitted = 0usize;
    let mut pending = None;
    let (mut source_index, mut target_index) = (0usize, 0usize);
    let mut groups = 0usize;
    while source_index < source.len() || target_index < target.len() {
        if groups.is_multiple_of(256) {
            check_cancelled(cancellation)?;
        }
        groups += 1;
        let ordering = match (source.get(source_index), target.get(target_index)) {
            (Some(left), Some(right)) => left.canonical_cmp(right),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => break,
        };
        match ordering {
            Ordering::Less => {
                let end = event_group_end(source, source_index, cancellation)?;
                queue_event_hunk(
                    &mut pending,
                    event_hunk(
                        TasEventDiffKind::SourceOnly,
                        source_index..end,
                        target_index..target_index,
                        source[source_index].frame(),
                    ),
                    &mut hunks,
                    &mut omitted,
                    max_hunks,
                );
                source_index = end;
            }
            Ordering::Greater => {
                let end = event_group_end(target, target_index, cancellation)?;
                queue_event_hunk(
                    &mut pending,
                    event_hunk(
                        TasEventDiffKind::TargetOnly,
                        source_index..source_index,
                        target_index..end,
                        target[target_index].frame(),
                    ),
                    &mut hunks,
                    &mut omitted,
                    max_hunks,
                );
                target_index = end;
            }
            Ordering::Equal => {
                let source_end = event_group_end(source, source_index, cancellation)?;
                let target_end = event_group_end(target, target_index, cancellation)?;
                if event_runs_equal(
                    &source[source_index..source_end],
                    &target[target_index..target_end],
                    cancellation,
                )? {
                    flush_event_hunk(&mut pending, &mut hunks, &mut omitted, max_hunks);
                } else {
                    queue_event_hunk(
                        &mut pending,
                        event_hunk(
                            TasEventDiffKind::Changed,
                            source_index..source_end,
                            target_index..target_end,
                            source[source_index].frame(),
                        ),
                        &mut hunks,
                        &mut omitted,
                        max_hunks,
                    );
                }
                source_index = source_end;
                target_index = target_end;
            }
        }
    }
    flush_event_hunk(&mut pending, &mut hunks, &mut omitted, max_hunks);
    Ok((hunks, omitted))
}

fn event_group_end(
    events: &[ReplayEvent],
    start: usize,
    cancellation: Option<&AtomicBool>,
) -> Result<usize> {
    let first = &events[start];
    let mut end = start + 1;
    while events
        .get(end)
        .is_some_and(|event| first.canonical_cmp(event).is_eq())
    {
        if (end - start).is_multiple_of(256) {
            check_cancelled(cancellation)?;
        }
        end += 1;
    }
    Ok(end)
}

fn event_runs_equal(
    source: &[ReplayEvent],
    target: &[ReplayEvent],
    cancellation: Option<&AtomicBool>,
) -> Result<bool> {
    if source.len() != target.len() {
        return Ok(false);
    }
    for (index, (source, target)) in source.iter().zip(target).enumerate() {
        if index.is_multiple_of(256) {
            check_cancelled(cancellation)?;
        }
        if source != target {
            return Ok(false);
        }
    }
    Ok(true)
}

fn check_cancelled(cancellation: Option<&AtomicBool>) -> Result<()> {
    if cancellation.is_some_and(|cancellation| cancellation.load(AtomicOrdering::Acquire)) {
        bail!("TAS branch diff was cancelled");
    }
    Ok(())
}

fn event_hunk(
    kind: TasEventDiffKind,
    source_event_indices: Range<usize>,
    target_event_indices: Range<usize>,
    frame: u64,
) -> TasEventDiffHunk {
    TasEventDiffHunk {
        kind,
        source_event_indices,
        target_event_indices,
        first_frame: frame,
        last_frame: frame,
    }
}

fn queue_event_hunk(
    pending: &mut Option<TasEventDiffHunk>,
    next: TasEventDiffHunk,
    hunks: &mut Vec<TasEventDiffHunk>,
    omitted: &mut usize,
    max_hunks: usize,
) {
    if let Some(previous) = pending
        && previous.kind == next.kind
        && previous.source_event_indices.end == next.source_event_indices.start
        && previous.target_event_indices.end == next.target_event_indices.start
    {
        previous.source_event_indices.end = next.source_event_indices.end;
        previous.target_event_indices.end = next.target_event_indices.end;
        previous.last_frame = next.last_frame;
        return;
    }
    flush_event_hunk(pending, hunks, omitted, max_hunks);
    *pending = Some(next);
}

fn flush_event_hunk(
    pending: &mut Option<TasEventDiffHunk>,
    hunks: &mut Vec<TasEventDiffHunk>,
    omitted: &mut usize,
    max_hunks: usize,
) {
    let Some(hunk) = pending.take() else {
        return;
    };
    if hunks.len() < max_hunks {
        hunks.push(hunk);
    } else {
        *omitted = omitted.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_event_group_and_equality_loops_check_cancellation() {
        let events = std::iter::repeat_n(ReplayEvent::FdsDiskSide { frame: 1, side: 0 }, 257)
            .collect::<Vec<_>>();
        let cancellation = AtomicBool::new(true);
        assert!(event_group_end(&events, 0, Some(&cancellation)).is_err());
        assert!(event_runs_equal(&events, &events, Some(&cancellation)).is_err());
    }
}
