use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::{debug::MenuAction, emu_backend::ActiveSystem, netplay::ui::Ui, settings::Settings};

use super::tool_window::{ToolWindow, ToolWindowConfig, ToolWindowStyle};
use super::{FrameError, Graphics};

pub(crate) struct NetplayRenderContext<'a> {
    pub(crate) settings: &'a Settings,
    pub(crate) state: &'a mut Ui,
    pub(crate) system: ActiveSystem,
}

pub(super) struct NetplayWindow(ToolWindow, f32);

impl Graphics {
    pub(crate) fn open_netplay_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        settings: &Settings,
    ) -> anyhow::Result<()> {
        if self.netplay_window.is_none() {
            let scale = settings.ui.ui_scale.clamp(0.5, 3.0);
            self.netplay_window = Some(NetplayWindow(
                ToolWindow::new_logical(
                    event_loop,
                    &self.gpu,
                    ToolWindowConfig {
                        title: "Netplay",
                        saved_size: [0, 0],
                        saved_position: None,
                        minimum: [360, 240],
                        fallback: [420, 340],
                        maximized: false,
                    },
                    scale,
                )?,
                scale,
            ));
        }
        Ok(())
    }

    pub(crate) fn close_netplay_window(&mut self) {
        self.netplay_window = None;
    }

    pub(crate) fn netplay_window_id(&self) -> Option<WindowId> {
        self.netplay_window.as_ref().map(|window| window.0.id())
    }

    pub(crate) fn netplay_window(&self) -> Option<&Window> {
        self.netplay_window.as_ref().map(|window| window.0.window())
    }

    pub(crate) fn netplay_handles_event(&mut self, event: &WindowEvent) -> bool {
        self.netplay_window
            .as_mut()
            .is_some_and(|window| window.0.handle_event(event))
    }

    pub(crate) fn resize_netplay_window(&mut self, width: u32, height: u32) {
        if let Some(window) = self.netplay_window.as_mut() {
            window.0.resize(width, height);
        }
    }

    pub(crate) fn render_netplay_window(
        &mut self,
        ctx: NetplayRenderContext<'_>,
    ) -> Result<Option<MenuAction>, FrameError> {
        let mut action = None;
        let window = self.netplay_window.as_mut().ok_or(FrameError::Lost)?;
        let scale = ctx.settings.ui.ui_scale.clamp(0.5, 3.0);
        if window.1 != scale {
            let minimum = winit::dpi::LogicalSize::new(360.0 * scale, 240.0 * scale);
            window.0.window().set_min_inner_size(Some(minimum));
            let minimum = minimum.to_physical::<u32>(window.0.window().scale_factor());
            let current = window.0.window().inner_size();
            let target = winit::dpi::PhysicalSize::new(
                current.width.max(minimum.width),
                current.height.max(minimum.height),
            );
            if target != current
                && let Some(size) = window.0.window().request_inner_size(target)
            {
                window.0.resize(size.width, size.height);
            }
            window.1 = scale;
        }
        window.0.render(
            ToolWindowStyle::from(ctx.settings),
            "netplay_root_ui",
            "netplay egui pass",
            |ui| action = ctx.state.draw_contents(ui, ctx.system),
        )?;
        Ok(action)
    }
}
