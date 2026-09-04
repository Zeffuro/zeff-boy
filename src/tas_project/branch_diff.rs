use std::{cmp::Ordering, ops::Range};

use anyhow::{Result, bail};
use zeff_emu_common::replay::ReplayEvent;

use super::{TasBranch, TasDigest, TasInputFrame, TasInputSpan, TasProject};

pub const MAX_BRANCH_DIFF_INPUT_SPANS_SCANNED: usize = 200_000;
pub const MAX_BRANCH_DIFF_EVENTS_SCANNED: usize = 200_000;
pub const MAX_BRANCH_DIFF_RETAINED_HUNKS: usize = 128;

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
        let common_frame_count = source.frame_count.min(target.frame_count);
        enforce_scan_limits(source, target, common_frame_count, limits)?;

        let source_movie_sha256 = self.branch_movie_sha256_from_validated(source)?;
        let target_movie_sha256 = if source_branch_id == target_branch_id {
            source_movie_sha256
        } else {
            self.branch_movie_sha256_from_validated(target)?
        };
        let (input_hunks, omitted_input_hunks) =
            diff_inputs(source, target, common_frame_count, limits.max_input_hunks);
        let timeline_tail = timeline_tail(source.frame_count, target.frame_count);
        let (event_hunks, omitted_event_hunks) =
            diff_events(&source.events, &target.events, limits.max_event_hunks);

        Ok(TasBranchDiff {
            source_movie_sha256,
            target_movie_sha256,
            source_frame_count: source.frame_count,
            target_frame_count: target.frame_count,
            input_hunks,
            timeline_tail,
            event_hunks,
            omitted_input_hunks,
            omitted_event_hunks,
        })
    }
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
    source: &TasBranch,
    target: &TasBranch,
    common_frame_count: u64,
    limits: TasBranchDiffLimits,
) -> Result<()> {
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
    source: &TasBranch,
    target: &TasBranch,
    frame_count: u64,
    max_hunks: usize,
) -> (Vec<TasInputDiffHunk>, usize) {
    let mut hunks = Vec::with_capacity(max_hunks);
    let mut omitted = 0usize;
    let mut pending = None;
    let mut source_index = 0usize;
    let mut target_index = 0usize;
    let mut cursor = 0u64;
    while cursor < frame_count {
        let (source_input, source_end) =
            input_segment(&source.input_spans, &mut source_index, cursor, frame_count);
        let (target_input, target_end) =
            input_segment(&target.input_spans, &mut target_index, cursor, frame_count);
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
    (hunks, omitted)
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
) -> (Vec<TasEventDiffHunk>, usize) {
    let mut hunks = Vec::with_capacity(max_hunks);
    let mut omitted = 0usize;
    let mut pending = None;
    let (mut source_index, mut target_index) = (0usize, 0usize);
    while source_index < source.len() || target_index < target.len() {
        let ordering = match (source.get(source_index), target.get(target_index)) {
            (Some(left), Some(right)) => left.canonical_cmp(right),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => break,
        };
        match ordering {
            Ordering::Less => {
                let end = event_group_end(source, source_index);
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
                let end = event_group_end(target, target_index);
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
                let source_end = event_group_end(source, source_index);
                let target_end = event_group_end(target, target_index);
                if source[source_index..source_end] == target[target_index..target_end] {
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
    (hunks, omitted)
}

fn event_group_end(events: &[ReplayEvent], start: usize) -> usize {
    let first = &events[start];
    let mut end = start + 1;
    while events
        .get(end)
        .is_some_and(|event| first.canonical_cmp(event).is_eq())
    {
        end += 1;
    }
    end
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
