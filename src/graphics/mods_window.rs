use std::sync::Arc;

use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes, WindowId};

use crate::debug::types::ModState;
use crate::settings::{Settings, VsyncMode};

use super::FrameError;
use super::egui_integration::EguiRenderer;
use super::gpu::GpuContext;
use super::window_geometry::{MODS_DEFAULT_SIZE, MODS_MIN_SIZE, restored_position, restored_size};

pub(crate) struct ModsRenderContext<'a> {
    pub(crate) settings: &'a Settings,
    pub(crate) state: &'a mut ModState,
}

pub(super) struct ModsWindow {
    window: Arc<Window>,
    gpu: GpuContext,
    egui: EguiRenderer,
    size: PhysicalSize<u32>,
}

impl ModsWindow {
    pub(super) fn new(
        event_loop: &ActiveEventLoop,
        shared_gpu: &GpuContext,
        settings: &Settings,
    ) -> anyhow::Result<Self> {
        let size = restored_size(
            settings.ui.mods_window_size,
            MODS_MIN_SIZE,
            MODS_DEFAULT_SIZE,
        );
        let mut attrs = WindowAttributes::default()
            .with_title(format!("zeff-boy Mods v{}", env!("CARGO_PKG_VERSION")))
            .with_inner_size(size)
            .with_min_inner_size(PhysicalSize::new(MODS_MIN_SIZE[0], MODS_MIN_SIZE[1]));
        if let Some(position) =
            restored_position(event_loop, settings.ui.mods_window_position, size)
        {
            attrs = attrs.with_position(position);
        }
        if let Some(icon) = super::Graphics::load_window_icon() {
            attrs = attrs.with_window_icon(Some(icon));
        }

        let window = Arc::new(event_loop.create_window(attrs)?);
        window.set_maximized(settings.ui.mods_window_maximized);
        let size = window.inner_size();
        let gpu = shared_gpu.new_surface(
            window.clone(),
            size.width.max(1),
            size.height.max(1),
            VsyncMode::On,
        )?;
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

    pub(super) fn render(&mut self, ctx: ModsRenderContext<'_>) -> Result<(), FrameError> {
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

        let mut root_ui = egui::Ui::new(
            self.egui.context().clone(),
            egui::Id::new("mods_root_ui"),
            egui::UiBuilder::new()
                .layer_id(egui::LayerId::background())
                .max_rect(self.egui.context().content_rect()),
        );
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::same(8)))
            .show(&mut root_ui, |ui| {
                crate::debug::draw_mods_content(ui, ctx.state);
            });
        let output = self.egui.end_frame(&self.window);

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("mods encoder"),
            });
        let (paint_jobs, screen_desc) = self.egui.prepare(&self.gpu, &mut encoder, &output);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("mods egui pass"),
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
        Ok(())
    }
}
