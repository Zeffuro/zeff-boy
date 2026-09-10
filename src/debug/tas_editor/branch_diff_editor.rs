use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

use anyhow::{Result, bail};

use super::{TasEditorAction, TasEditorWindowState, input_clipboard::TasInputClipboardAction};
use crate::tas_project::{
    TasBranchDiff, TasBranchDiffSide, TasDigest, TasEditorSession, TasEventDiffKind, TasInputFrame,
};

const DIFF_LIST_HEIGHT: f32 = 220.0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TasBranchDiffJumpAction {
    expected_project_sha256: TasDigest,
    source_branch_id: String,
    source_movie_sha256: TasDigest,
    cursor: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BranchDiffRequestKey {
    project_sha256: TasDigest,
    source_branch_id: String,
    target_branch_id: String,
}

#[derive(Clone, Debug)]
struct CachedBranchDiff {
    key: BranchDiffRequestKey,
    diff: TasBranchDiff,
}

#[derive(Clone, Debug)]
struct FailedBranchDiff {
    key: BranchDiffRequestKey,
    message: String,
}

enum BranchDiffWorkerResult {
    Completed(Result<TasBranchDiff>),
    Cancelled,
}

struct InFlightBranchDiff {
    key: BranchDiffRequestKey,
    generation: u64,
    cancellation: Arc<AtomicBool>,
    task: JoinHandle<BranchDiffWorkerResult>,
}

pub(super) enum TasBranchDiffPresentation<'a> {
    NoTarget,
    Pending,
    Failed(&'a str),
    Ready(&'a TasBranchDiff),
}

pub(super) struct TasBranchDiffEditorState {
    source_branch_id: Option<String>,
    target_branch_id: Option<String>,
    request_key: Option<BranchDiffRequestKey>,
    cache: Option<CachedBranchDiff>,
    failure: Option<FailedBranchDiff>,
    in_flight: Option<InFlightBranchDiff>,
    generation: u64,
}

impl TasBranchDiffEditorState {
    pub(super) fn new() -> Self {
        Self {
            source_branch_id: None,
            target_branch_id: None,
            request_key: None,
            cache: None,
            failure: None,
            in_flight: None,
            generation: 0,
        }
    }

    pub(super) fn clear(&mut self) {
        self.source_branch_id = None;
        self.target_branch_id = None;
        self.request_key = None;
        self.cache = None;
        self.failure = None;
        self.generation = self.generation.wrapping_add(1);
        self.request_cancellation();
    }

    fn sync_source_and_target(&mut self, session: &TasEditorSession) {
        let source = session.selected_branch_id();
        if self.source_branch_id.as_deref() != Some(source) {
            self.source_branch_id = Some(source.to_owned());
            self.target_branch_id = default_target_branch_id(session);
        } else {
            let target_is_valid = self.target_branch_id.as_deref().is_some_and(|target| {
                target != source && session.project().branch(target).is_some()
            });
            if !target_is_valid {
                self.target_branch_id = default_target_branch_id(session);
            }
        }
        self.update_request_key(session);
    }

    fn set_target(&mut self, session: &TasEditorSession, target_branch_id: String) {
        if self.target_branch_id.as_deref() != Some(&target_branch_id) {
            self.target_branch_id = Some(target_branch_id);
            self.update_request_key(session);
        }
    }

    fn update_request_key(&mut self, session: &TasEditorSession) {
        let key = self
            .source_branch_id
            .as_ref()
            .zip(self.target_branch_id.as_ref())
            .map(
                |(source_branch_id, target_branch_id)| BranchDiffRequestKey {
                    project_sha256: session.project_content_sha256(),
                    source_branch_id: source_branch_id.clone(),
                    target_branch_id: target_branch_id.clone(),
                },
            );
        if self.request_key != key {
            self.request_key = key;
            self.generation = self.generation.wrapping_add(1);
            self.request_cancellation();
        }
    }

    fn request_cancellation(&self) {
        if let Some(in_flight) = &self.in_flight {
            in_flight.cancellation.store(true, Ordering::Release);
        }
    }

    fn poll(&mut self) {
        let Some(in_flight) = self.in_flight.as_ref() else {
            return;
        };
        if !in_flight.task.is_finished() {
            return;
        }
        let in_flight = self
            .in_flight
            .take()
            .expect("finished TAS branch-diff task disappeared");
        let result = match in_flight.task.join() {
            Ok(result) => result,
            Err(_) => BranchDiffWorkerResult::Completed(Err(anyhow::anyhow!(
                "background TAS branch comparison panicked"
            ))),
        };
        let is_current = self.request_key.as_ref() == Some(&in_flight.key)
            && self.generation == in_flight.generation;
        if !is_current {
            return;
        }
        match result {
            BranchDiffWorkerResult::Completed(Ok(diff)) => {
                self.cache = Some(CachedBranchDiff {
                    key: in_flight.key,
                    diff,
                });
                self.failure = None;
            }
            BranchDiffWorkerResult::Completed(Err(error)) => {
                self.failure = Some(FailedBranchDiff {
                    key: in_flight.key,
                    message: format!("{error:#}"),
                });
                self.cache = None;
            }
            BranchDiffWorkerResult::Cancelled => {}
        }
    }

    fn start_if_needed(&mut self, session: &TasEditorSession, ctx: &egui::Context) {
        if self.in_flight.is_some()
            || self
                .cache
                .as_ref()
                .is_some_and(|cache| self.request_key.as_ref() == Some(&cache.key))
            || self
                .failure
                .as_ref()
                .is_some_and(|failure| self.request_key.as_ref() == Some(&failure.key))
        {
            return;
        }
        let Some(key) = self.request_key.clone() else {
            return;
        };
        let snapshot = match session
            .project()
            .branch_diff_snapshot_from_validated(&key.source_branch_id, &key.target_branch_id)
        {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.failure = Some(FailedBranchDiff {
                    key,
                    message: format!("{error:#}"),
                });
                return;
            }
        };
        let cancellation = Arc::new(AtomicBool::new(false));
        let worker_cancellation = Arc::clone(&cancellation);
        let worker_ctx = ctx.clone();
        match std::thread::Builder::new()
            .name("tas-branch-diff".to_owned())
            .spawn(move || {
                let result = match snapshot.diff(&worker_cancellation) {
                    Ok(Some(diff)) => BranchDiffWorkerResult::Completed(Ok(diff)),
                    Ok(None) => BranchDiffWorkerResult::Cancelled,
                    Err(error) => BranchDiffWorkerResult::Completed(Err(error)),
                };
                worker_ctx.request_repaint();
                result
            }) {
            Ok(task) => {
                self.in_flight = Some(InFlightBranchDiff {
                    key,
                    generation: self.generation,
                    cancellation,
                    task,
                });
            }
            Err(error) => {
                self.failure = Some(FailedBranchDiff {
                    key,
                    message: format!("could not start background TAS branch comparison: {error}"),
                });
            }
        }
    }

    fn presentation(&self) -> TasBranchDiffPresentation<'_> {
        let Some(key) = self.request_key.as_ref() else {
            return TasBranchDiffPresentation::NoTarget;
        };
        if let Some(cache) = self.cache.as_ref().filter(|cache| &cache.key == key) {
            return TasBranchDiffPresentation::Ready(&cache.diff);
        }
        if let Some(failure) = self.failure.as_ref().filter(|failure| &failure.key == key) {
            return TasBranchDiffPresentation::Failed(&failure.message);
        }
        TasBranchDiffPresentation::Pending
    }

    pub(super) fn refresh<'a>(
        &'a mut self,
        session: &TasEditorSession,
        ctx: &egui::Context,
        request_if_missing: bool,
    ) -> TasBranchDiffPresentation<'a> {
        self.sync_source_and_target(session);
        self.poll();
        if request_if_missing {
            self.start_if_needed(session, ctx);
        }
        if self.in_flight.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        self.presentation()
    }

    #[cfg(test)]
    pub(super) fn selected_target(&mut self, session: &TasEditorSession) -> Option<&str> {
        self.sync_source_and_target(session);
        self.target_branch_id.as_deref()
    }

    #[cfg(test)]
    pub(super) fn cached_diff(&self) -> Option<&TasBranchDiff> {
        self.cache
            .as_ref()
            .filter(|cache| self.request_key.as_ref() == Some(&cache.key))
            .map(|cache| &cache.diff)
    }
}

impl Drop for TasBranchDiffEditorState {
    fn drop(&mut self) {
        self.request_cancellation();
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
    let _ = state.refresh(session, ui.ctx(), false);
    let branch_count = session.project().branches().len();
    ui.collapsing("Branch diff", |ui| {
        if branch_count < 2 {
            ui.small("Create another branch to compare immutable movie snapshots.");
            return;
        }

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

        ui.label("Source");
        ui.add(
            egui::Label::new(format!("{source_name} [{source_branch_id}]")).wrap(),
        );
        ui.label("Target");
        ui.add(
            egui::Label::new(format!("{target_name} [{target_branch_id}]")).wrap(),
        );
        let selector_width = ui.available_width();
        egui::ComboBox::from_id_salt("tas_branch_diff_target")
            .width(selector_width)
            .selected_text("Change comparison branch…")
            .show_ui(ui, |ui| {
                ui.set_max_width(selector_width);
                for branch in session.project().branches() {
                    if branch.id() != source_branch_id
                        && ui
                            .add(
                                egui::Button::selectable(
                                    branch.id() == target_branch_id,
                                    format!("{}\n[{}]", branch.name(), branch.id()),
                                )
                                .wrap(),
                            )
                            .clicked()
                    {
                        state.set_target(session, branch.id().to_owned());
                    }
                }
            });
        ui.small("Comparison is read-only and does not change the selected branch, cursor, preview, or save state.");

        let diff = match state.refresh(session, ui.ctx(), true) {
            TasBranchDiffPresentation::NoTarget => {
                ui.small("Select a comparison branch.");
                return;
            }
            TasBranchDiffPresentation::Pending => {
                ui.small("Comparing branches…");
                return;
            }
            TasBranchDiffPresentation::Failed(error) => {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Cannot compare branches: {error}"),
                );
                return;
            }
            TasBranchDiffPresentation::Ready(diff) => diff,
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
    ui.add(
        egui::Label::new(format!(
            "Frames: source {} / target {}",
            diff.source_frame_count, diff.target_frame_count
        ))
        .wrap(),
    );
    ui.add(
        egui::Label::new(format!(
            "Retained hunks: {} input, {} event",
            diff.input_hunks.len(),
            diff.event_hunks.len()
        ))
        .wrap(),
    );
    ui.add(
        egui::Label::new(format!(
            "Omitted hunks: {} input, {} event",
            diff.omitted_input_hunks, diff.omitted_event_hunks
        ))
        .wrap(),
    );
    if let Some(tail) = diff.timeline_tail {
        let longer = match tail.longer_side {
            TasBranchDiffSide::Source => "source",
            TasBranchDiffSide::Target => "target",
        };
        ui.add(
            egui::Label::new(format!(
                "Timeline tail: {longer} alone has frames {}..{}",
                tail.start,
                tail.start.saturating_add(tail.length)
            ))
            .wrap(),
        );
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
            .min_scrolled_height(DIFF_LIST_HEIGHT)
            .show(ui, |ui| {
                for hunk in &diff.input_hunks {
                    ui.group(|ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!(
                                    "Frames {}..{}",
                                    hunk.start,
                                    hunk.start.saturating_add(hunk.length)
                                ))
                                .monospace(),
                            )
                            .wrap(),
                        );
                        draw_input_side(ui, "Source input", hunk.source_input);
                        draw_input_side(ui, "Target input", hunk.target_input);
                        ui.horizontal_wrapped(|ui| {
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
                                .expect(
                                    "bounded branch-diff input hunks have nonzero valid lengths",
                                );
                                actions.push(TasEditorAction::InputClipboard(action));
                            }
                        });
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
                .min_scrolled_height(DIFF_LIST_HEIGHT)
                .show(ui, |ui| {
                    for hunk in &diff.event_hunks {
                        ui.group(|ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!(
                                        "Frames {}..{}",
                                        hunk.first_frame, hunk.last_frame
                                    ))
                                    .monospace(),
                                )
                                .wrap(),
                            );
                            ui.add(egui::Label::new(event_kind_label(hunk.kind)).wrap());
                            ui.add(
                                egui::Label::new(format!(
                                    "Source events {}..{}",
                                    hunk.source_event_indices.start, hunk.source_event_indices.end,
                                ))
                                .wrap(),
                            );
                            ui.add(
                                egui::Label::new(format!(
                                    "Target events {}..{}",
                                    hunk.target_event_indices.start, hunk.target_event_indices.end,
                                ))
                                .wrap(),
                            );
                            ui.horizontal_wrapped(|ui| {
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
                        });
                    }
                });
            if diff.event_hunks.is_empty() {
                ui.small("No canonical replay-event differences.");
            }
        },
    );
}

fn draw_input_side(ui: &mut egui::Ui, label: &str, input: TasInputFrame) {
    ui.strong(label);
    for summary in branch_diff_input_summary(input) {
        ui.add(egui::Label::new(summary).wrap());
    }
}

fn branch_diff_input_summary(input: TasInputFrame) -> [String; 5] {
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
    let coleco = coleco_input_summary(input);
    let zapper_position = input
        .zapper
        .screen_pos
        .map(|[x, y]| format!("{x},{y}"))
        .unwrap_or_else(|| "none".to_owned());
    let zapper = format!(
        "zapper=e{}/t{}/h{}/pos={zapper_position}",
        u8::from(input.zapper.enabled),
        u8::from(input.zapper.trigger),
        u8::from(input.zapper.hit),
    );
    let tilt = format!(
        "tilt=0x{:08X}/0x{:08X}",
        input.tilt_x_bits, input.tilt_y_bits,
    );
    let camera = match input.camera {
        crate::tas_project::TasCameraInput::None => "camera=none".to_owned(),
        crate::tas_project::TasCameraInput::Blob(digest) => {
            let digest = digest.to_hex();
            format!(
                "camera=blob:\n{} {} {} {}",
                &digest[..16],
                &digest[16..32],
                &digest[32..48],
                &digest[48..],
            )
        }
    };
    [players, coleco, zapper, tilt, camera]
}

fn coleco_input_summary(input: TasInputFrame) -> String {
    let controllers = input
        .coleco
        .iter()
        .enumerate()
        .map(|(index, controller)| {
            format!(
                "p{}=u{}/r{}/d{}/l{}/fire-l{}/fire-r{}/key={}",
                index + 1,
                u8::from(controller.up),
                u8::from(controller.right),
                u8::from(controller.down),
                u8::from(controller.left),
                u8::from(controller.left_button),
                u8::from(controller.right_button),
                super::coleco_input::keypad_label(controller.keypad),
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("coleco={controllers}")
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
        "{players}; {}; zapper=e{}/t{}/h{}/pos={zapper_position}; tilt=0x{:08X}/0x{:08X}; camera={camera}",
        coleco_input_summary(input),
        u8::from(input.zapper.enabled),
        u8::from(input.zapper.trigger),
        u8::from(input.zapper.hit),
        input.tilt_x_bits,
        input.tilt_y_bits,
    )
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        thread::JoinHandle,
    };

    use super::*;

    fn key(value: u8) -> BranchDiffRequestKey {
        BranchDiffRequestKey {
            project_sha256: TasDigest([value; 32]),
            source_branch_id: "source".to_owned(),
            target_branch_id: "target".to_owned(),
        }
    }

    fn diff(value: u8) -> TasBranchDiff {
        TasBranchDiff {
            source_movie_sha256: TasDigest([value; 32]),
            target_movie_sha256: TasDigest([value; 32]),
            source_frame_count: 0,
            target_frame_count: 0,
            input_hunks: Vec::new(),
            timeline_tail: None,
            event_hunks: Vec::new(),
            omitted_input_hunks: 0,
            omitted_event_hunks: 0,
        }
    }

    fn held_task(
        receiver: mpsc::Receiver<()>,
        result: BranchDiffWorkerResult,
    ) -> JoinHandle<BranchDiffWorkerResult> {
        std::thread::Builder::new()
            .name("tas-branch-diff-test".to_owned())
            .spawn(move || {
                receiver.recv().unwrap();
                result
            })
            .unwrap()
    }

    fn poll_until_drained(state: &mut TasBranchDiffEditorState) {
        for _ in 0..1_000 {
            state.poll();
            if state.in_flight.is_none() {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("held TAS branch-diff worker did not finish");
    }

    #[test]
    fn superseded_worker_is_held_then_drained_without_installing_a_late_result() {
        let mut state = TasBranchDiffEditorState::new();
        let original_key = key(1);
        let replacement_key = key(2);
        let (sender, receiver) = mpsc::channel();
        let cancellation = Arc::new(AtomicBool::new(false));
        state.request_key = Some(original_key.clone());
        state.generation = 1;
        state.in_flight = Some(InFlightBranchDiff {
            key: original_key,
            generation: state.generation,
            cancellation: Arc::clone(&cancellation),
            task: held_task(receiver, BranchDiffWorkerResult::Completed(Ok(diff(1)))),
        });

        state.request_key = Some(replacement_key);
        state.generation += 1;
        state.request_cancellation();
        assert!(cancellation.load(Ordering::Acquire));
        state.poll();
        assert!(state.in_flight.is_some());
        assert!(matches!(
            state.presentation(),
            TasBranchDiffPresentation::Pending
        ));

        sender.send(()).unwrap();
        poll_until_drained(&mut state);
        assert!(state.cache.is_none());
        assert!(state.failure.is_none());
        assert!(matches!(
            state.presentation(),
            TasBranchDiffPresentation::Pending
        ));
    }

    #[test]
    fn clear_cancels_but_keeps_the_one_worker_until_no_target_result_is_drained() {
        let mut state = TasBranchDiffEditorState::new();
        let (sender, receiver) = mpsc::channel();
        let cancellation = Arc::new(AtomicBool::new(false));
        state.request_key = Some(key(3));
        state.generation = 1;
        state.in_flight = Some(InFlightBranchDiff {
            key: key(3),
            generation: state.generation,
            cancellation: Arc::clone(&cancellation),
            task: held_task(receiver, BranchDiffWorkerResult::Completed(Ok(diff(3)))),
        });

        state.clear();
        assert!(cancellation.load(Ordering::Acquire));
        assert!(state.in_flight.is_some());
        assert!(matches!(
            state.presentation(),
            TasBranchDiffPresentation::NoTarget
        ));

        sender.send(()).unwrap();
        poll_until_drained(&mut state);
        assert!(state.cache.is_none());
        assert!(state.failure.is_none());
    }

    #[test]
    fn current_worker_failure_is_cached_and_pending_never_has_a_diff() {
        let mut state = TasBranchDiffEditorState::new();
        let (sender, receiver) = mpsc::channel();
        let current_key = key(4);
        state.request_key = Some(current_key.clone());
        state.generation = 4;
        state.in_flight = Some(InFlightBranchDiff {
            key: current_key,
            generation: state.generation,
            cancellation: Arc::new(AtomicBool::new(false)),
            task: held_task(
                receiver,
                BranchDiffWorkerResult::Completed(Err(anyhow::anyhow!("worker failed"))),
            ),
        });
        assert!(matches!(
            state.presentation(),
            TasBranchDiffPresentation::Pending
        ));

        sender.send(()).unwrap();
        poll_until_drained(&mut state);
        assert!(matches!(
            state.presentation(),
            TasBranchDiffPresentation::Failed("worker failed")
        ));
        assert!(state.cache.is_none());
    }

    #[test]
    fn current_worker_panic_is_cached_as_a_terminal_error() {
        let mut state = TasBranchDiffEditorState::new();
        state.request_key = Some(key(5));
        state.generation = 5;
        state.in_flight = Some(InFlightBranchDiff {
            key: key(5),
            generation: state.generation,
            cancellation: Arc::new(AtomicBool::new(false)),
            task: std::thread::Builder::new()
                .name("tas-branch-diff-panic-test".to_owned())
                .spawn(|| panic!("expected branch-diff worker panic"))
                .unwrap(),
        });

        poll_until_drained(&mut state);
        assert!(matches!(
            state.presentation(),
            TasBranchDiffPresentation::Failed(message) if message.contains("panicked")
        ));
    }

    #[test]
    fn keyed_snapshot_failure_is_not_retried_on_refresh() {
        let (_root, window) = super::super::tests::state_with_project(4);
        let session = window.session.as_ref().unwrap();
        let context = egui::Context::default();
        let mut state = TasBranchDiffEditorState::new();
        state.request_key = Some(BranchDiffRequestKey {
            project_sha256: session.project_content_sha256(),
            source_branch_id: "missing".to_owned(),
            target_branch_id: "target".to_owned(),
        });

        state.start_if_needed(session, &context);
        let message = match state.presentation() {
            TasBranchDiffPresentation::Failed(message) => message.to_owned(),
            _ => panic!("missing branch snapshot did not fail"),
        };
        state.start_if_needed(session, &context);
        assert!(state.in_flight.is_none());
        assert!(matches!(
            state.presentation(),
            TasBranchDiffPresentation::Failed(current) if current == message
        ));
    }

    #[test]
    fn input_summaries_distinguish_both_coleco_controllers_fire_and_keypad() {
        let input = TasInputFrame {
            coleco: [
                crate::tas_project::TasColecoControllerInput {
                    up: true,
                    left_button: true,
                    keypad: crate::tas_project::TasColecoKeypadKey::Star,
                    ..crate::tas_project::TasColecoControllerInput::default()
                },
                crate::tas_project::TasColecoControllerInput {
                    right: true,
                    right_button: true,
                    keypad: crate::tas_project::TasColecoKeypadKey::Pound,
                    ..crate::tas_project::TasColecoControllerInput::default()
                },
            ],
            ..TasInputFrame::default()
        };
        let expected =
            "coleco=p1=u1/r0/d0/l0/fire-l1/fire-r0/key=* p2=u0/r1/d0/l0/fire-l0/fire-r1/key=#";

        assert_eq!(branch_diff_input_summary(input)[1], expected);
        assert!(raw_input_summary(input).contains(expected));
    }
}
