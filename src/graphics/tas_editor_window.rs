use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::debug::{TasEditorHostRequest, TasEditorWindowState, draw_tas_editor_content};
use crate::settings::Settings;

use super::FrameError;
use super::gpu::GpuContext;
use super::tool_window::{ToolWindow, ToolWindowConfig, ToolWindowStyle};
use super::window_geometry::{TAS_EDITOR_DEFAULT_SIZE, TAS_EDITOR_MIN_SIZE};

pub(crate) struct TasEditorRenderContext<'a> {
    pub(crate) settings: &'a Settings,
    pub(crate) state: &'a mut TasEditorWindowState,
}

pub(crate) struct TasEditorRenderResult {
    pub(crate) host_request: Option<TasEditorHostRequest>,
}

pub(super) struct TasEditorWindow(ToolWindow);

impl TasEditorWindow {
    pub(super) fn new(
        event_loop: &ActiveEventLoop,
        shared_gpu: &GpuContext,
    ) -> anyhow::Result<Self> {
        ToolWindow::new(
            event_loop,
            shared_gpu,
            ToolWindowConfig {
                title: "TAS Editor",
                saved_size: [0, 0],
                saved_position: None,
                minimum: TAS_EDITOR_MIN_SIZE,
                fallback: TAS_EDITOR_DEFAULT_SIZE,
                maximized: false,
            },
        )
        .map(Self)
    }

    pub(super) fn id(&self) -> WindowId {
        self.0.id()
    }

    pub(super) fn window(&self) -> &Window {
        self.0.window()
    }

    pub(super) fn handle_event(&mut self, event: &WindowEvent) -> bool {
        self.0.handle_event(event)
    }

    pub(super) fn resize(&mut self, width: u32, height: u32) {
        self.0.resize(width, height);
    }

    pub(super) fn render(
        &mut self,
        ctx: TasEditorRenderContext<'_>,
    ) -> Result<TasEditorRenderResult, FrameError> {
        let mut host_request = None;
        self.0.render(
            ToolWindowStyle::from(ctx.settings),
            "tas_editor_root_ui",
            "tas editor egui pass",
            |ui| {
                host_request = draw_tas_editor_content(ui, ctx.state);
            },
        )?;
        Ok(TasEditorRenderResult { host_request })
    }
}
