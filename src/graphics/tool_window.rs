use std::sync::Arc;

use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes, WindowId};

use crate::settings::{DebugColors, Settings, UiDensity, UiThemePreset, VsyncMode};

use super::FrameError;
use super::egui_integration::EguiRenderer;
use super::gpu::GpuContext;
use super::window_geometry::{restored_position, restored_size};

pub(super) struct ToolWindow {
    window: Arc<Window>,
    gpu: GpuContext,
    egui: EguiRenderer,
}

pub(super) struct ToolWindowConfig {
    pub(super) title: &'static str,
    pub(super) saved_size: [u32; 2],
    pub(super) saved_position: Option<[i32; 2]>,
    pub(super) minimum: [u32; 2],
    pub(super) fallback: [u32; 2],
    pub(super) maximized: bool,
}

#[derive(Clone, Copy)]
pub(super) struct ToolWindowStyle {
    theme_preset: UiThemePreset,
    density: UiDensity,
    debug_monospace_scale: f32,
    debug_colors: DebugColors,
    ui_scale: f32,
}

impl From<&Settings> for ToolWindowStyle {
    fn from(settings: &Settings) -> Self {
        Self {
            theme_preset: settings.ui.theme_preset,
            density: settings.ui.ui_density,
            debug_monospace_scale: settings.ui.debug_monospace_scale,
            debug_colors: settings.ui.effective_debug_colors(),
            ui_scale: settings.ui.ui_scale,
        }
    }
}

impl ToolWindow {
    pub(super) fn new(
        event_loop: &ActiveEventLoop,
        shared_gpu: &GpuContext,
        config: ToolWindowConfig,
    ) -> anyhow::Result<Self> {
        let size = restored_size(config.saved_size, config.minimum, config.fallback);
        let mut attrs = WindowAttributes::default()
            .with_title(format!(
                "zeff-boy {} v{}",
                config.title,
                env!("CARGO_PKG_VERSION")
            ))
            .with_inner_size(size)
            .with_min_inner_size(PhysicalSize::new(config.minimum[0], config.minimum[1]));
        if let Some(position) = restored_position(event_loop, config.saved_position, size) {
            attrs = attrs.with_position(position);
        }
        if let Some(icon) = super::Graphics::load_window_icon() {
            attrs = attrs.with_window_icon(Some(icon));
        }

        let window = Arc::new(event_loop.create_window(attrs)?);
        window.set_maximized(config.maximized);
        let size = window.inner_size();
        let gpu = shared_gpu.new_surface(
            window.clone(),
            size.width.max(1),
            size.height.max(1),
            VsyncMode::On,
        )?;
        gpu.clear(wgpu::Color {
            r: 0.08,
            g: 0.08,
            b: 0.12,
            a: 1.0,
        });
        let egui = EguiRenderer::new(&window, &gpu.device, gpu.config.format)?;
        window.request_redraw();
        Ok(Self { window, gpu, egui })
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
        if width != 0 && height != 0 {
            self.gpu.resize(width, height);
        }
    }

    pub(super) fn render(
        &mut self,
        style: ToolWindowStyle,
        root_id: &'static str,
        label: &'static str,
        draw: impl FnOnce(&mut egui::Ui),
    ) -> Result<(), FrameError> {
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
            style.theme_preset,
            style.density,
            style.debug_monospace_scale,
            style.debug_colors,
        );
        let target_ppp = self.window.scale_factor() as f32 * style.ui_scale.clamp(0.5, 3.0);
        if (self.egui.context().pixels_per_point() - target_ppp).abs() > 0.01 {
            self.egui.context().set_pixels_per_point(target_ppp);
        }

        let mut root_ui = egui::Ui::new(
            self.egui.context().clone(),
            egui::Id::new(root_id),
            egui::UiBuilder::new()
                .layer_id(egui::LayerId::background())
                .max_rect(self.egui.context().content_rect()),
        );
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::same(8)))
            .show(&mut root_ui, draw);
        let output = self.egui.end_frame(&self.window);

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) });
        let (paint_jobs, screen_desc) = self.egui.prepare(&self.gpu, &mut encoder, &output);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(label),
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
        self.egui
            .submit_and_cleanup(&self.gpu.queue, encoder, &output);
        frame.present();
        Ok(())
    }
}
