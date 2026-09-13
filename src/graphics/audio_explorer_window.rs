use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::debug::AudioDiscoveryState;
use crate::settings::Settings;

use super::FrameError;
use super::gpu::GpuContext;
use super::tool_window::{ToolWindow, ToolWindowConfig, ToolWindowStyle};
use super::window_geometry::{AUDIO_EXPLORER_DEFAULT_SIZE, AUDIO_EXPLORER_MIN_SIZE};

pub(crate) struct AudioExplorerRenderContext<'a> {
    pub(crate) settings: &'a Settings,
    pub(crate) state: &'a mut AudioDiscoveryState,
}

pub(super) struct AudioExplorerWindow(ToolWindow);

impl AudioExplorerWindow {
    pub(super) fn new(
        event_loop: &ActiveEventLoop,
        shared_gpu: &GpuContext,
        settings: &Settings,
    ) -> anyhow::Result<Self> {
        ToolWindow::new(
            event_loop,
            shared_gpu,
            ToolWindowConfig {
                title: "Audio Explorer",
                saved_size: settings.ui.audio_explorer_window_size,
                saved_position: settings.ui.audio_explorer_window_position,
                minimum: AUDIO_EXPLORER_MIN_SIZE,
                fallback: AUDIO_EXPLORER_DEFAULT_SIZE,
                maximized: settings.ui.audio_explorer_window_maximized,
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

    pub(super) fn render(&mut self, ctx: AudioExplorerRenderContext<'_>) -> Result<(), FrameError> {
        self.0.render(
            ToolWindowStyle::from(ctx.settings),
            "audio_explorer_root_ui",
            "audio explorer egui pass",
            |ui| crate::debug::draw_audio_explorer(ui, ctx.state),
        )
    }
}
