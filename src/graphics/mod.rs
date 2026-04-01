use anyhow::Result;
use std::sync::Arc;
use winit::{
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{Window, WindowAttributes},
};

mod egui_integration;
mod framebuffer;
mod gpu;
mod pipeline;
mod render_frame;
mod viewport;

use egui_integration::EguiRenderer;
use framebuffer::FramebufferRenderer;
use gpu::GpuContext;

pub(crate) use render_frame::{FrameError, RenderContext};
pub(crate) use viewport::AspectRatioMode;

use crate::settings::VsyncMode;

pub(crate) struct Graphics {
    window: Arc<Window>,
    gpu: GpuContext,
    egui: EguiRenderer,
    framebuffer: FramebufferRenderer,
    size: PhysicalSize<u32>,
    aspect_ratio_mode: AspectRatioMode,
    game_egui_texture_id: Option<egui::TextureId>,
    game_view_pixel_size: Option<(u32, u32)>,
}

impl Graphics {
    pub(crate) async fn new(event_loop: &ActiveEventLoop, vsync: VsyncMode) -> Result<Self> {
        let window =
            Arc::new(event_loop.create_window(WindowAttributes::default().with_title("zeff-boy"))?);

        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        let gpu = GpuContext::new(window.clone(), width, height, vsync).await?;
        let egui = EguiRenderer::new(event_loop, &window, &gpu.device, gpu.config.format)?;
        let framebuffer = FramebufferRenderer::new(&gpu.device, gpu.config.format)?;

        Ok(Self {
            window,
            gpu,
            egui,
            framebuffer,
            size,
            aspect_ratio_mode: AspectRatioMode::IntegerScale,
            game_egui_texture_id: None,
            game_view_pixel_size: None,
        })
    }

    pub(crate) fn window(&self) -> &Window {
        &self.window
    }

    pub(crate) fn size(&self) -> PhysicalSize<u32> {
        self.size
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.size = PhysicalSize::new(width, height);
        self.gpu.resize(width, height);
    }

    pub(crate) fn set_vsync(&mut self, vsync: VsyncMode) {
        self.gpu.set_present_mode(vsync);
    }

    pub(crate) fn handle_event(&mut self, event: &WindowEvent) -> bool {
        self.egui.handle_event(&self.window, event)
    }

    pub(crate) fn upload_framebuffer(&self, framebuffer: &[u8]) {
        self.framebuffer
            .upload_framebuffer(&self.gpu.queue, framebuffer);
    }

    pub(crate) fn clear_framebuffer(&self) {
        let (w, h) = self.framebuffer.native_size();
        let len = (w * h * 4) as usize;
        let black = vec![0u8; len];
        self.framebuffer.upload_framebuffer(&self.gpu.queue, &black);
    }

    pub(crate) fn set_native_size(&mut self, width: u32, height: u32) {
        self.framebuffer
            .set_native_size(&self.gpu.device, width, height);
    }
}
