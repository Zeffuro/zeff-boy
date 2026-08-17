use std::sync::Arc;

use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes, WindowId};

use crate::debug::{DebugDataRefs, DebugTab, DebugTabViewer, DebugUiActions, DebugWindowState};
use crate::settings::{Settings, VsyncMode};

use super::egui_integration::EguiRenderer;
use super::gpu::GpuContext;
use super::window_geometry::{
    DEBUGGER_DEFAULT_SIZE, DEBUGGER_MIN_SIZE, restored_position, restored_size,
};
use super::{AspectRatioMode, FrameError};

pub(crate) struct DebuggerRenderContext<'a> {
    pub(crate) data: DebugDataRefs<'a>,
    pub(crate) debug_windows: &'a mut DebugWindowState,
    pub(crate) settings: &'a Settings,
    pub(crate) dock_state: &'a mut egui_dock::DockState<DebugTab>,
}

pub(crate) struct DebuggerRenderResult {
    pub(crate) debug_actions: DebugUiActions,
}

pub(super) struct DebuggerWindow {
    window: Arc<Window>,
    gpu: GpuContext,
    egui: EguiRenderer,
    size: PhysicalSize<u32>,
}

impl DebuggerWindow {
    pub(super) fn new(
        event_loop: &ActiveEventLoop,
        shared_gpu: &GpuContext,
        settings: &Settings,
    ) -> anyhow::Result<Self> {
        let size = restored_size(
            settings.ui.debugger_window_size,
            DEBUGGER_MIN_SIZE,
            DEBUGGER_DEFAULT_SIZE,
        );
        let mut attrs = WindowAttributes::default()
            .with_title(format!("zeff-boy Debugger v{}", env!("CARGO_PKG_VERSION")))
            .with_inner_size(size)
            .with_min_inner_size(PhysicalSize::new(
                DEBUGGER_MIN_SIZE[0],
                DEBUGGER_MIN_SIZE[1],
            ));
        if let Some(position) =
            restored_position(event_loop, settings.ui.debugger_window_position, size)
        {
            attrs = attrs.with_position(position);
        }
        if let Some(icon) = super::Graphics::load_window_icon() {
            attrs = attrs.with_window_icon(Some(icon));
        }

        let window = Arc::new(event_loop.create_window(attrs)?);
        window.set_maximized(settings.ui.debugger_window_maximized);
        let size = window.inner_size();
        let gpu = shared_gpu.new_surface(
            window.clone(),
            size.width.max(1),
            size.height.max(1),
            VsyncMode::Off,
        )?;
        gpu.clear(wgpu::Color {
            r: 0.08,
            g: 0.08,
            b: 0.12,
            a: 1.0,
        });
        let egui = EguiRenderer::new(&window, &gpu.device, gpu.config.format)?;
        window.request_redraw();

        Ok(Self {
            window,
            gpu,
            egui,
            size,
        })
    }

    pub(super) fn id(&self) -> WindowId {
        self.window.id()
    }

    pub(super) fn window(&self) -> &Window {
        &self.window
    }

    pub(super) fn handle_event(&mut self, event: &WindowEvent) -> bool {
        self.egui.handle_event_with_repaint(&self.window, event).1
    }

    pub(super) fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.size = PhysicalSize::new(width, height);
        self.gpu.resize(width, height);
    }

    pub(super) fn render(
        &mut self,
        ctx: DebuggerRenderContext<'_>,
    ) -> Result<DebuggerRenderResult, FrameError> {
        let frame = match self.gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Err(FrameError::Timeout);
            }
            wgpu::CurrentSurfaceTexture::Outdated => return Err(FrameError::Outdated),
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Validation => {
                return Err(FrameError::Lost);
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.egui.begin_frame(&self.window);
        self.egui.apply_style(
            ctx.settings.ui.theme_preset,
            ctx.settings.ui.ui_density,
            ctx.settings.ui.debug_monospace_scale,
            ctx.settings.ui.effective_debug_colors(),
        );
        let target_ppp =
            self.window.scale_factor() as f32 * ctx.settings.ui.ui_scale.clamp(0.5, 3.0);
        if (self.egui.context().pixels_per_point() - target_ppp).abs() > 0.01 {
            self.egui.context().set_pixels_per_point(target_ppp);
        }

        let mut tab_viewer = DebugTabViewer {
            data: ctx.data,
            window_state: ctx.debug_windows,
            actions: DebugUiActions::none(),
            game_texture_id: None,
            game_native_size: (160, 144),
            aspect_ratio_mode: AspectRatioMode::IntegerScale,
            game_view_pixel_size: None,
        };
        let mut root_ui = egui::Ui::new(
            self.egui.context().clone(),
            egui::Id::new("debugger_root_ui"),
            egui::UiBuilder::new()
                .layer_id(egui::LayerId::background())
                .max_rect(self.egui.context().content_rect()),
        );
        egui::Panel::top("debugger_toolbar")
            .frame(
                egui::Frame::new()
                    .fill(self.egui.context().global_style().visuals.faint_bg_color)
                    .inner_margin(egui::Margin::symmetric(6, 3)),
            )
            .show(&mut root_ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.menu_button("Windows", |ui| {
                        for tab in DebugTab::all_tools() {
                            let open = crate::debug::is_tab_open(ctx.dock_state, tab);
                            if ui.selectable_label(open, tab.title()).clicked() {
                                crate::debug::activate_dock_tab(ctx.dock_state, tab);
                                ui.close();
                            }
                        }
                    });
                    ui.menu_button("Layout", |ui| {
                        for preset in crate::debug::DebugWorkspacePreset::ALL {
                            if ui.button(preset.label()).clicked() {
                                *ctx.dock_state = crate::debug::create_workspace_dock_state(
                                    crate::settings::DebugPresentation::GameAndDebugger,
                                    preset,
                                );
                                ui.close();
                            }
                        }
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(&mut root_ui, |ui| {
                egui_dock::DockArea::new(ctx.dock_state)
                    .secondary_button_on_modifier(false)
                    .style(super::egui_integration::dock_style(
                        self.egui.context(),
                        ctx.settings.ui.ui_density,
                    ))
                    .show_inside(ui, &mut tab_viewer);
            });
        let mut debug_actions = tab_viewer.actions;
        if let Some(tab) = debug_actions.focus_tab.take() {
            crate::debug::activate_dock_tab(ctx.dock_state, tab);
        }
        let output = self.egui.end_frame(&self.window);

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("debugger encoder"),
            });
        let (paint_jobs, screen_desc) = self.egui.prepare(&self.gpu, &mut encoder, &output);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("debugger egui pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.08,
                                g: 0.08,
                                b: 0.12,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            self.egui
                .render_to_pass(&mut pass, &paint_jobs, &screen_desc);
        }
        self.egui.cleanup(&output);
        self.gpu.queue.submit(Some(encoder.finish()));
        frame.present();

        Ok(DebuggerRenderResult { debug_actions })
    }
}
