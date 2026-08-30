mod presentation;

use anyhow::{Result, bail};

use super::{TasEditorWindowState, special_input_editor::ensure_nondefault_input_authorable};
use crate::tas_project::{TasDigest, TasEditorSession, TasInputPattern};
use presentation::TasInputSelection;

pub(super) fn draw_input_clipboard(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    state: &mut TasInputClipboardState,
    actions: &mut Vec<super::TasEditorAction>,
) -> Option<(u64, u64)> {
    presentation::draw_input_clipboard(ui, session, state, actions)
}

pub(super) fn selected_input_range(
    session: &TasEditorSession,
    state: &mut TasInputClipboardState,
) -> Option<(u64, u64)> {
    state.sync_selection(session);
    let selection = state.selection();
    (selection.start <= selection.end)
        .then_some((selection.start, selection.end))
        .filter(|(start, end)| start != end)
}

struct TasInputPatternPasteWitness {
    expected_project_sha256: TasDigest,
    target_branch_id: String,
    target_movie_sha256: TasDigest,
    expected_cursor: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum TasInputClipboardAction {
    CopyPattern(TasInputClipboardCopyAction),
    PasteAtCursor(TasInputClipboardPasteAction),
    TileAcrossSelection(TasInputClipboardTileAction),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TasInputClipboardCopyAction {
    expected_project_sha256: TasDigest,
    source_branch_id: String,
    source_movie_sha256: TasDigest,
    start: u64,
    pattern: TasInputPattern,
    expected_selection: Option<TasInputSelection>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TasInputClipboardPasteAction {
    expected_project_sha256: TasDigest,
    target_branch_id: String,
    target_movie_sha256: TasDigest,
    cursor: u64,
    clipboard_generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TasInputClipboardTileAction {
    expected_project_sha256: TasDigest,
    target_branch_id: String,
    target_movie_sha256: TasDigest,
    selection: TasInputSelection,
    clipboard_generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TasInputClipboardEntry {
    source_branch_id: String,
    source_movie_sha256: TasDigest,
    start: u64,
    pattern: TasInputPattern,
}

pub(super) struct TasInputClipboardState {
    generation: u64,
    entry: Option<TasInputClipboardEntry>,
    selection: Option<TasInputSelection>,
    selection_frame_count: Option<u64>,
}

impl TasInputClipboardAction {
    pub(super) fn copy_pattern(
        expected_project_sha256: TasDigest,
        source_branch_id: String,
        source_movie_sha256: TasDigest,
        start: u64,
        pattern: TasInputPattern,
    ) -> Self {
        Self::CopyPattern(TasInputClipboardCopyAction {
            expected_project_sha256,
            source_branch_id,
            source_movie_sha256,
            start,
            pattern,
            expected_selection: None,
        })
    }

    pub(super) fn copy_constant(
        expected_project_sha256: TasDigest,
        source_branch_id: String,
        source_movie_sha256: TasDigest,
        start: u64,
        length: u64,
        input: crate::tas_project::TasInputFrame,
    ) -> Result<Self> {
        Ok(Self::copy_pattern(
            expected_project_sha256,
            source_branch_id,
            source_movie_sha256,
            start,
            TasInputPattern::constant(length, input)?,
        ))
    }

    #[cfg(test)]
    pub(super) fn paste_at_cursor(
        expected_project_sha256: TasDigest,
        target_branch_id: String,
        target_movie_sha256: TasDigest,
        cursor: u64,
        clipboard_generation: u64,
    ) -> Self {
        Self::PasteAtCursor(TasInputClipboardPasteAction {
            expected_project_sha256,
            target_branch_id,
            target_movie_sha256,
            cursor,
            clipboard_generation,
        })
    }

    #[cfg(test)]
    pub(super) fn copy_selection(
        expected_project_sha256: TasDigest,
        source_branch_id: String,
        source_movie_sha256: TasDigest,
        start: u64,
        pattern: TasInputPattern,
        expected_selection: (u64, u64),
    ) -> Self {
        let expected_branch_id = source_branch_id.clone();
        Self::CopyPattern(TasInputClipboardCopyAction {
            expected_project_sha256,
            source_branch_id,
            source_movie_sha256,
            start,
            pattern,
            expected_selection: Some(TasInputSelection {
                branch_id: expected_branch_id,
                start: expected_selection.0,
                end: expected_selection.1,
            }),
        })
    }

    #[cfg(test)]
    pub(super) fn tile_selection(
        expected_project_sha256: TasDigest,
        target_branch_id: String,
        target_movie_sha256: TasDigest,
        selection_start: u64,
        selection_end: u64,
        clipboard_generation: u64,
    ) -> Self {
        let selection_branch_id = target_branch_id.clone();
        Self::TileAcrossSelection(TasInputClipboardTileAction {
            expected_project_sha256,
            target_branch_id,
            target_movie_sha256,
            selection: TasInputSelection {
                branch_id: selection_branch_id,
                start: selection_start,
                end: selection_end,
            },
            clipboard_generation,
        })
    }
}

impl TasInputClipboardState {
    pub(super) fn new() -> Self {
        Self {
            generation: 0,
            entry: None,
            selection: None,
            selection_frame_count: None,
        }
    }

    pub(super) fn clear(&mut self) -> Result<()> {
        if self.entry.is_some() {
            self.bump_generation()?;
            self.entry = None;
        }
        self.selection = None;
        self.selection_frame_count = None;
        Ok(())
    }

    fn sync_selection(&mut self, session: &TasEditorSession) {
        let branch_id = session.selected_branch_id();
        let frame_count = session.selected_branch().frame_count();
        let stale = self.selection.as_ref().is_none_or(|selection| {
            selection.branch_id != branch_id || self.selection_frame_count != Some(frame_count)
        });
        if stale {
            self.selection = Some(TasInputSelection::new(
                branch_id.to_owned(),
                session.cursor(),
            ));
            self.selection_frame_count = Some(frame_count);
        }
    }

    fn selection(&self) -> &TasInputSelection {
        self.selection
            .as_ref()
            .expect("input clipboard selection was synchronized before use")
    }

    fn copy(
        &mut self,
        action: TasInputClipboardCopyAction,
        session: &TasEditorSession,
    ) -> Result<()> {
        if session.project_content_sha256() != action.expected_project_sha256 {
            bail!("TAS project changed after this input pattern was prepared; retry it");
        }
        if session.selected_branch_id() != action.source_branch_id {
            bail!("selected TAS branch changed after this input pattern was prepared; retry it");
        }
        if session
            .project()
            .branch_movie_sha256(&action.source_branch_id)?
            != action.source_movie_sha256
        {
            bail!("source TAS branch changed after this input pattern was prepared; retry it");
        }
        let source = session
            .project()
            .branch(&action.source_branch_id)
            .ok_or_else(|| anyhow::anyhow!("input-pattern source branch no longer exists"))?;
        if let Some(expected_selection) = &action.expected_selection
            && self.selection.as_ref() != Some(expected_selection)
        {
            bail!("input selection changed after this copy was requested; retry it");
        }
        if source.input_pattern(action.start, action.pattern.length())? != action.pattern {
            bail!("input-pattern source changed after this copy was prepared; retry it");
        }
        self.bump_generation()?;
        self.entry = Some(TasInputClipboardEntry {
            source_branch_id: action.source_branch_id,
            source_movie_sha256: action.source_movie_sha256,
            start: action.start,
            pattern: action.pattern,
        });
        Ok(())
    }

    fn entry(&self, generation: u64) -> Result<&TasInputClipboardEntry> {
        if self.generation != generation {
            bail!("input clipboard changed after this operation was requested; retry it");
        }
        self.entry
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("input clipboard is empty"))
    }

    fn bump_generation(&mut self) -> Result<()> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("input clipboard generation overflow"))?;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn has_entry(&self) -> bool {
        self.entry.is_some()
    }

    #[cfg(test)]
    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    #[cfg(test)]
    pub(super) fn set_selection(
        &mut self,
        branch_id: String,
        start: u64,
        end: u64,
        frame_count: u64,
    ) {
        self.selection = Some(TasInputSelection {
            branch_id,
            start,
            end,
        });
        self.selection_frame_count = Some(frame_count);
    }

    #[cfg(test)]
    pub(super) fn selection_after_sync(
        &mut self,
        session: &TasEditorSession,
    ) -> (String, u64, u64) {
        self.sync_selection(session);
        let selection = self.selection();
        (selection.branch_id.clone(), selection.start, selection.end)
    }
}

impl TasEditorWindowState {
    pub(super) fn apply_input_clipboard_action(
        &mut self,
        action: TasInputClipboardAction,
    ) -> Result<String> {
        match action {
            TasInputClipboardAction::CopyPattern(action) => {
                let session = self
                    .session
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
                self.input_clipboard.copy(action, session)?;
                let entry = self
                    .input_clipboard
                    .entry
                    .as_ref()
                    .expect("successful input-pattern copy installs a clipboard entry");
                Ok(format!(
                    "Copied input pattern {}..{} from branch {}",
                    entry.start,
                    entry.start.saturating_add(entry.pattern.length()),
                    entry.source_branch_id
                ))
            }
            TasInputClipboardAction::PasteAtCursor(action) => {
                let pattern = self
                    .input_clipboard
                    .entry(action.clipboard_generation)?
                    .pattern
                    .clone();
                let witness = TasInputPatternPasteWitness {
                    expected_project_sha256: action.expected_project_sha256,
                    target_branch_id: action.target_branch_id,
                    target_movie_sha256: action.target_movie_sha256,
                    expected_cursor: Some(action.cursor),
                };
                self.apply_pattern_at_cursor(witness, action.cursor, pattern)
            }
            TasInputClipboardAction::TileAcrossSelection(action) => {
                if self.input_clipboard.selection.as_ref() != Some(&action.selection) {
                    bail!("input selection changed after this tile was requested; retry it");
                }
                if action.selection.branch_id != action.target_branch_id {
                    bail!("input selection branch changed after this tile was requested; retry it");
                }
                let length = action.selection.length().ok_or_else(|| {
                    anyhow::anyhow!("input selection start is after its exclusive end")
                })?;
                if length == 0 {
                    bail!("input selection is empty");
                }
                let pattern = self
                    .input_clipboard
                    .entry(action.clipboard_generation)?
                    .pattern
                    .tile_to_length(length)?;
                let witness = TasInputPatternPasteWitness {
                    expected_project_sha256: action.expected_project_sha256,
                    target_branch_id: action.target_branch_id,
                    target_movie_sha256: action.target_movie_sha256,
                    expected_cursor: None,
                };
                self.apply_pattern_at_cursor(witness, action.selection.start, pattern)
            }
        }
    }

    fn apply_pattern_at_cursor(
        &mut self,
        witness: TasInputPatternPasteWitness,
        start: u64,
        pattern: TasInputPattern,
    ) -> Result<String> {
        let outcome = {
            let session = self
                .session
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
            if session.project_content_sha256() != witness.expected_project_sha256 {
                bail!("TAS project changed after this paste was requested; retry it");
            }
            if session.selected_branch_id() != witness.target_branch_id {
                bail!("selected TAS branch changed after this paste was requested; retry it");
            }
            if let Some(expected_cursor) = witness.expected_cursor
                && session.cursor() != expected_cursor
            {
                bail!("selected TAS cursor changed after this paste was requested; retry it");
            }
            if session
                .project()
                .branch_movie_sha256(&witness.target_branch_id)?
                != witness.target_movie_sha256
            {
                bail!("paste target branch changed after this paste was requested; retry it");
            }
            let current = session
                .selected_branch()
                .input_pattern(start, pattern.length())?;
            if current != pattern {
                for span in pattern.spans() {
                    ensure_nondefault_input_authorable(
                        session.project().identity(),
                        session.project().assets(),
                        span.input,
                    )?;
                }
            }
            let branch_id = witness.target_branch_id;
            session.edit_transaction(move |edit| {
                edit.replace_input_pattern(&branch_id, start, &pattern)
            })?
        };
        if !outcome.changed {
            return Ok("Input pattern made no change".to_owned());
        }
        self.execution_preview.clear();
        if let Some(error) = self.detach_incompatible_execution() {
            return Ok(format!(
                "Pasted input pattern; private execution detached because the edited project no longer matches it: {error:#}"
            ));
        }
        Ok("Pasted input pattern".to_owned())
    }
}
