use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use zeff_emu_common::replay::{
    ReplayEvent, decode_replay_event_stream, encode_replay_event_stream,
};

use super::identity::hash_branch;
use super::input_pattern::{TasInputPattern, replace_branch_input_pattern};
use super::model::{
    TasAnnotation, TasBranch, TasBranchOrigin, TasDigest, TasInputFrame, TasInputSpan, TasMarker,
    TasProject,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TasEditOutcome {
    pub changed: bool,
    pub edit_generation: u64,
    pub rerecord_count: u64,
    pub branch_impacts: Vec<TasBranchEditImpact>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TasBranchEditImpact {
    pub branch_id: String,
    pub kind: TasBranchEditImpactKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TasBranchEditImpactKind {
    Created { fork_cursor: u64 },
    Modified { earliest_cursor: u64 },
}

pub struct TasProjectEdit<'a> {
    project: &'a mut TasProject,
    timeline_earliest_cursors: BTreeMap<String, u64>,
}

impl TasProject {
    pub fn edit_transaction<F>(&mut self, edit: F) -> Result<TasEditOutcome>
    where
        F: FnOnce(&mut TasProjectEdit<'_>) -> Result<()>,
    {
        self.validate()?;
        let sync_identity = self.sync_identity_sha256()?;
        let before_hashes = branch_hashes(self, sync_identity)?;
        let before_ids = before_hashes.keys().cloned().collect::<BTreeSet<_>>();

        let mut candidate = self.clone();
        let timeline_earliest_cursors = {
            let mut editor = TasProjectEdit {
                project: &mut candidate,
                timeline_earliest_cursors: BTreeMap::new(),
            };
            edit(&mut editor)?;
            editor.timeline_earliest_cursors
        };

        let changed = candidate != *self;
        let after_hashes = branch_hashes(&candidate, sync_identity)?;
        let after_ids = after_hashes.keys().cloned().collect::<BTreeSet<_>>();
        let created_ids = after_ids
            .difference(&before_ids)
            .cloned()
            .collect::<BTreeSet<_>>();
        let changed_existing_ids = before_ids
            .intersection(&after_ids)
            .filter(|id| before_hashes.get(*id) != after_hashes.get(*id))
            .cloned()
            .collect::<BTreeSet<_>>();
        let divergent_fork_created = created_ids.iter().any(|branch_id| {
            let branch = candidate
                .branch(branch_id)
                .expect("created branch was collected from the candidate project");
            branch.parent.as_ref().is_some_and(|parent| {
                after_hashes.get(branch_id) != Some(&parent.branch_movie_sha256)
            })
        });

        for branch in &mut candidate.branches {
            if created_ids.contains(&branch.id) || changed_existing_ids.contains(&branch.id) {
                branch.verification = None;
            }
        }
        candidate.validate()?;

        if changed {
            candidate.edit_generation = candidate
                .edit_generation
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("TAS edit generation overflow"))?;
        }
        if !changed_existing_ids.is_empty() || divergent_fork_created {
            candidate.rerecord_count = candidate
                .rerecord_count
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("TAS rerecord count overflow"))?;
        }
        candidate.validate()?;

        let mut branch_impacts = Vec::with_capacity(created_ids.len() + changed_existing_ids.len());
        for branch_id in after_ids {
            let kind = if created_ids.contains(&branch_id) {
                let fork_cursor = candidate
                    .branch(&branch_id)
                    .and_then(|branch| branch.parent.as_ref())
                    .map_or(0, |parent| parent.fork_cursor);
                Some(TasBranchEditImpactKind::Created { fork_cursor })
            } else if changed_existing_ids.contains(&branch_id) {
                let before = self
                    .branch(&branch_id)
                    .expect("existing branch was collected from the original project");
                let after = candidate
                    .branch(&branch_id)
                    .expect("existing branch was collected from the candidate project");
                let earliest_cursor = earliest_branch_movie_difference(before, after).unwrap_or(0);
                Some(TasBranchEditImpactKind::Modified {
                    earliest_cursor: timeline_earliest_cursors
                        .get(&branch_id)
                        .copied()
                        .map_or(earliest_cursor, |timeline| timeline.min(earliest_cursor)),
                })
            } else {
                None
            };
            if let Some(kind) = kind {
                branch_impacts.push(TasBranchEditImpact { branch_id, kind });
            }
        }

        let outcome = TasEditOutcome {
            changed,
            edit_generation: candidate.edit_generation,
            rerecord_count: candidate.rerecord_count,
            branch_impacts,
        };
        *self = candidate;
        Ok(outcome)
    }
}

impl TasProjectEdit<'_> {
    pub fn fork_branch(
        &mut self,
        source_branch_id: &str,
        fork_cursor: u64,
        new_branch_id: impl Into<String>,
        name: impl Into<String>,
    ) -> Result<()> {
        let new_branch_id = new_branch_id.into();
        if self.project.branch(&new_branch_id).is_some() {
            bail!("duplicate TAS branch ID {new_branch_id:?}");
        }
        let source = self
            .project
            .branch(source_branch_id)
            .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {source_branch_id:?}"))?;
        if fork_cursor > source.frame_count {
            bail!("TAS fork cursor is past branch end");
        }
        let source_hash = self.project.branch_movie_sha256(source_branch_id)?;
        let mut branch = source.clone();
        branch.id = new_branch_id;
        branch.name = name.into();
        branch.comment.clear();
        branch.parent = Some(TasBranchOrigin {
            branch_id: source_branch_id.to_owned(),
            branch_movie_sha256: source_hash,
            fork_cursor,
        });
        branch.verification = None;
        self.project.branches.push(branch);
        Ok(())
    }

    pub fn set_active_branch(&mut self, branch_id: &str) -> Result<()> {
        if self.project.branch(branch_id).is_none() {
            bail!("unknown TAS branch {branch_id:?}");
        }
        self.project.active_branch_id = branch_id.to_owned();
        Ok(())
    }

    pub fn set_project_comment(&mut self, comment: impl Into<String>) {
        self.project.project_comment = comment.into();
    }

    pub fn rename_branch(&mut self, branch_id: &str, name: impl Into<String>) -> Result<()> {
        self.branch_mut(branch_id)?.name = name.into();
        Ok(())
    }

    pub fn set_branch_comment(
        &mut self,
        branch_id: &str,
        comment: impl Into<String>,
    ) -> Result<()> {
        self.branch_mut(branch_id)?.comment = comment.into();
        Ok(())
    }

    pub fn replace_markers(&mut self, markers: Vec<TasMarker>) {
        self.project.markers = markers;
    }

    pub fn replace_annotations(&mut self, annotations: Vec<TasAnnotation>) {
        self.project.annotations = annotations;
    }

    pub fn insert_camera_asset(&mut self, bytes: Vec<u8>) -> TasDigest {
        let digest = TasDigest::from_bytes(&bytes);
        self.project.assets.insert(digest, bytes);
        digest
    }

    pub fn remove_camera_asset(&mut self, digest: TasDigest) -> bool {
        self.project.assets.remove(&digest).is_some()
    }

    pub fn insert_frames(&mut self, branch_id: &str, cursor: u64, count: u64) -> Result<()> {
        if count == 0 {
            bail!("TAS frame insertion cannot be empty");
        }
        let branch_index = self.branch_index(branch_id)?;
        let old_frame_count = self.project.branches[branch_index].frame_count;
        if cursor > old_frame_count {
            bail!("TAS frame insertion cursor is past branch end");
        }
        let new_frame_count = old_frame_count
            .checked_add(count)
            .ok_or_else(|| anyhow::anyhow!("TAS frame insertion overflows"))?;
        if new_frame_count > super::model::MAX_PROJECT_FRAMES {
            bail!(
                "TAS frame insertion exceeds the {} frame limit",
                super::model::MAX_PROJECT_FRAMES
            );
        }

        {
            let branch = &mut self.project.branches[branch_index];
            branch.input_spans = insert_input_frames(&branch.input_spans, cursor, count)?;
            for event in &mut branch.events {
                if event.frame() >= cursor {
                    let frame = event
                        .frame()
                        .checked_add(count)
                        .ok_or_else(|| anyhow::anyhow!("TAS event frame insertion overflows"))?;
                    set_event_frame(event, frame);
                }
            }
            branch.frame_count = new_frame_count;
        }

        for marker in self
            .project
            .markers
            .iter_mut()
            .filter(|marker| marker.branch_id == branch_id && marker.cursor >= cursor)
        {
            marker.cursor = marker
                .cursor
                .checked_add(count)
                .ok_or_else(|| anyhow::anyhow!("TAS marker cursor insertion overflows"))?;
        }
        for annotation in self
            .project
            .annotations
            .iter_mut()
            .filter(|annotation| annotation.branch_id == branch_id)
        {
            let end = annotation
                .start
                .checked_add(annotation.length)
                .ok_or_else(|| anyhow::anyhow!("TAS annotation range overflows"))?;
            if annotation.start >= cursor {
                annotation.start = annotation
                    .start
                    .checked_add(count)
                    .ok_or_else(|| anyhow::anyhow!("TAS annotation insertion overflows"))?;
            } else if end > cursor {
                annotation.length = annotation
                    .length
                    .checked_add(count)
                    .ok_or_else(|| anyhow::anyhow!("TAS annotation insertion overflows"))?;
            }
        }
        self.record_timeline_impact(branch_id, cursor);
        Ok(())
    }

    pub fn delete_frames(&mut self, branch_id: &str, start: u64, count: u64) -> Result<()> {
        if count == 0 {
            bail!("TAS frame deletion cannot be empty");
        }
        let end = start
            .checked_add(count)
            .ok_or_else(|| anyhow::anyhow!("TAS frame deletion range overflows"))?;
        let branch_index = self.branch_index(branch_id)?;
        let old_frame_count = self.project.branches[branch_index].frame_count;
        if end > old_frame_count {
            bail!("TAS frame deletion extends past branch end");
        }

        {
            let branch = &mut self.project.branches[branch_index];
            branch.input_spans = delete_input_frames(&branch.input_spans, start, end, count)?;
            let mut events = Vec::with_capacity(branch.events.len());
            for mut event in branch.events.drain(..) {
                let frame = event.frame();
                if (start..end).contains(&frame) {
                    continue;
                }
                if frame >= end {
                    let rebased = frame
                        .checked_sub(count)
                        .ok_or_else(|| anyhow::anyhow!("TAS event frame deletion underflows"))?;
                    set_event_frame(&mut event, rebased);
                }
                events.push(event);
            }
            branch.events = events;
            branch.frame_count = old_frame_count
                .checked_sub(count)
                .ok_or_else(|| anyhow::anyhow!("TAS frame count deletion underflows"))?;
        }

        for marker in self
            .project
            .markers
            .iter_mut()
            .filter(|marker| marker.branch_id == branch_id)
        {
            marker.cursor = delete_cursor(marker.cursor, start, end, count)?;
        }
        let mut annotations = Vec::with_capacity(self.project.annotations.len());
        for mut annotation in self.project.annotations.drain(..) {
            if annotation.branch_id != branch_id {
                annotations.push(annotation);
                continue;
            }
            let annotation_end = annotation
                .start
                .checked_add(annotation.length)
                .ok_or_else(|| anyhow::anyhow!("TAS annotation range overflows"))?;
            let rebased_start = delete_cursor(annotation.start, start, end, count)?;
            let rebased_end = delete_cursor(annotation_end, start, end, count)?;
            if rebased_start == rebased_end {
                continue;
            }
            annotation.start = rebased_start;
            annotation.length = rebased_end
                .checked_sub(rebased_start)
                .ok_or_else(|| anyhow::anyhow!("TAS annotation deletion range underflows"))?;
            annotations.push(annotation);
        }
        self.project.annotations = annotations;
        self.record_timeline_impact(branch_id, start);
        Ok(())
    }

    pub fn set_input_range(
        &mut self,
        branch_id: &str,
        start: u64,
        length: u64,
        input: TasInputFrame,
    ) -> Result<()> {
        let pattern = TasInputPattern::constant(length, input)?;
        self.replace_input_pattern(branch_id, start, &pattern)
    }

    pub fn replace_input_pattern(
        &mut self,
        branch_id: &str,
        start: u64,
        pattern: &TasInputPattern,
    ) -> Result<()> {
        let branch = self.branch_mut(branch_id)?;
        replace_branch_input_pattern(branch, start, pattern)
    }

    pub fn replace_branch_events(
        &mut self,
        branch_id: &str,
        events: Vec<ReplayEvent>,
    ) -> Result<()> {
        let canonical = decode_replay_event_stream(&encode_replay_event_stream(&events)?)?;
        let branch = self.branch_mut(branch_id)?;
        if canonical != branch.events {
            branch.events = canonical;
        }
        Ok(())
    }

    fn branch_mut(&mut self, branch_id: &str) -> Result<&mut TasBranch> {
        self.project
            .branches
            .iter_mut()
            .find(|branch| branch.id == branch_id)
            .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {branch_id:?}"))
    }

    fn branch_index(&self, branch_id: &str) -> Result<usize> {
        self.project
            .branches
            .iter()
            .position(|branch| branch.id == branch_id)
            .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {branch_id:?}"))
    }

    fn record_timeline_impact(&mut self, branch_id: &str, cursor: u64) {
        self.timeline_earliest_cursors
            .entry(branch_id.to_owned())
            .and_modify(|earliest| *earliest = (*earliest).min(cursor))
            .or_insert(cursor);
    }
}

fn insert_input_frames(
    spans: &[TasInputSpan],
    cursor: u64,
    count: u64,
) -> Result<Vec<TasInputSpan>> {
    let mut rebased = Vec::with_capacity(spans.len().saturating_add(1));
    for span in spans {
        let span_end = span
            .start
            .checked_add(span.length)
            .ok_or_else(|| anyhow::anyhow!("TAS input span insertion overflows"))?;
        if span_end <= cursor {
            rebased.push(*span);
        } else if span.start >= cursor {
            rebased.push(TasInputSpan {
                start: span
                    .start
                    .checked_add(count)
                    .ok_or_else(|| anyhow::anyhow!("TAS input span insertion overflows"))?,
                ..*span
            });
        } else {
            rebased.push(TasInputSpan {
                length: cursor
                    .checked_sub(span.start)
                    .ok_or_else(|| anyhow::anyhow!("TAS input span insertion range underflows"))?,
                ..*span
            });
            rebased.push(TasInputSpan {
                start: cursor
                    .checked_add(count)
                    .ok_or_else(|| anyhow::anyhow!("TAS input span insertion overflows"))?,
                length: span_end
                    .checked_sub(cursor)
                    .ok_or_else(|| anyhow::anyhow!("TAS input span insertion range underflows"))?,
                input: span.input,
            });
        }
    }
    normalize_spans(&mut rebased);
    Ok(rebased)
}

fn delete_input_frames(
    spans: &[TasInputSpan],
    start: u64,
    end: u64,
    count: u64,
) -> Result<Vec<TasInputSpan>> {
    let mut rebased = Vec::with_capacity(spans.len());
    for span in spans {
        let span_end = span
            .start
            .checked_add(span.length)
            .ok_or_else(|| anyhow::anyhow!("TAS input span deletion overflows"))?;
        if span_end <= start {
            rebased.push(*span);
        } else if span.start >= end {
            rebased.push(TasInputSpan {
                start: span
                    .start
                    .checked_sub(count)
                    .ok_or_else(|| anyhow::anyhow!("TAS input span deletion underflows"))?,
                ..*span
            });
        } else {
            if span.start < start {
                rebased.push(TasInputSpan {
                    length: start.checked_sub(span.start).ok_or_else(|| {
                        anyhow::anyhow!("TAS input span deletion range underflows")
                    })?,
                    ..*span
                });
            }
            if span_end > end {
                rebased.push(TasInputSpan {
                    start: end
                        .checked_sub(count)
                        .ok_or_else(|| anyhow::anyhow!("TAS input span deletion underflows"))?,
                    length: span_end.checked_sub(end).ok_or_else(|| {
                        anyhow::anyhow!("TAS input span deletion range underflows")
                    })?,
                    input: span.input,
                });
            }
        }
    }
    normalize_spans(&mut rebased);
    Ok(rebased)
}

fn delete_cursor(cursor: u64, start: u64, end: u64, count: u64) -> Result<u64> {
    if cursor <= start {
        Ok(cursor)
    } else if cursor < end {
        Ok(start)
    } else {
        cursor
            .checked_sub(count)
            .ok_or_else(|| anyhow::anyhow!("TAS cursor deletion underflows"))
    }
}

fn set_event_frame(event: &mut ReplayEvent, new_frame: u64) {
    match event {
        ReplayEvent::FdsDiskSide { frame, .. }
        | ReplayEvent::Media { frame, .. }
        | ReplayEvent::GameBoyLink { frame, .. }
        | ReplayEvent::GameBoyLinkState { frame, .. }
        | ReplayEvent::GameBoyLinkStateAtTick { frame, .. }
        | ReplayEvent::WonderSwanLink { frame, .. } => *frame = new_frame,
    }
}

fn branch_hashes(
    project: &TasProject,
    sync_identity: TasDigest,
) -> Result<BTreeMap<String, TasDigest>> {
    project
        .branches
        .iter()
        .map(|branch| {
            Ok((
                branch.id.clone(),
                hash_branch(sync_identity, branch, branch.frame_count, true)?,
            ))
        })
        .collect()
}

fn normalize_spans(spans: &mut Vec<TasInputSpan>) {
    spans.sort_unstable_by_key(|span| span.start);
    let mut normalized = Vec::<TasInputSpan>::with_capacity(spans.len());
    for span in spans.drain(..) {
        if span.length == 0 || span.input == TasInputFrame::default() {
            continue;
        }
        if let Some(previous) = normalized.last_mut()
            && previous.start + previous.length == span.start
            && previous.input == span.input
        {
            previous.length += span.length;
        } else {
            normalized.push(span);
        }
    }
    *spans = normalized;
}

fn earliest_event_difference(before: &[ReplayEvent], after: &[ReplayEvent]) -> u64 {
    let shared = before
        .iter()
        .zip(after)
        .take_while(|(left, right)| left == right)
        .count();
    match (before.get(shared), after.get(shared)) {
        (Some(left), Some(right)) => left.frame().min(right.frame()),
        (Some(event), None) | (None, Some(event)) => event.frame(),
        (None, None) => 0,
    }
}

fn earliest_branch_movie_difference(before: &TasBranch, after: &TasBranch) -> Option<u64> {
    let input = earliest_input_difference(before, after);
    let events = (before.events != after.events)
        .then(|| earliest_event_difference(&before.events, &after.events));
    match (input, events) {
        (Some(input), Some(event)) => Some(input.min(event)),
        (Some(cursor), None) | (None, Some(cursor)) => Some(cursor),
        (None, None) => (before.frame_count != after.frame_count)
            .then_some(before.frame_count.min(after.frame_count)),
    }
}

fn earliest_input_difference(before: &TasBranch, after: &TasBranch) -> Option<u64> {
    let frame_count = before.frame_count.min(after.frame_count);
    let mut cursor = 0;
    let mut before_index = 0;
    let mut after_index = 0;
    while cursor < frame_count {
        let (before_input, before_boundary) =
            input_and_next_boundary(&before.input_spans, &mut before_index, cursor, frame_count);
        let (after_input, after_boundary) =
            input_and_next_boundary(&after.input_spans, &mut after_index, cursor, frame_count);
        if before_input != after_input {
            return Some(cursor);
        }
        cursor = before_boundary.min(after_boundary);
    }
    None
}

fn input_and_next_boundary(
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
        (span.input, span.start + span.length)
    } else {
        (TasInputFrame::default(), span.start.min(frame_count))
    }
}
