use anyhow::Result;
use egui::ClippedPrimitive;
use winit::{event::WindowEvent, event_loop::ActiveEventLoop, window::Window};

use crate::graphics::gpu::GpuContext;

pub(crate) struct EguiFrameOutput {
    pub(crate) full_output: egui::FullOutput,
}

pub(crate) struct EguiRenderer {
    ctx: egui::Context,
    state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
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
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Small,
            egui::FontId::new(11.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::new(18.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            egui::FontId::new(13.0, egui::FontFamily::Monospace),
        );

        // Slightly more generous spacing for a comfortable emulator UI.
        style.spacing.item_spacing = egui::vec2(8.0, 4.0);
        style.spacing.button_padding = egui::vec2(6.0, 2.0);
        style.spacing.interact_size = egui::vec2(40.0, 20.0);

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

        let renderer =
            egui_wgpu::Renderer::new(device, format, egui_wgpu::RendererOptions::default());

        Ok(Self {
            ctx,
            state,
            renderer,
        })
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
        EguiFrameOutput { full_output }
    }

    pub(crate) fn prepare(
        &mut self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        output: &EguiFrameOutput,
    ) -> (Vec<ClippedPrimitive>, egui_wgpu::ScreenDescriptor) {
        let paint_jobs = self.ctx.tessellate(
            output.full_output.shapes.clone(),
            output.full_output.pixels_per_point,
        );

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [gpu.config.width, gpu.config.height],
            pixels_per_point: output.full_output.pixels_per_point,
        };

        for (id, delta) in &output.full_output.textures_delta.set {
            self.renderer
                .update_texture(&gpu.device, &gpu.queue, *id, delta);
        }

        self.renderer.update_buffers(
            &gpu.device,
            &gpu.queue,
            encoder,
            &paint_jobs,
            &screen_descriptor,
        );

        (paint_jobs, screen_descriptor)
    }

    pub(crate) fn render_to_pass(
        &self,
        render_pass: &mut wgpu::RenderPass<'static>,
        paint_jobs: &[ClippedPrimitive],
        screen_descriptor: &egui_wgpu::ScreenDescriptor,
    ) {
        self.renderer
            .render(render_pass, paint_jobs, screen_descriptor);
    }

    pub(crate) fn cleanup(&mut self, output: &EguiFrameOutput) {
        for id in &output.full_output.textures_delta.free {
            self.renderer.free_texture(id);
        }
    }

    pub(crate) fn register_native_texture(
        &mut self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
        filter: wgpu::FilterMode,
    ) -> egui::TextureId {
        self.renderer.register_native_texture(device, view, filter)
    }

    pub(crate) fn update_native_texture(
        &mut self,
        device: &wgpu::Device,
        id: egui::TextureId,
        view: &wgpu::TextureView,
        filter: wgpu::FilterMode,
    ) {
        self.renderer
            .update_egui_texture_from_wgpu_texture(device, view, filter, id);
    }
}
