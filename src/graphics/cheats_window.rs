use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::debug::types::CheatState;
use crate::settings::Settings;

use super::tool_window::{ToolWindow, ToolWindowConfig, ToolWindowStyle};
use super::window_geometry::{CHEATS_DEFAULT_SIZE, CHEATS_MIN_SIZE};
use super::{FrameError, GpuContext};

pub(crate) struct CheatsRenderContext<'a> {
    pub(crate) settings: &'a Settings,
    pub(crate) state: &'a mut CheatState,
}

pub(super) struct CheatsWindow(ToolWindow);

impl CheatsWindow {
    pub(super) fn new(
        event_loop: &ActiveEventLoop,
        shared_gpu: &GpuContext,
        settings: &Settings,
    ) -> anyhow::Result<Self> {
        ToolWindow::new(
            event_loop,
            shared_gpu,
            ToolWindowConfig {
                title: "Cheats",
                saved_size: settings.ui.cheats_window_size,
                saved_position: settings.ui.cheats_window_position,
                minimum: CHEATS_MIN_SIZE,
                fallback: CHEATS_DEFAULT_SIZE,
                maximized: settings.ui.cheats_window_maximized,
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

    pub(super) fn render(&mut self, ctx: CheatsRenderContext<'_>) -> Result<(), FrameError> {
        self.0.render(
            ToolWindowStyle::from(ctx.settings),
            "cheats_root_ui",
            "cheats egui pass",
            |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| crate::debug::draw_cheats_content(ui, ctx.state));
            },
        )
    }
}
