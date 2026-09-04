use anyhow::{Result, bail};

use super::{TasEditorAction, TasEditorWindowState, input_clipboard::TasInputClipboardAction};
use crate::tas_project::{
    TasBranchDiff, TasBranchDiffLimits, TasBranchDiffSide, TasDigest, TasEditorSession,
    TasEventDiffKind, TasInputFrame,
};

const DIFF_ROW_HEIGHT: f32 = 22.0;
const DIFF_LIST_HEIGHT: f32 = 154.0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TasBranchDiffJumpAction {
    expected_project_sha256: TasDigest,
    source_branch_id: String,
    source_movie_sha256: TasDigest,
    cursor: u64,
}

#[derive(Clone, Debug)]
struct CachedBranchDiff {
    project_sha256: TasDigest,
    source_branch_id: String,
    target_branch_id: String,
    diff: TasBranchDiff,
}

pub(super) struct TasBranchDiffEditorState {
    source_branch_id: Option<String>,
    target_branch_id: Option<String>,
    cache: Option<CachedBranchDiff>,
}

impl TasBranchDiffEditorState {
    pub(super) fn new() -> Self {
        Self {
            source_branch_id: None,
            target_branch_id: None,
            cache: None,
        }
    }

    pub(super) fn clear(&mut self) {
        self.source_branch_id = None;
        self.target_branch_id = None;
        self.cache = None;
    }

    fn sync_source_and_target(&mut self, session: &TasEditorSession) {
        let source = session.selected_branch_id();
        if self.source_branch_id.as_deref() != Some(source) {
            self.source_branch_id = Some(source.to_owned());
            self.target_branch_id = default_target_branch_id(session);
            self.cache = None;
            return;
        }
        let target_is_valid = self
            .target_branch_id
            .as_deref()
            .is_some_and(|target| target != source && session.project().branch(target).is_some());
        if !target_is_valid {
            self.target_branch_id = default_target_branch_id(session);
            self.cache = None;
        }
    }

    fn set_target(&mut self, target_branch_id: String) {
        if self.target_branch_id.as_deref() != Some(&target_branch_id) {
            self.target_branch_id = Some(target_branch_id);
            self.cache = None;
        }
    }

    pub(super) fn diff(&mut self, session: &TasEditorSession) -> Result<&TasBranchDiff> {
        self.sync_source_and_target(session);
        let source_branch_id = self
            .source_branch_id
            .as_deref()
            .expect("open branch-diff panel has a source branch");
        let target_branch_id = self
            .target_branch_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("select a comparison branch"))?;
        let project_sha256 = session.project_content_sha256();
        let cache_is_current = self.cache.as_ref().is_some_and(|cache| {
            cache.project_sha256 == project_sha256
                && cache.source_branch_id == source_branch_id
                && cache.target_branch_id == target_branch_id
        });
        if !cache_is_current {
            let diff = session.project().diff_branches(
                source_branch_id,
                target_branch_id,
                TasBranchDiffLimits::default(),
            )?;
            self.cache = Some(CachedBranchDiff {
                project_sha256,
                source_branch_id: source_branch_id.to_owned(),
                target_branch_id: target_branch_id.to_owned(),
                diff,
            });
        }
        Ok(&self
            .cache
            .as_ref()
            .expect("branch-diff cache was just installed")
            .diff)
    }

    #[cfg(test)]
    pub(super) fn selected_target(&mut self, session: &TasEditorSession) -> Option<&str> {
        self.sync_source_and_target(session);
        self.target_branch_id.as_deref()
    }

    #[cfg(test)]
    pub(super) fn cached_diff(&self) -> Option<&TasBranchDiff> {
        self.cache.as_ref().map(|cache| &cache.diff)
    }
}

impl TasBranchDiffJumpAction {
    pub(super) fn new(
        expected_project_sha256: TasDigest,
        source_branch_id: String,
        source_movie_sha256: TasDigest,
        cursor: u64,
    ) -> Self {
        Self {
            expected_project_sha256,
            source_branch_id,
            source_movie_sha256,
            cursor,
        }
    }

    #[cfg(test)]
    pub(super) fn cursor(&self) -> u64 {
        self.cursor
    }
}

impl TasEditorWindowState {
    pub(super) fn apply_branch_diff_jump_action(
        &mut self,
        action: TasBranchDiffJumpAction,
    ) -> Result<String> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("open a TAS project first"))?;
        if session.project_content_sha256() != action.expected_project_sha256 {
            bail!("TAS project changed after this diff was computed; refresh the comparison");
        }
        let current_movie_sha256 = session
            .project()
            .branch_movie_sha256(&action.source_branch_id)?;
        if current_movie_sha256 != action.source_movie_sha256 {
            bail!("source TAS branch changed after this diff was computed; refresh the comparison");
        }
        let frame_count = session
            .project()
            .branch(&action.source_branch_id)
            .ok_or_else(|| anyhow::anyhow!("diff source branch no longer exists"))?
            .frame_count();
        if action.cursor > frame_count {
            bail!("diff cursor is past its source branch end; refresh the comparison");
        }
        session.select_branch_at_cursor(&action.source_branch_id, action.cursor)?;
        self.execution_preview.clear();
        Ok(format!(
            "Selected diff source branch {} at cursor {}",
            action.source_branch_id, action.cursor
        ))
    }
}

pub(super) fn draw_branch_diff_editor(
    ui: &mut egui::Ui,
    session: &TasEditorSession,
    state: &mut TasBranchDiffEditorState,
    actions: &mut Vec<TasEditorAction>,
) {
    let branch_count = session.project().branches().len();
    ui.collapsing("Branch diff", |ui| {
        if branch_count < 2 {
            ui.small("Create another branch to compare immutable movie snapshots.");
            return;
        }

        state.sync_source_and_target(session);
        let source_branch_id = session.selected_branch_id();
        let source_name = session.selected_branch().name();
        let target_branch_id = state
            .target_branch_id
            .as_deref()
            .expect("two branches provide a comparison target")
            .to_owned();
        let target_name = session
            .project()
            .branch(&target_branch_id)
            .expect("validated branch-diff target exists")
            .name()
            .to_owned();

        ui.horizontal_wrapped(|ui| {
            ui.label(format!("Source: {source_name} [{source_branch_id}]"));
            egui::ComboBox::from_id_salt("tas_branch_diff_target")
                .selected_text(format!("Compare with: {target_name} [{target_branch_id}]"))
                .show_ui(ui, |ui| {
                    for branch in session.project().branches() {
                        if branch.id() != source_branch_id
                            && ui
                                .selectable_label(
                                    branch.id() == target_branch_id,
                                    format!("{} [{}]", branch.name(), branch.id()),
                                )
                                .clicked()
                        {
                            state.set_target(branch.id().to_owned());
                        }
                    }
                });
        });
        ui.small("Comparison is read-only and does not change the selected branch, cursor, preview, or save state.");

        let diff = match state.diff(session) {
            Ok(diff) => diff,
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, format!("Cannot compare branches: {error:#}"));
                return;
            }
        };
        draw_diff_summary(ui, diff);
        draw_input_hunks(ui, diff, source_branch_id, session.project_content_sha256(), actions);
        draw_event_hunks(ui, diff, source_branch_id, session.project_content_sha256(), actions);
    });
}

fn default_target_branch_id(session: &TasEditorSession) -> Option<String> {
    let source = session.selected_branch();
    if let Some(parent) = source.parent().filter(|parent| {
        parent.branch_id != source.id() && session.project().branch(&parent.branch_id).is_some()
    }) {
        return Some(parent.branch_id.clone());
    }
    session
        .project()
        .branches()
        .iter()
        .find(|branch| branch.id() != source.id())
        .map(|branch| branch.id().to_owned())
}

fn draw_diff_summary(ui: &mut egui::Ui, diff: &TasBranchDiff) {
    ui.horizontal_wrapped(|ui| {
        ui.label(format!(
            "Frames: source {} / target {}",
            diff.source_frame_count, diff.target_frame_count
        ));
        ui.separator();
        ui.label(format!(
            "Retained hunks: {} input, {} event",
            diff.input_hunks.len(),
            diff.event_hunks.len()
        ));
        ui.separator();
        ui.label(format!(
            "Omitted hunks: {} input, {} event",
            diff.omitted_input_hunks, diff.omitted_event_hunks
        ));
    });
    if let Some(tail) = diff.timeline_tail {
        let longer = match tail.longer_side {
            TasBranchDiffSide::Source => "source",
            TasBranchDiffSide::Target => "target",
        };
        ui.small(format!(
            "Timeline tail: {longer} alone has frames {}..{}",
            tail.start,
            tail.start.saturating_add(tail.length)
        ));
    }
    if diff.is_identical() {
        ui.small("Movie snapshots are identical.");
    } else if diff.is_truncated() {
        ui.small("Only the bounded retained hunk rows are shown; omitted hunk counts are exact within the completed scan.");
    }
}

fn draw_input_hunks(
    ui: &mut egui::Ui,
    diff: &TasBranchDiff,
    source_branch_id: &str,
    project_sha256: TasDigest,
    actions: &mut Vec<TasEditorAction>,
) {
    ui.collapsing(format!("Input hunks ({})", diff.input_hunks.len()), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("tas_branch_diff_inputs")
            .max_height(DIFF_LIST_HEIGHT)
            .show_rows(ui, DIFF_ROW_HEIGHT, diff.input_hunks.len(), |ui, rows| {
                for row in rows {
                    let hunk = &diff.input_hunks[row];
                    ui.horizontal_wrapped(|ui| {
                        ui.monospace(format!(
                            "{}..{}",
                            hunk.start,
                            hunk.start.saturating_add(hunk.length)
                        ));
                        ui.label(format!("source {}", raw_input_summary(hunk.source_input)));
                        ui.label(format!("target {}", raw_input_summary(hunk.target_input)));
                        if ui.small_button("Jump to source").clicked() {
                            actions.push(TasEditorAction::JumpToBranchDiffHunk(
                                TasBranchDiffJumpAction::new(
                                    project_sha256,
                                    source_branch_id.to_owned(),
                                    diff.source_movie_sha256,
                                    hunk.start,
                                ),
                            ));
                        }
                        if ui.small_button("Copy source input").clicked() {
                            let action = TasInputClipboardAction::copy_constant(
                                project_sha256,
                                source_branch_id.to_owned(),
                                diff.source_movie_sha256,
                                hunk.start,
                                hunk.length,
                                hunk.source_input,
                            )
                            .expect("bounded branch-diff input hunks have nonzero valid lengths");
                            actions.push(TasEditorAction::InputClipboard(action));
                        }
                    });
                }
            });
        if diff.input_hunks.is_empty() {
            ui.small("No input-span differences in the shared timeline.");
        }
    });
}

fn draw_event_hunks(
    ui: &mut egui::Ui,
    diff: &TasBranchDiff,
    source_branch_id: &str,
    project_sha256: TasDigest,
    actions: &mut Vec<TasEditorAction>,
) {
    ui.collapsing(
        format!("Replay-event hunks ({})", diff.event_hunks.len()),
        |ui| {
            egui::ScrollArea::vertical()
                .id_salt("tas_branch_diff_events")
                .max_height(DIFF_LIST_HEIGHT)
                .show_rows(ui, DIFF_ROW_HEIGHT, diff.event_hunks.len(), |ui, rows| {
                    for row in rows {
                        let hunk = &diff.event_hunks[row];
                        ui.horizontal_wrapped(|ui| {
                            ui.monospace(format!("{}..{}", hunk.first_frame, hunk.last_frame));
                            ui.label(event_kind_label(hunk.kind));
                            ui.label(format!(
                                "source events {}..{}; target events {}..{}",
                                hunk.source_event_indices.start,
                                hunk.source_event_indices.end,
                                hunk.target_event_indices.start,
                                hunk.target_event_indices.end
                            ));
                            if ui.small_button("Jump to source").clicked() {
                                actions.push(TasEditorAction::JumpToBranchDiffHunk(
                                    TasBranchDiffJumpAction::new(
                                        project_sha256,
                                        source_branch_id.to_owned(),
                                        diff.source_movie_sha256,
                                        hunk.first_frame.min(diff.source_frame_count),
                                    ),
                                ));
                            }
                        });
                    }
                });
            if diff.event_hunks.is_empty() {
                ui.small("No canonical replay-event differences.");
            }
        },
    );
}

fn event_kind_label(kind: TasEventDiffKind) -> &'static str {
    match kind {
        TasEventDiffKind::Changed => "changed canonical event group",
        TasEventDiffKind::SourceOnly => "source-only canonical event group",
        TasEventDiffKind::TargetOnly => "target-only canonical event group",
    }
}

pub(super) fn raw_input_summary(input: TasInputFrame) -> String {
    let players = input
        .players
        .iter()
        .enumerate()
        .map(|(index, player)| {
            format!(
                "p{}=b{:02X}/d{:02X}",
                index + 1,
                player.buttons,
                player.dpad
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let zapper_position = input
        .zapper
        .screen_pos
        .map(|[x, y]| format!("{x},{y}"))
        .unwrap_or_else(|| "none".to_owned());
    let camera = match input.camera {
        crate::tas_project::TasCameraInput::None => "none".to_owned(),
        crate::tas_project::TasCameraInput::Blob(digest) => {
            format!("blob:{}", &digest.to_hex()[..12])
        }
    };
    format!(
        "{players}; zapper=e{}/t{}/h{}/pos={zapper_position}; tilt=0x{:08X}/0x{:08X}; camera={camera}",
        u8::from(input.zapper.enabled),
        u8::from(input.zapper.trigger),
        u8::from(input.zapper.hit),
        input.tilt_x_bits,
        input.tilt_y_bits,
    )
}
