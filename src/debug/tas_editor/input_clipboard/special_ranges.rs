use anyhow::{Result, bail};

use super::marked_ranges::TasMarkedRangesSnapshot;
use crate::{
    debug::tas_editor::{
        TasEditorWindowState, special_input_editor::special_input_capabilities,
        timeline_selection::TasInputSelection,
    },
    tas_project::{
        TasDigest, TasEditorSession, TasInputPattern, TasSpecialInputMask, TasSpecialTransform,
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in super::super) enum TasSpecialRangeTarget {
    Selection(TasInputSelection),
    Marked(TasMarkedRangesSnapshot),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in super::super) struct TasSpecialRangeAction {
    pub(in super::super) expected_project_sha256: TasDigest,
    pub(in super::super) target_branch_id: String,
    pub(in super::super) target_movie_sha256: TasDigest,
    pub(in super::super) target: TasSpecialRangeTarget,
    pub(in super::super) mask: TasSpecialInputMask,
    pub(in super::super) transform: TasSpecialTransform,
}

impl TasEditorWindowState {
    pub(super) fn apply_special_range_transform(
        &mut self,
        action: TasSpecialRangeAction,
    ) -> Result<String> {
        let TasSpecialRangeAction {
            expected_project_sha256,
            target_branch_id,
            target_movie_sha256,
            target,
            mask,
            transform,
        } = action;
        let (ranges, marked_snapshot) = {
            let session = self
                .session
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
            validate_context(
                session,
                expected_project_sha256,
                &target_branch_id,
                target_movie_sha256,
            )?;
            validate_mask(session, mask)?;
            match &target {
                TasSpecialRangeTarget::Selection(selection) => {
                    if self.timeline_selection.snapshot(session).as_ref() != Some(selection) {
                        bail!(
                            "input selection changed after this recorded-input edit was requested; retry it"
                        );
                    }
                    validate_selection(session, selection)?;
                    (vec![(selection.start, selection.end)], None)
                }
                TasSpecialRangeTarget::Marked(snapshot) => {
                    let current = self.input_clipboard.marked_ranges.snapshot(session)?;
                    if &current != snapshot {
                        bail!(
                            "marked input ranges changed after this recorded-input edit was requested; retry it"
                        );
                    }
                    if snapshot.ranges.is_empty() {
                        bail!("mark at least one input range before editing recorded input");
                    }
                    (snapshot.ranges.clone(), Some(snapshot.clone()))
                }
            }
        };

        let prepared = self
            .session
            .as_ref()
            .expect("open session was checked before preparing recorded-input ranges")
            .selected_branch()
            .prepare_special_transform_ranges(&ranges, mask, transform)?;
        if prepared.is_empty() {
            return Ok("Recorded-input edit made no change".to_owned());
        }

        let marked_revision = marked_snapshot
            .as_ref()
            .map(|_| self.input_clipboard.marked_ranges.next_revision())
            .transpose()?;
        let (changed_start, changed_end) = changed_bounds(&prepared)?;
        {
            let session = self
                .session
                .as_ref()
                .expect("open session was checked while preparing recorded-input ranges");
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

        let branch_id = target_branch_id;
        let outcome = self
            .session
            .as_mut()
            .expect("open session was checked while preparing recorded-input ranges")
            .edit_transaction(move |edit| {
                for (start, pattern) in &prepared {
                    edit.replace_input_pattern(&branch_id, *start, pattern)?;
                }
                Ok(())
            })?;
        if !outcome.changed {
            return Ok("Recorded-input edit made no change".to_owned());
        }

        if let (Some(snapshot), Some(revision)) = (marked_snapshot, marked_revision) {
            let session = self
                .session
                .as_ref()
                .expect("open session was checked before refreshing marked ranges");
            self.input_clipboard.marked_ranges.refresh_after_apply(
                session,
                revision,
                snapshot.ranges,
            );
        }
        self.execution_preview.clear();
        self.queue_linked_edit_reconstruction(changed_start, changed_end);

        let operation = match transform {
            TasSpecialTransform::Clear => "Cleared selected recorded input",
            TasSpecialTransform::Reverse => "Reversed selected recorded input",
        };
        let target = match target {
            TasSpecialRangeTarget::Selection(_) => "in the input selection".to_owned(),
            TasSpecialRangeTarget::Marked(snapshot) => {
                format!("across {} marked ranges", snapshot.ranges.len())
            }
        };
        let message = format!("{operation} {target}");
        if let Some(error) = self.detach_incompatible_execution() {
            return Ok(format!(
                "{message}; private execution detached because the edited project no longer matches it: {error:#}"
            ));
        }
        Ok(message)
    }
}

fn validate_context(
    session: &TasEditorSession,
    expected_project_sha256: TasDigest,
    target_branch_id: &str,
    target_movie_sha256: TasDigest,
) -> Result<()> {
    if session.project_content_sha256() != expected_project_sha256
        || session.selected_branch_id() != target_branch_id
        || session.project().branch_movie_sha256(target_branch_id)? != target_movie_sha256
    {
        bail!(
            "TAS project or branch changed after this recorded-input edit was requested; retry it"
        );
    }
    Ok(())
}

fn validate_selection(session: &TasEditorSession, selection: &TasInputSelection) -> Result<()> {
    if selection.branch_id != session.selected_branch_id()
        || selection.start >= selection.end
        || selection.end > session.selected_branch().frame_count()
    {
        bail!("select a valid non-empty input range before editing recorded input");
    }
    Ok(())
}

fn validate_mask(session: &TasEditorSession, mask: TasSpecialInputMask) -> Result<()> {
    if !mask.zapper && !mask.tilt_x && !mask.tilt_y && !mask.camera {
        bail!("select at least one recorded-input channel to edit");
    }
    let capabilities = special_input_capabilities(session.project().identity());
    if mask.zapper && !capabilities.nes_zapper {
        bail!("the TAS project does not declare NES Zapper recorded input");
    }
    if (mask.tilt_x || mask.tilt_y) && !(capabilities.mbc7_tilt || capabilities.gba_tilt) {
        bail!("the TAS project does not declare supported recorded tilt input");
    }
    if mask.camera && !capabilities.pocket_camera {
        bail!("the TAS project does not declare Game Boy Pocket Camera recorded input");
    }
    Ok(())
}

fn changed_bounds(prepared: &[(u64, TasInputPattern)]) -> Result<(u64, u64)> {
    let start = prepared
        .first()
        .map(|(start, _)| *start)
        .ok_or_else(|| anyhow::anyhow!("recorded-input edit contains no changed ranges"))?;
    let end = prepared.iter().try_fold(start, |end, (start, pattern)| {
        start
            .checked_add(pattern.length())
            .map(|pattern_end| end.max(pattern_end))
            .ok_or_else(|| anyhow::anyhow!("recorded-input edit boundary overflows"))
    })?;
    Ok((start, end))
}
