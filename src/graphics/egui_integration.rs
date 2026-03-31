use anyhow::Result;
use egui::ClippedPrimitive;
use winit::{event::WindowEvent, event_loop::ActiveEventLoop, window::Window};

use crate::graphics::gpu::GpuContext;
use crate::settings::UiThemePreset;

pub(crate) struct EguiFrameOutput {
    pub(crate) full_output: egui::FullOutput,
}

pub(crate) struct EguiRenderer {
    ctx: egui::Context,
    state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    active_theme: UiThemePreset,
}

impl EguiRenderer {
    pub(crate) fn new(
        event_loop: &ActiveEventLoop,
        window: &Window,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> Result<Self> {
        let ctx = egui::Context::default();
        let mut style = (*ctx.global_style()).clone();

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

        style.spacing.item_spacing = egui::vec2(8.0, 4.0);
        style.spacing.button_padding = egui::vec2(6.0, 2.0);
        style.spacing.interact_size = egui::vec2(40.0, 20.0);
        style.interaction.selectable_labels = false;

        let theme = UiThemePreset::default();
        ctx.set_visuals(build_visuals(theme));
        ctx.set_global_style(style);

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
            active_theme: theme,
        })
    }

    pub(crate) fn apply_theme(&mut self, preset: UiThemePreset) {
        if preset == self.active_theme {
            return;
        }
        self.active_theme = preset;
        self.ctx.set_visuals(build_visuals(preset));
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

fn build_visuals(preset: UiThemePreset) -> egui::Visuals {
    match preset {
        UiThemePreset::DefaultDark => build_default_dark(),
        UiThemePreset::HighContrastDark => build_high_contrast_dark(),
        UiThemePreset::Light => build_light(),
        UiThemePreset::Retro => build_retro(),
    }
}

fn build_default_dark() -> egui::Visuals {
    let mut v = egui::Visuals::dark();

    v.window_shadow = egui::Shadow {
        offset: [0, 2],
        blur: 6,
        spread: 0,
        color: egui::Color32::from_black_alpha(50),
    };

    v.selection.bg_fill = egui::Color32::from_rgb(45, 85, 150);

    v
}

fn build_high_contrast_dark() -> egui::Visuals {
    let mut v = egui::Visuals::dark();

    v.window_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(140));
    v.window_shadow = egui::Shadow {
        offset: [0, 2],
        blur: 6,
        spread: 0,
        color: egui::Color32::from_black_alpha(80),
    };

    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(220));
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(210));
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    v.widgets.active.fg_stroke = egui::Stroke::new(1.5, egui::Color32::WHITE);

    v.selection.bg_fill = egui::Color32::from_rgb(30, 80, 180);

    v.override_text_color = Some(egui::Color32::from_gray(240));
    v.faint_bg_color = egui::Color32::from_gray(18);
    v.extreme_bg_color = egui::Color32::from_gray(6);

    v
}

fn build_light() -> egui::Visuals {
    let mut v = egui::Visuals::light();

    v.window_shadow = egui::Shadow {
        offset: [0, 2],
        blur: 8,
        spread: 0,
        color: egui::Color32::from_black_alpha(25),
    };

    v.selection.bg_fill = egui::Color32::from_rgb(140, 180, 240);
    v.selection.stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(40, 80, 160));

    v
}

fn build_retro() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    let rounding = egui::CornerRadius::same(2);

    let bg = egui::Color32::from_rgb(20, 22, 18);
    let fg = egui::Color32::from_rgb(50, 180, 50);
    let fg_dim = egui::Color32::from_rgb(40, 130, 40);
    let accent = egui::Color32::from_rgb(180, 160, 40);
    let border = egui::Color32::from_rgb(50, 65, 42);
    let hover_bg = egui::Color32::from_rgb(30, 40, 26);

    v.window_corner_radius = rounding;
    v.window_fill = bg;
    v.window_stroke = egui::Stroke::new(1.0, border);
    v.window_shadow = egui::Shadow {
        offset: [0, 1],
        blur: 4,
        spread: 0,
        color: egui::Color32::from_black_alpha(60),
    };
    v.panel_fill = bg;

    v.widgets.noninteractive.corner_radius = rounding;
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, fg_dim);
    v.widgets.noninteractive.bg_fill = bg;
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.0, border);

    v.widgets.inactive.corner_radius = rounding;
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, fg);
    v.widgets.inactive.bg_fill = egui::Color32::from_rgb(28, 34, 26);

    v.widgets.hovered.corner_radius = rounding;
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, accent);
    v.widgets.hovered.bg_fill = hover_bg;

    v.widgets.active.corner_radius = rounding;
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, accent);
    v.widgets.active.bg_fill = egui::Color32::from_rgb(40, 52, 34);

    v.widgets.open.corner_radius = rounding;
    v.widgets.open.bg_fill = hover_bg;

    v.selection.bg_fill = egui::Color32::from_rgb(30, 70, 30);
    v.selection.stroke = egui::Stroke::new(1.0, fg);

    v.override_text_color = Some(fg);
    v.hyperlink_color = accent;
    v.faint_bg_color = egui::Color32::from_rgb(24, 27, 21);
    v.extreme_bg_color = egui::Color32::from_rgb(12, 14, 10);

    v
}
