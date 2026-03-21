use anyhow::Result;
use egui::ClippedPrimitive;
use winit::{event::WindowEvent, event_loop::ActiveEventLoop, window::Window};

use crate::graphics::egui_painter::EguiPainter;
use crate::graphics::gpu::GpuContext;

pub(crate) struct EguiFrameOutput {
    pub(crate) full_output: egui::FullOutput,
    pub(crate) paint_jobs: Vec<ClippedPrimitive>,
}

pub(crate) struct EguiRenderer {
    ctx: egui::Context,
    state: egui_winit::State,
    painter: EguiPainter,
}

impl EguiRenderer {
    pub(crate) fn new(
        event_loop: &ActiveEventLoop,
        window: &Window,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> Result<Self> {
        let ctx = egui::Context::default();
        let mut style = (*ctx.style()).clone();
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new(13.0, egui::FontFamily::Monospace),
        );
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            egui::FontId::new(13.0, egui::FontFamily::Monospace),
        );
        ctx.set_visuals(egui::Visuals::dark());
        ctx.set_style(style);

        let state = egui_winit::State::new(
            ctx.clone(),
            ctx.viewport_id(),
            event_loop,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        let painter = EguiPainter::new(device, format)?;

        Ok(Self { ctx, state, painter })
    }

    pub(crate) fn context(&self) -> &egui::Context {
        &self.ctx
    }

    pub(crate) fn handle_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        let response = self.state.on_window_event(window, event);
        response.consumed
    }

    pub(crate) fn begin_frame(&mut self, window: &Window) {
        let raw_input = self.state.take_egui_input(window);
        self.ctx.begin_pass(raw_input);
    }

    pub(crate) fn end_frame(&mut self, window: &Window) -> EguiFrameOutput {
        let full_output = self.ctx.end_pass();
        self.state
            .handle_platform_output(window, full_output.platform_output.clone());
        let paint_jobs = self
            .ctx
            .tessellate(full_output.shapes.clone(), full_output.pixels_per_point);

        EguiFrameOutput {
            full_output,
            paint_jobs,
        }
    }

    pub(crate) fn paint(
        &mut self,
        gpu: &GpuContext,
        render_pass: &mut wgpu::RenderPass<'_>,
        output: &EguiFrameOutput,
    ) -> Result<()> {
        self.painter.paint(
            &gpu.device,
            &gpu.queue,
            render_pass,
            gpu.config.width,
            gpu.config.height,
            output.full_output.pixels_per_point,
            &output.paint_jobs,
            &output.full_output.textures_delta,
        )
    }
}

