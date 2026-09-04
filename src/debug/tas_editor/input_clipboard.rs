mod events;
mod presentation;

use anyhow::{Result, bail};
use zeff_emu_common::replay::ReplayEvent;

use super::{
    TasEditorWindowState, special_input_editor::ensure_nondefault_input_authorable,
    timeline_selection::TasInputSelection,
};
use crate::tas_project::{TasDigest, TasEditorSession, TasInputPattern};
use events::{replacement_events, validate_copied_events};

pub(super) fn draw_input_clipboard(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    state: &mut TasInputClipboardState,
    selection: Option<&TasInputSelection>,
    actions: &mut Vec<super::TasEditorAction>,
) {
    presentation::draw_input_clipboard(ui, session, state, selection, actions)
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
    InsertAtCursor(TasInputClipboardPasteAction),
    TileAcrossSelection(TasInputClipboardTileAction),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TasInputClipboardCopyAction {
    expected_project_sha256: TasDigest,
    source_branch_id: String,
    source_movie_sha256: TasDigest,
    start: u64,
    pattern: TasInputPattern,
    events: Vec<ReplayEvent>,
    include_events: bool,
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
    events: Vec<ReplayEvent>,
    include_events: bool,
}

pub(super) struct TasInputClipboardState {
    generation: u64,
    entry: Option<TasInputClipboardEntry>,
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
            events: Vec::new(),
            include_events: false,
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
    pub(super) fn insert_at_cursor(
        expected_project_sha256: TasDigest,
        target_branch_id: String,
        target_movie_sha256: TasDigest,
        cursor: u64,
        clipboard_generation: u64,
    ) -> Self {
        Self::InsertAtCursor(TasInputClipboardPasteAction {
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
            events: Vec::new(),
            include_events: true,
            expected_selection: Some(TasInputSelection {
                branch_id: expected_branch_id,
                start: expected_selection.0,
                end: expected_selection.1,
            }),
        })
    }

    #[cfg(test)]
    pub(super) fn copy_selection_with_events(
        expected_project_sha256: TasDigest,
        source_movie_sha256: TasDigest,
        pattern: TasInputPattern,
        events: Vec<ReplayEvent>,
        expected_selection: TasInputSelection,
    ) -> Self {
        let source_branch_id = expected_selection.branch_id.clone();
        let start = expected_selection.start;
        Self::CopyPattern(TasInputClipboardCopyAction {
            expected_project_sha256,
            source_branch_id,
            source_movie_sha256,
            start,
            pattern,
            events,
            include_events: true,
            expected_selection: Some(expected_selection),
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
        }
    }

    pub(super) fn clear(&mut self) -> Result<()> {
        if self.entry.is_some() {
            self.bump_generation()?;
            self.entry = None;
        }
        Ok(())
    }

    fn copy(
        &mut self,
        action: TasInputClipboardCopyAction,
        session: &TasEditorSession,
        selection: Option<TasInputSelection>,
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
            && selection.as_ref() != Some(expected_selection)
        {
            bail!("input selection changed after this copy was requested; retry it");
        }
        if source.input_pattern(action.start, action.pattern.length())? != action.pattern {
            bail!("input-pattern source changed after this copy was prepared; retry it");
        }
        let events = if action.include_events {
            let end = action
                .start
                .checked_add(action.pattern.length())
                .ok_or_else(|| anyhow::anyhow!("copied event range overflows"))?;
            let source_events = source
                .events()
                .iter()
                .filter(|event| event.frame() >= action.start && event.frame() < end)
                .cloned()
                .collect::<Vec<_>>();
            if source_events != action.events {
                bail!("source TAS events changed after this copy was prepared; retry it");
            }
            validate_copied_events(session, &source_events)?;
            source_events
                .into_iter()
                .map(|mut event| {
                    let frame = event.frame() - action.start;
                    set_event_frame(&mut event, frame);
                    event
                })
                .collect()
        } else {
            Vec::new()
        };
        self.bump_generation()?;
        self.entry = Some(TasInputClipboardEntry {
            source_branch_id: action.source_branch_id,
            source_movie_sha256: action.source_movie_sha256,
            start: action.start,
            pattern: action.pattern,
            events,
            include_events: action.include_events,
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
                let selection = self.timeline_selection.snapshot(session);
                self.input_clipboard.copy(action, session, selection)?;
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
                let entry = self.input_clipboard.entry(action.clipboard_generation)?;
                let pattern = entry.pattern.clone();
                let events = entry.events.clone();
                let include_events = entry.include_events;
                let witness = TasInputPatternPasteWitness {
                    expected_project_sha256: action.expected_project_sha256,
                    target_branch_id: action.target_branch_id,
                    target_movie_sha256: action.target_movie_sha256,
                    expected_cursor: Some(action.cursor),
                };
                self.apply_pattern_at_cursor(
                    witness,
                    action.cursor,
                    pattern,
                    events,
                    include_events,
                )
            }
            TasInputClipboardAction::InsertAtCursor(action) => {
                let entry = self.input_clipboard.entry(action.clipboard_generation)?;
                let pattern = entry.pattern.clone();
                let events = entry.events.clone();
                let witness = TasInputPatternPasteWitness {
                    expected_project_sha256: action.expected_project_sha256,
                    target_branch_id: action.target_branch_id,
                    target_movie_sha256: action.target_movie_sha256,
                    expected_cursor: Some(action.cursor),
                };
                self.insert_pattern_at_cursor(witness, action.cursor, pattern, events)
            }
            TasInputClipboardAction::TileAcrossSelection(action) => {
                let session = self
                    .session
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
                if self.timeline_selection.snapshot(session).as_ref() != Some(&action.selection) {
                    bail!("input selection changed after this tile was requested; retry it");
                }
                if action.selection.branch_id != action.target_branch_id {
                    bail!("input selection branch changed after this tile was requested; retry it");
                }
                let length = action.selection.length();
                let entry = self.input_clipboard.entry(action.clipboard_generation)?;
                if !entry.events.is_empty() {
                    bail!("copied drive events cannot be tiled safely");
                }
                let pattern = entry.pattern.tile_to_length(length)?;
                let witness = TasInputPatternPasteWitness {
                    expected_project_sha256: action.expected_project_sha256,
                    target_branch_id: action.target_branch_id,
                    target_movie_sha256: action.target_movie_sha256,
                    expected_cursor: None,
                };
                self.apply_pattern_at_cursor(
                    witness,
                    action.selection.start,
                    pattern,
                    Vec::new(),
                    false,
                )
            }
        }
    }

    fn apply_pattern_at_cursor(
        &mut self,
        witness: TasInputPatternPasteWitness,
        start: u64,
        pattern: TasInputPattern,
        relative_events: Vec<ReplayEvent>,
        include_events: bool,
    ) -> Result<String> {
        let end = start
            .checked_add(pattern.length())
            .ok_or_else(|| anyhow::anyhow!("pasted input pattern boundary overflow"))?;
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
            let replacement_events = replacement_events(
                session,
                &witness.target_branch_id,
                start,
                end,
                &relative_events,
                include_events,
            )?;
            let branch_id = witness.target_branch_id;
            session.edit_transaction(move |edit| {
                edit.replace_input_pattern(&branch_id, start, &pattern)?;
                if let Some(events) = replacement_events {
                    edit.replace_branch_events(&branch_id, events)?;
                }
                Ok(())
            })?
        };
        if !outcome.changed {
            let session = self
                .session
                .as_ref()
                .expect("open session was checked before applying input pattern");
            self.timeline_selection.select_range(session, start, end);
            return Ok("Input pattern made no change".to_owned());
        }
        let session = self
            .session
            .as_ref()
            .expect("open session was checked before applying input pattern");
        self.timeline_selection.select_range(session, start, end);
        self.execution_preview.clear();
        self.queue_linked_edit_reconstruction(start, end);
        if let Some(error) = self.detach_incompatible_execution() {
            return Ok(format!(
                "Pasted input pattern; private execution detached because the edited project no longer matches it: {error:#}"
            ));
        }
        Ok("Pasted input pattern".to_owned())
    }

    fn insert_pattern_at_cursor(
        &mut self,
        witness: TasInputPatternPasteWitness,
        cursor: u64,
        pattern: TasInputPattern,
        relative_events: Vec<ReplayEvent>,
    ) -> Result<String> {
        let end = cursor
            .checked_add(pattern.length())
            .ok_or_else(|| anyhow::anyhow!("inserted input pattern boundary overflow"))?;
        {
            let session = self
                .session
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
            validate_pattern_target(session, &witness, "insertion")?;
            for span in pattern.spans() {
                ensure_nondefault_input_authorable(
                    session.project().identity(),
                    session.project().assets(),
                    span.input,
                )?;
            }
            validate_copied_events(session, &relative_events)?;
            session.insert_input_pattern_with_events(cursor, &pattern, &relative_events)?;
            session.set_cursor(cursor)?;
        }
        let session = self
            .session
            .as_ref()
            .expect("open session was checked before inserting input pattern");
        self.timeline_selection.select_range(session, cursor, end);
        self.execution_preview.clear();
        if let Some(error) = self.detach_incompatible_execution() {
            return Ok(format!(
                "Inserted copied input frames; private execution detached because the edited project no longer matches it: {error:#}"
            ));
        }
        Ok("Inserted copied input frames".to_owned())
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

fn validate_pattern_target(
    session: &TasEditorSession,
    witness: &TasInputPatternPasteWitness,
    operation: &str,
) -> Result<()> {
    if session.project_content_sha256() != witness.expected_project_sha256 {
        bail!("TAS project changed after this {operation} was requested; retry it");
    }
    if session.selected_branch_id() != witness.target_branch_id {
        bail!("selected TAS branch changed after this {operation} was requested; retry it");
    }
    if let Some(expected_cursor) = witness.expected_cursor
        && session.cursor() != expected_cursor
    {
        bail!("selected TAS cursor changed after this {operation} was requested; retry it");
    }
    if session
        .project()
        .branch_movie_sha256(&witness.target_branch_id)?
        != witness.target_movie_sha256
    {
        bail!("{operation} target branch changed after this operation was requested; retry it");
    }
    Ok(())
}
