use anyhow::{Result, bail};

use super::digital_transform::validate_mask;
use crate::{
    debug::tas_editor::{TasEditorWindowState, timeline_selection::TasInputSelection},
    tas_project::{
        TasDigest, TasDigitalInputMask, TasDigitalTransform, TasEditorSession, TasInputPattern,
    },
};

const MAX_MARKED_INPUT_RANGES: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
struct TasMarkedRangesContext {
    expected_project_sha256: TasDigest,
    target_branch_id: String,
    expected_edit_generation: u64,
    expected_frame_count: u64,
}

impl TasMarkedRangesContext {
    fn from_session(session: &TasEditorSession) -> Self {
        Self {
            expected_project_sha256: session.project_content_sha256(),
            target_branch_id: session.selected_branch_id().to_owned(),
            expected_edit_generation: session.project().edit_generation(),
            expected_frame_count: session.selected_branch().frame_count(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in super::super) struct TasMarkedRangesSnapshot {
    pub(in super::super) expected_project_sha256: TasDigest,
    pub(in super::super) target_branch_id: String,
    pub(in super::super) expected_edit_generation: u64,
    pub(in super::super) expected_frame_count: u64,
    pub(in super::super) revision: u64,
    pub(in super::super) ranges: Vec<(u64, u64)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in super::super) enum TasMarkedRangesAction {
    Add {
        snapshot: TasMarkedRangesSnapshot,
        selection: TasInputSelection,
    },
    Remove {
        snapshot: TasMarkedRangesSnapshot,
        index: usize,
    },
    Clear {
        snapshot: TasMarkedRangesSnapshot,
    },
    Apply {
        snapshot: TasMarkedRangesSnapshot,
        target_movie_sha256: TasDigest,
        mask: TasDigitalInputMask,
        transform: TasDigitalTransform,
    },
}

pub(in super::super) struct TasMarkedInputRangesState {
    context: Option<TasMarkedRangesContext>,
    revision: u64,
    ranges: Vec<(u64, u64)>,
}

impl TasMarkedInputRangesState {
    pub(in super::super) fn new() -> Self {
        Self {
            context: None,
            revision: 0,
            ranges: Vec::new(),
        }
    }

    pub(in super::super) fn reset(&mut self) -> Result<()> {
        let revision = self.next_revision()?;
        self.context = None;
        self.ranges.clear();
        self.revision = revision;
        Ok(())
    }

    pub(in super::super) fn snapshot(
        &mut self,
        session: &TasEditorSession,
    ) -> Result<TasMarkedRangesSnapshot> {
        self.sync(session)?;
        let context = self
            .context
            .as_ref()
            .expect("marked range context is installed by sync");
        Ok(TasMarkedRangesSnapshot {
            expected_project_sha256: context.expected_project_sha256,
            target_branch_id: context.target_branch_id.clone(),
            expected_edit_generation: context.expected_edit_generation,
            expected_frame_count: context.expected_frame_count,
            revision: self.revision,
            ranges: self.ranges.clone(),
        })
    }

    pub(in super::super) fn visible_ranges(&self, session: &TasEditorSession) -> &[(u64, u64)] {
        if self.context.as_ref() == Some(&TasMarkedRangesContext::from_session(session)) {
            &self.ranges
        } else {
            &[]
        }
    }

    fn sync(&mut self, session: &TasEditorSession) -> Result<()> {
        let context = TasMarkedRangesContext::from_session(session);
        if self.context.as_ref() == Some(&context) {
            return Ok(());
        }
        let revision = self.next_revision()?;
        self.context = Some(context);
        self.ranges.clear();
        self.revision = revision;
        Ok(())
    }

    fn matches_snapshot(&self, snapshot: &TasMarkedRangesSnapshot) -> bool {
        self.context.as_ref().is_some_and(|context| {
            context.expected_project_sha256 == snapshot.expected_project_sha256
                && context.target_branch_id == snapshot.target_branch_id
                && context.expected_edit_generation == snapshot.expected_edit_generation
                && context.expected_frame_count == snapshot.expected_frame_count
        }) && self.revision == snapshot.revision
            && self.ranges == snapshot.ranges
    }

    pub(in super::super) fn next_revision(&self) -> Result<u64> {
        self.revision
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("marked input range revision overflow"))
    }

    fn replace_ranges(&mut self, ranges: Vec<(u64, u64)>) -> Result<()> {
        let revision = self.next_revision()?;
        self.ranges = ranges;
        self.revision = revision;
        Ok(())
    }

    pub(in super::super) fn refresh_after_apply(
        &mut self,
        session: &TasEditorSession,
        revision: u64,
        ranges: Vec<(u64, u64)>,
    ) {
        self.context = Some(TasMarkedRangesContext::from_session(session));
        self.revision = revision;
        self.ranges = ranges;
    }
}

impl TasEditorWindowState {
    pub(super) fn apply_marked_ranges_action(
        &mut self,
        action: TasMarkedRangesAction,
    ) -> Result<String> {
        match action {
            TasMarkedRangesAction::Add {
                snapshot,
                selection,
            } => self.add_marked_range(snapshot, selection),
            TasMarkedRangesAction::Remove { snapshot, index } => {
                self.remove_marked_range(snapshot, index)
            }
            TasMarkedRangesAction::Clear { snapshot } => self.clear_marked_ranges(snapshot),
            TasMarkedRangesAction::Apply {
                snapshot,
                target_movie_sha256,
                mask,
                transform,
            } => self.apply_transform_to_marked_ranges(
                snapshot,
                target_movie_sha256,
                mask,
                transform,
            ),
        }
    }

    fn add_marked_range(
        &mut self,
        snapshot: TasMarkedRangesSnapshot,
        selection: TasInputSelection,
    ) -> Result<String> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
        self.input_clipboard.marked_ranges.sync(session)?;
        validate_snapshot(&self.input_clipboard.marked_ranges, &snapshot)?;
        if self.timeline_selection.snapshot(session).as_ref() != Some(&selection) {
            bail!("input selection changed after this range was marked; retry it");
        }
        validate_selection(session, &selection)?;
        let mut ranges = snapshot.ranges;
        ranges.push((selection.start, selection.end));
        let ranges = normalize_ranges(ranges, session.selected_branch().frame_count())?;
        let marked_frames = selected_frame_count(&ranges)?;
        self.input_clipboard.marked_ranges.replace_ranges(ranges)?;
        Ok(format!(
            "Marked {marked_frames} input frames across {} ranges",
            self.input_clipboard.marked_ranges.ranges.len()
        ))
    }

    fn remove_marked_range(
        &mut self,
        snapshot: TasMarkedRangesSnapshot,
        index: usize,
    ) -> Result<String> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
        self.input_clipboard.marked_ranges.sync(session)?;
        validate_snapshot(&self.input_clipboard.marked_ranges, &snapshot)?;
        if index >= snapshot.ranges.len() {
            bail!("marked input range changed after removal was requested; retry it");
        }
        let mut ranges = snapshot.ranges;
        ranges.remove(index);
        let marked_frames = selected_frame_count(&ranges)?;
        self.input_clipboard.marked_ranges.replace_ranges(ranges)?;
        Ok(format!(
            "{} input frames remain marked across {} ranges",
            marked_frames,
            self.input_clipboard.marked_ranges.ranges.len()
        ))
    }

    fn clear_marked_ranges(&mut self, snapshot: TasMarkedRangesSnapshot) -> Result<String> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
        self.input_clipboard.marked_ranges.sync(session)?;
        validate_snapshot(&self.input_clipboard.marked_ranges, &snapshot)?;
        self.input_clipboard
            .marked_ranges
            .replace_ranges(Vec::new())?;
        Ok("Cleared marked input ranges".to_owned())
    }

    fn apply_transform_to_marked_ranges(
        &mut self,
        snapshot: TasMarkedRangesSnapshot,
        target_movie_sha256: TasDigest,
        mask: TasDigitalInputMask,
        transform: TasDigitalTransform,
    ) -> Result<String> {
        let prepared = {
            let session = self
                .session
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
            self.input_clipboard.marked_ranges.sync(session)?;
            validate_snapshot(&self.input_clipboard.marked_ranges, &snapshot)?;
            validate_ranges(&snapshot.ranges, session.selected_branch().frame_count())?;
            if snapshot.ranges.is_empty() {
                bail!("mark at least one input range before editing controls");
            }
            if session
                .project()
                .branch_movie_sha256(&snapshot.target_branch_id)?
                != target_movie_sha256
            {
                bail!("marked range target branch changed after this edit was requested; retry it");
            }
            validate_mask(session, mask)?;
            session.selected_branch().prepare_digital_transform_ranges(
                &snapshot.ranges,
                mask,
                transform,
            )?
        };
        if prepared.is_empty() {
            return Ok("Control edit made no change".to_owned());
        }
        let next_revision = self.input_clipboard.marked_ranges.next_revision()?;
        let (changed_start, changed_end) = changed_bounds(&prepared)?;
        {
            let session = self
                .session
                .as_ref()
                .expect("open session was checked while preparing marked ranges");
            for (_, pattern) in &prepared {
                for span in pattern.spans() {
                    super::ensure_nondefault_input_authorable(
                        session.project().identity(),
                        session.project().assets(),
                        span.input,
                    )?;
                }
            }
        }
        let branch_id = snapshot.target_branch_id.clone();
        let outcome = self
            .session
            .as_mut()
            .expect("open session was checked while preparing marked ranges")
            .edit_transaction(move |edit| {
                for (start, pattern) in &prepared {
                    edit.replace_input_pattern(&branch_id, *start, pattern)?;
                }
                Ok(())
            })?;
        if !outcome.changed {
            return Ok("Control edit made no change".to_owned());
        }
        let session = self
            .session
            .as_ref()
            .expect("open session was checked before applying marked ranges");
        self.input_clipboard.marked_ranges.refresh_after_apply(
            session,
            next_revision,
            snapshot.ranges.clone(),
        );
        self.execution_preview.clear();
        self.queue_linked_edit_reconstruction(changed_start, changed_end);
        let operation = match transform {
            TasDigitalTransform::Clear => "Cleared selected controls",
            TasDigitalTransform::Invert => "Inverted selected controls",
            TasDigitalTransform::Reverse => "Reversed selected controls",
        };
        let message = format!("{operation} across {} marked ranges", snapshot.ranges.len());
        if let Some(error) = self.detach_incompatible_execution() {
            return Ok(format!(
                "{message}; private execution detached because the edited project no longer matches it: {error:#}"
            ));
        }
        Ok(message)
    }
}

fn validate_snapshot(
    state: &TasMarkedInputRangesState,
    snapshot: &TasMarkedRangesSnapshot,
) -> Result<()> {
    if !state.matches_snapshot(snapshot) {
        bail!("marked input ranges changed after this operation was requested; retry it");
    }
    Ok(())
}

fn validate_selection(session: &TasEditorSession, selection: &TasInputSelection) -> Result<()> {
    if selection.branch_id != session.selected_branch_id()
        || selection.start >= selection.end
        || selection.end > session.selected_branch().frame_count()
    {
        bail!("select a valid non-empty input range before marking it");
    }
    Ok(())
}

fn normalize_ranges(mut ranges: Vec<(u64, u64)>, frame_count: u64) -> Result<Vec<(u64, u64)>> {
    for &(start, end) in &ranges {
        if start >= end || end > frame_count {
            bail!("marked input range is outside the active branch");
        }
    }
    ranges.sort_unstable();
    let mut normalized: Vec<(u64, u64)> = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        if let Some((_, previous_end)) = normalized.last_mut()
            && start <= *previous_end
        {
            *previous_end = (*previous_end).max(end);
            continue;
        }
        if normalized.len() == MAX_MARKED_INPUT_RANGES {
            bail!("mark at most {MAX_MARKED_INPUT_RANGES} discontiguous input ranges");
        }
        normalized.push((start, end));
    }
    Ok(normalized)
}

fn validate_ranges(ranges: &[(u64, u64)], frame_count: u64) -> Result<()> {
    if ranges.len() > MAX_MARKED_INPUT_RANGES {
        bail!("marked input range count exceeds the limit of {MAX_MARKED_INPUT_RANGES}");
    }
    let normalized = normalize_ranges(ranges.to_vec(), frame_count)?;
    if normalized != ranges {
        bail!("marked input ranges must be sorted, disjoint, and normalized");
    }
    selected_frame_count(ranges)?;
    Ok(())
}

fn selected_frame_count(ranges: &[(u64, u64)]) -> Result<u64> {
    ranges.iter().try_fold(0_u64, |total, &(start, end)| {
        total
            .checked_add(end - start)
            .ok_or_else(|| anyhow::anyhow!("marked input frame count overflows"))
    })
}

fn changed_bounds(prepared: &[(u64, TasInputPattern)]) -> Result<(u64, u64)> {
    let start = prepared
        .first()
        .map(|(start, _)| *start)
        .ok_or_else(|| anyhow::anyhow!("marked input edit contains no changed ranges"))?;
    let end = prepared.iter().try_fold(start, |end, (start, pattern)| {
        start
            .checked_add(pattern.length())
            .map(|pattern_end| end.max(pattern_end))
            .ok_or_else(|| anyhow::anyhow!("marked input edit boundary overflows"))
    })?;
    Ok((start, end))
}
