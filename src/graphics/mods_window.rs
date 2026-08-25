use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::debug::types::ModState;
use crate::settings::Settings;

use super::FrameError;
use super::gpu::GpuContext;
use super::tool_window::{ToolWindow, ToolWindowConfig, ToolWindowStyle};
use super::window_geometry::{MODS_DEFAULT_SIZE, MODS_MIN_SIZE};

pub(crate) struct ModsRenderContext<'a> {
    pub(crate) settings: &'a Settings,
    pub(crate) state: &'a mut ModState,
}

pub(super) struct ModsWindow(ToolWindow);

impl ModsWindow {
    pub(super) fn new(
        event_loop: &ActiveEventLoop,
        shared_gpu: &GpuContext,
        settings: &Settings,
    ) -> anyhow::Result<Self> {
        ToolWindow::new(
            event_loop,
            shared_gpu,
            ToolWindowConfig {
                title: "Mods",
                saved_size: settings.ui.mods_window_size,
                saved_position: settings.ui.mods_window_position,
                minimum: MODS_MIN_SIZE,
                fallback: MODS_DEFAULT_SIZE,
                maximized: settings.ui.mods_window_maximized,
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

    pub(super) fn render(&mut self, ctx: ModsRenderContext<'_>) -> Result<(), FrameError> {
        self.0.render(
            ToolWindowStyle::from(ctx.settings),
            "mods_root_ui",
            "mods egui pass",
            |ui| {
                crate::debug::draw_mods_content(ui, ctx.state);
            },
        )
    }
}
