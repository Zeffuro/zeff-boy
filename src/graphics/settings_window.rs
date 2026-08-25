use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::debug::{DebugWindowState, SettingsContext};
use crate::emu_backend::ActiveSystem;
use crate::settings::Settings;

use super::FrameError;
use super::gpu::GpuContext;
use super::tool_window::{ToolWindow, ToolWindowConfig, ToolWindowStyle};
use super::window_geometry::{SETTINGS_DEFAULT_SIZE, SETTINGS_MIN_SIZE};

pub(crate) struct SettingsRenderContext<'a> {
    pub(crate) settings: &'a mut Settings,
    pub(crate) state: &'a mut DebugWindowState,
    pub(crate) active_system: Option<ActiveSystem>,
    pub(crate) gb_hardware_mode_label: Option<&'a str>,
    pub(crate) is_pocket_camera: bool,
}

pub(super) struct SettingsWindow(ToolWindow);

impl SettingsWindow {
    pub(super) fn new(
        event_loop: &ActiveEventLoop,
        shared_gpu: &GpuContext,
        settings: &Settings,
    ) -> anyhow::Result<Self> {
        ToolWindow::new(
            event_loop,
            shared_gpu,
            ToolWindowConfig {
                title: "Settings",
                saved_size: settings.ui.settings_window_size,
                saved_position: settings.ui.settings_window_position,
                minimum: SETTINGS_MIN_SIZE,
                fallback: SETTINGS_DEFAULT_SIZE,
                maximized: settings.ui.settings_window_maximized,
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

    pub(super) fn render(&mut self, ctx: SettingsRenderContext<'_>) -> Result<(), FrameError> {
        let style = ToolWindowStyle::from(&*ctx.settings);
        self.0
            .render(style, "settings_root_ui", "settings egui pass", |ui| {
                crate::debug::draw_settings_content(
                    ui,
                    ctx.settings,
                    ctx.state,
                    &SettingsContext {
                        active_system: ctx.active_system,
                        gb_hardware_mode_label: ctx.gb_hardware_mode_label,
                        is_pocket_camera: ctx.is_pocket_camera,
                    },
                );
            })
    }
}
