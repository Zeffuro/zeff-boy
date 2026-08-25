use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::debug::PrinterViewerState;
use crate::settings::Settings;

use super::FrameError;
use super::gpu::GpuContext;
use super::tool_window::{ToolWindow, ToolWindowConfig, ToolWindowStyle};
use super::window_geometry::{PRINTER_DEFAULT_SIZE, PRINTER_MIN_SIZE};

pub(crate) struct PrinterRenderContext<'a> {
    pub(crate) settings: &'a Settings,
    pub(crate) state: &'a mut PrinterViewerState,
}

pub(super) struct PrinterWindow(ToolWindow);

impl PrinterWindow {
    pub(super) fn new(
        event_loop: &ActiveEventLoop,
        shared_gpu: &GpuContext,
        settings: &Settings,
    ) -> anyhow::Result<Self> {
        ToolWindow::new(
            event_loop,
            shared_gpu,
            ToolWindowConfig {
                title: "Game Boy Printer",
                saved_size: settings.ui.printer_window_size,
                saved_position: settings.ui.printer_window_position,
                minimum: PRINTER_MIN_SIZE,
                fallback: PRINTER_DEFAULT_SIZE,
                maximized: settings.ui.printer_window_maximized,
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

    pub(super) fn render(&mut self, ctx: PrinterRenderContext<'_>) -> Result<(), FrameError> {
        self.0.render(
            ToolWindowStyle::from(ctx.settings),
            "printer_root_ui",
            "printer egui pass",
            |ui| crate::debug::draw_printer_viewer_content(ui, ctx.state),
        )
    }
}
