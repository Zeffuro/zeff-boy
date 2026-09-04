use std::collections::HashMap;

use anyhow::{Result, bail};

use super::{TasEditorAction, TasEditorExecutionAvailability, TasEditorWindowState};
use crate::tas_project::{TasEditorExecutionOutcome, TasEditorFramebuffer};

pub(super) struct TasEditorExecutionPreview {
    branch_id: Option<String>,
    cursor: Option<u64>,
    requested_cursor: Option<u64>,
    frame: Option<TasEditorFramebuffer>,
    textures: HashMap<usize, egui::TextureHandle>,
}

impl TasEditorExecutionPreview {
    pub(super) fn new() -> Self {
        Self {
            branch_id: None,
            cursor: None,
            requested_cursor: None,
            frame: None,
            textures: HashMap::new(),
        }
    }

    pub(super) fn clear(&mut self) {
        self.branch_id = None;
        self.cursor = None;
        self.requested_cursor = None;
        self.frame = None;
        self.textures.clear();
    }

    fn install(&mut self, branch_id: String, outcome: TasEditorExecutionOutcome) {
        self.branch_id = Some(branch_id);
        self.cursor = Some(outcome.cursor);
        self.requested_cursor = Some(outcome.requested_cursor);
        self.frame = Some(outcome.framebuffer);
        self.textures.clear();
    }

    fn install_linked(&mut self, branch_id: String, cursor: u64, frame: TasEditorFramebuffer) {
        self.branch_id = Some(branch_id);
        self.cursor = Some(cursor);
        self.requested_cursor = Some(cursor);
        self.frame = Some(frame);
        self.textures.clear();
    }

    fn cursor(&self) -> Option<u64> {
        self.frame.as_ref()?;
        self.cursor
    }

    fn remaining_frames(&self) -> u64 {
        let Some(requested) = self.requested_cursor else {
            return 0;
        };
        requested.saturating_sub(self.cursor.unwrap_or(requested))
    }

    fn pending_target(&self) -> Option<u64> {
        (self.remaining_frames() > 0).then_some(self.requested_cursor?)
    }

    #[cfg(test)]
    pub(super) fn exact_frame(&self) -> Option<&TasEditorFramebuffer> {
        self.frame.as_ref()
    }

    #[cfg(test)]
    pub(super) fn texture_count(&self) -> usize {
        self.textures.len()
    }

    #[cfg(test)]
    pub(super) fn only_texture_id(&self) -> Option<egui::TextureId> {
        (self.textures.len() == 1)
            .then(|| self.textures.values().next().map(egui::TextureHandle::id))
            .flatten()
    }
}

impl TasEditorWindowState {
    pub(crate) fn install_linked_frame(
        &mut self,
        cursor: u64,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> Result<()> {
        let branch_id = self
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no TAS editor project is open"))?
            .selected_branch_id()
            .to_owned();
        let frame = TasEditorFramebuffer::from_rgba(width, height, rgba)?;
        self.execution_preview
            .install_linked(branch_id, cursor, frame);
        Ok(())
    }

    pub(super) fn execute_private_seek(&mut self, target_cursor: u64) -> Result<String> {
        let mut engine = self
            .execution_engine
            .take()
            .ok_or_else(|| anyhow::anyhow!("private TAS execution is not attached"))?;
        let result = match self.session.as_mut() {
            Some(session) => engine.seek(session, target_cursor),
            None => {
                self.execution_engine = Some(engine);
                bail!("open a TAS project first");
            }
        };
        self.execution_engine = Some(engine);
        let outcome = result?;
        let branch_id = self
            .session
            .as_ref()
            .expect("successful TAS execution requires an open session")
            .selected_branch_id()
            .to_owned();
        let message = execution_message(&outcome);
        self.execution_preview.install(branch_id, outcome);
        Ok(message)
    }
}

pub(super) fn draw_execution_panel(
    ui: &mut egui::Ui,
    availability: &TasEditorExecutionAvailability,
    cursor: u64,
    frame_count: u64,
    preview: &mut TasEditorExecutionPreview,
    actions: &mut Vec<TasEditorAction>,
) {
    let attached = matches!(availability, TasEditorExecutionAvailability::Ready);
    ui.group(|ui| {
        ui.horizontal_wrapped(|ui| {
            ui.strong("Playback");
            let (before_label, before_help) = if cursor < frame_count {
                (
                    "State before input",
                    "Show the emulator state immediately before the selected input row",
                )
            } else {
                (
                    "Show final state",
                    "Show the state after the final input row",
                )
            };
            if ui
                .add_enabled(attached, egui::Button::new(before_label))
                .on_hover_text(before_help)
                .clicked()
            {
                actions.push(TasEditorAction::ExecuteSeek(cursor));
            }

            let pending_target = preview.pending_target();
            let seek_target = pending_target
                .unwrap_or_else(|| selected_frame_playback_target(cursor, frame_count));
            let seek_label = pending_target.map_or_else(
                || "Run selected input".to_owned(),
                |target| format!("Continue to frame {target}"),
            );
            if ui
                .add_enabled(
                    attached && cursor < frame_count,
                    egui::Button::new(seek_label),
                )
                .on_hover_text("Apply the selected input row and show the resulting frame")
                .clicked()
            {
                actions.push(TasEditorAction::ExecuteSeek(seek_target));
            }
            if ui
                .add_enabled(attached, egui::Button::new("Run to end"))
                .on_hover_text("Run every input row in the selected branch")
                .clicked()
            {
                actions.push(TasEditorAction::ExecuteSeek(frame_count));
            }
        });

        match availability {
            TasEditorExecutionAvailability::Checking => {
                ui.small("Checking whether the loaded game can run this project…");
            }
            TasEditorExecutionAvailability::GameReady => {
                ui.small("A compatible game is loaded; open or create a project to begin.");
            }
            TasEditorExecutionAvailability::Ready => {
                ui.small("Ready — playback is isolated from the main emulator.");
            }
            TasEditorExecutionAvailability::Unavailable(reason) => {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    format!("Playback unavailable: {reason}"),
                );
                ui.small(
                    "Load the matching game in the main window. Editing and saving still work.",
                );
            }
        }
        draw_preview(ui, preview);
    });
}

pub(super) fn draw_linked_frame(ui: &mut egui::Ui, preview: &mut TasEditorExecutionPreview) {
    ui.group(|ui| {
        ui.strong("Loaded game");
        draw_preview(ui, preview);
    });
}

fn draw_preview(ui: &mut egui::Ui, preview: &mut TasEditorExecutionPreview) {
    let Some(frame) = preview.frame.as_ref() else {
        ui.small("No frame has been previewed yet.");
        return;
    };
    let cursor = preview.cursor().expect("preview frame has a cursor");
    let branch = preview
        .branch_id
        .as_deref()
        .expect("preview frame has a branch");
    let position = if cursor == 0 {
        "initial state before frame 0".to_owned()
    } else {
        format!("after input frame {}", cursor - 1)
    };
    ui.label(format!("Showing branch {branch}, {position}"));
    if let Some(target) = preview.pending_target() {
        ui.small(format!(
            "Running to frame {target}; {} frames remain. Continue to run the next chunk.",
            preview.remaining_frames()
        ));
    }

    let width = frame.width() as usize;
    let height = frame.height() as usize;
    let texture_manager = ui.ctx().tex_manager();
    let texture_manager_key = std::sync::Arc::as_ptr(&texture_manager) as usize;
    let texture = preview
        .textures
        .entry(texture_manager_key)
        .or_insert_with(|| {
            let image = egui::ColorImage::from_rgba_unmultiplied([width, height], frame.rgba());
            ui.ctx().load_texture(
                "tas_editor_private_preview",
                image,
                egui::TextureOptions::NEAREST,
            )
        });
    let scale = (ui.available_width() / width as f32).clamp(0.25, 2.0);
    let display_size = egui::vec2(width as f32 * scale, height as f32 * scale);
    ui.add(egui::Image::new((texture.id(), display_size)));
}

pub(super) fn selected_frame_playback_target(cursor: u64, frame_count: u64) -> u64 {
    cursor.saturating_add(1).min(frame_count)
}

fn execution_message(outcome: &TasEditorExecutionOutcome) -> String {
    let cache = outcome
        .cache_store_error
        .as_ref()
        .map_or_else(String::new, |error| {
            format!("; seek-cache write skipped: {error}")
        });
    if outcome.reached_target() {
        format!(
            "Preview reached cursor {} (executed {} frames){cache}",
            outcome.cursor, outcome.executed_frames
        )
    } else {
        format!(
            "Preview reached cursor {} of requested {} ({} frames remain; at most {} execute per request){cache}",
            outcome.cursor,
            outcome.requested_cursor,
            outcome.remaining_frames(),
            crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES
        )
    }
}
