use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use zeff_emu_common::replay::{
    ReplayEvent, decode_replay_event_stream, encode_replay_event_stream,
};

use super::identity::hash_branch;
use super::model::{
    TasBranch, TasBranchOrigin, TasDigest, TasInputFrame, TasInputSpan, TasProject,
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

/// Opaque editor used by [`TasProject::edit_transaction`].
///
/// Timeline length changes are deliberately excluded until marker, annotation, and event
/// rebasing have one explicit policy.
pub struct TasProjectEdit<'a> {
    project: &'a mut TasProject,
}

impl TasProject {
    /// Applies a batch of project edits atomically in memory.
    ///
    /// Any persisted change increments `edit_generation` once. `rerecord_count` increments once
    /// when the final movie of an existing branch changes, or when a new fork diverges from its
    /// captured parent movie in the same transaction. Presentation edits, branch selection, true
    /// no-ops, and a plain snapshot fork do not count as rerecords. A failed edit or validation
    /// leaves the project byte-for-byte unchanged.
    pub fn edit_transaction<F>(&mut self, edit: F) -> Result<TasEditOutcome>
    where
        F: FnOnce(&mut TasProjectEdit<'_>) -> Result<()>,
    {
        self.validate()?;
        let sync_identity = self.sync_identity_sha256()?;
        let before_hashes = branch_hashes(self, sync_identity)?;
        let before_ids = before_hashes.keys().cloned().collect::<BTreeSet<_>>();

        let mut candidate = self.clone();
        {
            let mut editor = TasProjectEdit {
                project: &mut candidate,
            };
            edit(&mut editor)?;
        }
        candidate.validate()?;

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
                Some(TasBranchEditImpactKind::Modified {
                    earliest_cursor: earliest_branch_movie_difference(before, after).unwrap_or(0),
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
    /// Creates a self-contained branch snapshot. The parent link records provenance only; later
    /// edits to either branch never alter the other branch or the stored origin hash.
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

    /// Replaces every controller/device input in a non-empty, fixed-length frame interval.
    pub fn set_input_range(
        &mut self,
        branch_id: &str,
        start: u64,
        length: u64,
        input: TasInputFrame,
    ) -> Result<()> {
        if length == 0 {
            bail!("TAS input edit range cannot be empty");
        }
        let end = start
            .checked_add(length)
            .ok_or_else(|| anyhow::anyhow!("TAS input edit range overflows"))?;
        let branch = self.branch_mut(branch_id)?;
        if end > branch.frame_count {
            bail!("TAS input edit range extends past branch end");
        }

        let mut spans = Vec::with_capacity(branch.input_spans.len() + 1);
        for span in &branch.input_spans {
            let span_end = span.start + span.length;
            if span_end <= start || span.start >= end {
                spans.push(*span);
                continue;
            }
            if span.start < start {
                spans.push(TasInputSpan {
                    length: start - span.start,
                    ..*span
                });
            }
            if span_end > end {
                spans.push(TasInputSpan {
                    start: end,
                    length: span_end - end,
                    input: span.input,
                });
            }
        }
        if input != TasInputFrame::default() {
            spans.push(TasInputSpan {
                start,
                length,
                input,
            });
        }
        normalize_spans(&mut spans);
        if spans != branch.input_spans {
            branch.input_spans = spans;
        }
        Ok(())
    }

    /// Replaces the complete event stream and normalizes it with the replay codec's canonical
    /// ordering rules.
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
