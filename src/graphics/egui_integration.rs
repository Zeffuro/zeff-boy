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
        apply_egui_theme(&ctx, theme, density, debug_monospace_scale, debug_colors);

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
        if theme != self.active_theme
            || density != self.active_density
            || (debug_monospace_scale - self.active_debug_monospace_scale).abs() > f32::EPSILON
        {
            self.active_theme = theme;
            self.active_density = density;
            self.active_debug_monospace_scale = debug_monospace_scale;
            apply_egui_theme(
                &self.ctx,
                theme,
                density,
                debug_monospace_scale,
                debug_colors,
            );
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

    fn cleanup(&mut self, output: &EguiFrameOutput) {
        for id in &output.full_output.textures_delta.free {
            self.renderer.free_texture(id);
        }
    }

    pub(crate) fn submit_and_cleanup(
        &mut self,
        queue: &wgpu::Queue,
        encoder: wgpu::CommandEncoder,
        output: &EguiFrameOutput,
    ) {
        let _ = self.submit_and_cleanup_timed(queue, encoder, output, false);
    }

    pub(crate) fn submit_and_cleanup_timed(
        &mut self,
        queue: &wgpu::Queue,
        encoder: wgpu::CommandEncoder,
        output: &EguiFrameOutput,
        measure_timing: bool,
    ) -> Option<crate::platform::Instant> {
        let mut submitted = None;
        submit_before_texture_cleanup(
            || {
                queue.submit(Some(encoder.finish()));
                submitted = measure_timing.then(crate::platform::Instant::now);
            },
            || self.cleanup(output),
        );
        submitted
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

fn submit_before_texture_cleanup(submit: impl FnOnce(), cleanup: impl FnOnce()) {
    submit();
    cleanup();
}

pub(crate) fn apply_egui_theme(
    ctx: &egui::Context,
    theme: UiThemePreset,
    density: UiDensity,
    debug_monospace_scale: f32,
    debug_colors: DebugColors,
) {
    let font_key = egui::Id::new("zeff_ui_fonts");
    if !ctx.data(|data| data.get_temp::<bool>(font_key).unwrap_or(false)) {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "Noto Sans".into(),
            egui::FontData::from_static(include_bytes!("../../assets/fonts/NotoSans-Regular.ttf"))
                .into(),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "Noto Sans".into());
        ctx.set_fonts(fonts);
        ctx.data_mut(|data| data.insert_temp(font_key, true));
    }
    let mut style = build_style(density, debug_monospace_scale);
    style.visuals = build_visuals(theme);
    ctx.set_theme(if style.visuals.dark_mode {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    ctx.set_global_style(style);
    crate::debug::common::set_debug_colors(ctx, debug_colors);
}

pub(super) fn dock_style(ctx: &egui::Context, density: UiDensity) -> egui_dock::Style {
    let mut style = egui_dock::Style::from_egui(ctx.global_style().as_ref());
    if density == UiDensity::Compact {
        style.tab_bar.height = 24.0;
        style.tab.spacing = 0.0;
        style.tab.tab_body.inner_margin = egui::Margin::same(2);
        style.separator.width = 1.0;
        style.separator.extra_interact_width = 3.0;
    }
    style
}

fn build_style(density: UiDensity, debug_monospace_scale: f32) -> egui::Style {
    let mut style = egui::Style::default();
    let (monospace, item_spacing, button_padding, interact_size) = match density {
        UiDensity::Compact => (
            12.0,
            egui::vec2(6.0, 3.0),
            egui::vec2(7.0, 2.0),
            egui::vec2(36.0, 24.0),
        ),
        UiDensity::Comfortable => (
            13.0,
            egui::vec2(8.0, 5.0),
            egui::vec2(9.0, 4.0),
            egui::vec2(40.0, 28.0),
        ),
    };

    // Preserve the former Inter x-height when using Noto Sans.
    const PROPORTIONAL_SCALE: f32 = 1.0185;
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(15.0 * PROPORTIONAL_SCALE, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(15.0 * PROPORTIONAL_SCALE, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::new(13.0 * PROPORTIONAL_SCALE, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(20.0 * PROPORTIONAL_SCALE, egui::FontFamily::Proportional),
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
    let mut visuals = match preset {
        UiThemePreset::DefaultDark => build_default_dark(),
        UiThemePreset::HighContrastDark => build_high_contrast_dark(),
        UiThemePreset::Light => build_light(),
        UiThemePreset::Retro => build_retro(),
    };
    let border = match preset {
        UiThemePreset::DefaultDark => egui::Color32::from_gray(72),
        UiThemePreset::HighContrastDark => egui::Color32::from_gray(140),
        UiThemePreset::Light => egui::Color32::from_gray(130),
        UiThemePreset::Retro => visuals.widgets.noninteractive.fg_stroke.color,
    };
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::same(8);
        widget.expansion = 0.0;
    }
    visuals.widgets.inactive.bg_fill = visuals.panel_fill;
    visuals.widgets.inactive.weak_bg_fill = visuals.panel_fill;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, border);
    visuals.widgets.hovered.weak_bg_fill = visuals.widgets.hovered.bg_fill;
    visuals.widgets.active.bg_fill = visuals.selection.bg_fill;
    visuals.widgets.active.weak_bg_fill = visuals.selection.bg_fill;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.5, visuals.selection.stroke.color);
    visuals.widgets.open = visuals.widgets.hovered;
    visuals.handle_shape = egui::style::HandleShape::Rect { aspect_ratio: 0.5 };
    visuals.slider_trailing_fill = true;
    visuals
}

fn build_default_dark() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.widgets.noninteractive.fg_stroke.color = egui::Color32::from_gray(220);
    v.widgets.inactive.fg_stroke.color = egui::Color32::from_gray(225);
    v.weak_text_color = Some(egui::Color32::from_gray(170));

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
    v.weak_text_color = Some(egui::Color32::from_gray(210));
    v.faint_bg_color = egui::Color32::from_gray(18);
    v.extreme_bg_color = egui::Color32::from_gray(6);

    v
}

fn build_light() -> egui::Visuals {
    let mut v = egui::Visuals::light();
    v.widgets.noninteractive.fg_stroke.color = egui::Color32::from_gray(40);
    v.weak_text_color = Some(egui::Color32::from_gray(92));

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
    v.weak_text_color = Some(egui::Color32::from_rgb(95, 165, 90));
    v.hyperlink_color = accent;
    v.faint_bg_color = egui::Color32::from_rgb(24, 27, 21);
    v.extreme_bg_color = egui::Color32::from_rgb(12, 14, 10);

    v
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::{apply_egui_theme, build_style, build_visuals, submit_before_texture_cleanup};
    use crate::settings::{DebugColors, UiDensity, UiThemePreset};

    #[test]
    fn density_changes_preserve_selected_theme_and_user_zoom() {
        let ctx = egui::Context::default();
        ctx.set_zoom_factor(1.5);
        ctx.begin_pass(Default::default());
        let _ = ctx.end_pass();
        for theme in [
            UiThemePreset::Light,
            UiThemePreset::HighContrastDark,
            UiThemePreset::Retro,
        ] {
            for density in [
                UiDensity::Compact,
                UiDensity::Comfortable,
                UiDensity::Compact,
            ] {
                apply_egui_theme(&ctx, theme, density, 1.25, DebugColors::default());
                let expected = build_visuals(theme);
                let style = ctx.global_style();
                assert_eq!(style.visuals.dark_mode, expected.dark_mode);
                assert_eq!(style.visuals.panel_fill, expected.panel_fill);
                assert_eq!(style.visuals.text_color(), expected.text_color());
                assert_eq!(
                    ctx.zoom_factor(),
                    1.5,
                    "styling must not replace the user's UI zoom"
                );
            }
        }
    }

    #[test]
    fn compact_spacing_does_not_shrink_readable_labels_or_override_debug_font_scale() {
        let compact = build_style(UiDensity::Compact, 1.0);
        let comfortable = build_style(UiDensity::Comfortable, 1.0);
        for text_style in [
            egui::TextStyle::Body,
            egui::TextStyle::Small,
            egui::TextStyle::Heading,
        ] {
            assert_eq!(
                compact.text_styles[&text_style],
                comfortable.text_styles[&text_style]
            );
        }
        assert!(compact.spacing.interact_size.y < comfortable.spacing.interact_size.y);
        let scaled = build_style(UiDensity::Compact, 1.25);
        assert_eq!(
            scaled.text_styles[&egui::TextStyle::Monospace].size,
            compact.text_styles[&egui::TextStyle::Monospace].size * 1.25
        );
        assert_eq!(
            scaled.text_styles[&egui::TextStyle::Body],
            compact.text_styles[&egui::TextStyle::Body]
        );
    }

    #[test]
    fn secondary_text_has_readable_contrast_on_each_theme_surface() {
        fn luminance(color: egui::Color32) -> f64 {
            let channel = |byte: u8| {
                let value = f64::from(byte) / 255.0;
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
        }
        for theme in [
            UiThemePreset::DefaultDark,
            UiThemePreset::HighContrastDark,
            UiThemePreset::Light,
            UiThemePreset::Retro,
        ] {
            let visuals = build_visuals(theme);
            let foreground = visuals.weak_text_color();
            assert_eq!(foreground.a(), 255);
            for background in [
                visuals.panel_fill,
                visuals.window_fill,
                visuals.extreme_bg_color,
            ] {
                let a = luminance(foreground);
                let b = luminance(background);
                let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                assert!(
                    ratio >= 4.5,
                    "{theme:?} secondary text contrast was {ratio:.2}:1"
                );
            }
        }
    }

    #[test]
    fn freed_textures_are_cleaned_up_only_after_submission() {
        let events = RefCell::new(Vec::new());

        submit_before_texture_cleanup(
            || events.borrow_mut().push("submit"),
            || events.borrow_mut().push("cleanup"),
        );

        assert_eq!(*events.borrow(), ["submit", "cleanup"]);
    }
}
