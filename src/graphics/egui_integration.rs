use anyhow::Result;
use egui::ClippedPrimitive;
use winit::{event::WindowEvent, window::Window};

use crate::graphics::gpu::GpuContext;
use crate::settings::{DebugColors, UiDensity, UiThemePreset};

pub(crate) struct EguiFrameOutput {
    pub(crate) full_output: egui::FullOutput,
}

pub(crate) struct EguiRenderer {
    ctx: egui::Context,
    state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    active_theme: UiThemePreset,
    active_density: UiDensity,
    active_debug_monospace_scale: f32,
    active_debug_colors: DebugColors,
}

impl EguiRenderer {
    pub(crate) fn new(
        window: &Window,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> Result<Self> {
        let ctx = egui::Context::default();
        let theme = UiThemePreset::default();
        let density = UiDensity::default();
        let debug_monospace_scale = 1.0;
        let debug_colors = DebugColors::default();
        ctx.set_visuals(build_visuals(theme));
        ctx.set_global_style(build_style(density, debug_monospace_scale));
        crate::debug::common::set_debug_colors(&ctx, debug_colors);

        let state = egui_winit::State::new(
            ctx.clone(),
            ctx.viewport_id(),
            window,
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
            active_density: density,
            active_debug_monospace_scale: debug_monospace_scale,
            active_debug_colors: debug_colors,
        })
    }

    pub(crate) fn apply_style(
        &mut self,
        theme: UiThemePreset,
        density: UiDensity,
        debug_monospace_scale: f32,
        debug_colors: DebugColors,
    ) {
        if theme != self.active_theme {
            self.active_theme = theme;
            self.ctx.set_visuals(build_visuals(theme));
        }
        if density != self.active_density
            || (debug_monospace_scale - self.active_debug_monospace_scale).abs() > f32::EPSILON
        {
            self.active_density = density;
            self.active_debug_monospace_scale = debug_monospace_scale;
            self.ctx
                .set_global_style(build_style(density, debug_monospace_scale));
        }
        if debug_colors != self.active_debug_colors {
            self.active_debug_colors = debug_colors;
            crate::debug::common::set_debug_colors(&self.ctx, debug_colors);
        }
    }

    pub(crate) fn context(&self) -> &egui::Context {
        &self.ctx
    }

    pub(crate) fn handle_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        let response = self.state.on_window_event(window, event);
        response.consumed
    }

    pub(crate) fn handle_event_with_repaint(
        &mut self,
        window: &Window,
        event: &WindowEvent,
    ) -> (bool, bool) {
        let response = self.state.on_window_event(window, event);
        (response.consumed, response.repaint)
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

pub(super) fn dock_style(ctx: &egui::Context, density: UiDensity) -> egui_dock::Style {
    let mut style = egui_dock::Style::from_egui(ctx.global_style().as_ref());
    if density == UiDensity::Compact {
        style.tab_bar.height = 20.0;
        style.tab.spacing = 0.0;
        style.tab.tab_body.inner_margin = egui::Margin::same(2);
        style.separator.width = 1.0;
        style.separator.extra_interact_width = 3.0;
    }
    style
}

fn build_style(density: UiDensity, debug_monospace_scale: f32) -> egui::Style {
    let mut style = egui::Style::default();
    let (body, small, heading, monospace, item_spacing, button_padding, interact_size) =
        match density {
            UiDensity::Compact => (
                12.5,
                10.0,
                15.0,
                12.0,
                egui::vec2(4.0, 2.0),
                egui::vec2(4.0, 1.0),
                egui::vec2(36.0, 18.0),
            ),
            UiDensity::Comfortable => (
                14.0,
                11.0,
                18.0,
                13.0,
                egui::vec2(8.0, 4.0),
                egui::vec2(6.0, 2.0),
                egui::vec2(40.0, 20.0),
            ),
        };

    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(body, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(body, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::new(small, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(heading, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::new(
            monospace * debug_monospace_scale.clamp(0.75, 1.5),
            egui::FontFamily::Monospace,
        ),
    );
    style.spacing.item_spacing = item_spacing;
    style.spacing.button_padding = button_padding;
    style.spacing.interact_size = interact_size;
    style.interaction.selectable_labels = false;
    style
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

    v.window_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_gray(140));
    v.window_shadow = egui::Shadow {
        offset: [0, 2],
        blur: 6,
        spread: 0,
        color: egui::Color32::from_black_alpha(80),
    };

    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_gray(220));
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_gray(210));
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::WHITE);
    v.widgets.active.fg_stroke = egui::Stroke::new(1.5_f32, egui::Color32::WHITE);

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
    v.selection.stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(40, 80, 160));

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
    v.window_stroke = egui::Stroke::new(1.0_f32, border);
    v.window_shadow = egui::Shadow {
        offset: [0, 1],
        blur: 4,
        spread: 0,
        color: egui::Color32::from_black_alpha(60),
    };
    v.panel_fill = bg;

    v.widgets.noninteractive.corner_radius = rounding;
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, fg_dim);
    v.widgets.noninteractive.bg_fill = bg;
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.0_f32, border);

    v.widgets.inactive.corner_radius = rounding;
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, fg);
    v.widgets.inactive.bg_fill = egui::Color32::from_rgb(28, 34, 26);

    v.widgets.hovered.corner_radius = rounding;
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, accent);
    v.widgets.hovered.bg_fill = hover_bg;

    v.widgets.active.corner_radius = rounding;
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, accent);
    v.widgets.active.bg_fill = egui::Color32::from_rgb(40, 52, 34);

    v.widgets.open.corner_radius = rounding;
    v.widgets.open.bg_fill = hover_bg;

    v.selection.bg_fill = egui::Color32::from_rgb(30, 70, 30);
    v.selection.stroke = egui::Stroke::new(1.0_f32, fg);

    v.override_text_color = Some(fg);
    v.hyperlink_color = accent;
    v.faint_bg_color = egui::Color32::from_rgb(24, 27, 21);
    v.extreme_bg_color = egui::Color32::from_rgb(12, 14, 10);

    v
}
