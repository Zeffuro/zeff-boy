use crate::debug::{
    self, DebugTab, DebugTabViewer, DebugUiActions, DebugWindowState, MenuAction,
};
use crate::debug::{DebugDataRefs, ToastManager};

use super::Graphics;
use super::viewport::calculate_viewport;

pub(crate) enum FrameError {
    Timeout,
    Outdated,
    Lost,
}

pub(crate) struct RenderContext<'a> {
    pub(crate) data: DebugDataRefs<'a>,
    pub(crate) debug_windows: &'a mut DebugWindowState,
    pub(crate) settings: &'a mut crate::settings::Settings,
    pub(crate) show_settings_window: &'a mut bool,
    pub(crate) dock_state: &'a mut egui_dock::DockState<DebugTab>,
    pub(crate) toast_manager: &'a mut ToastManager,
    pub(crate) speed_mode_label: Option<&'a str>,
    pub(crate) is_recording_audio: bool,
    pub(crate) is_recording_replay: bool,
    pub(crate) is_playing_replay: bool,
    pub(crate) is_rewinding: bool,
    pub(crate) rewind_seconds_back: f32,
    pub(crate) is_paused: bool,
    pub(crate) is_pocket_camera: bool,
    pub(crate) autohide_menu_bar: bool,
    pub(crate) cursor_y: Option<f32>,
    pub(crate) slot_labels: &'a [String; 10],
    pub(crate) slot_occupied: [bool; 10],
    pub(crate) active_save_slot: u8,
}

pub(crate) struct RenderResult {
    pub(crate) actions: Vec<MenuAction>,
    pub(crate) debug_actions: DebugUiActions,
    pub(crate) egui_wants_keyboard: bool,
    pub(crate) game_view_focused: bool,
}

const EMPTY_STATE_MESSAGE: &str = "Drag & drop a ROM file, or use File > Open";

impl Graphics {
    fn acquire_surface_frame(&self) -> Result<(wgpu::SurfaceTexture, wgpu::TextureView), FrameError> {
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
        Ok((frame, view))
    }

    fn ensure_game_texture(
        &mut self,
        has_game_view: bool,
        offscreen_scale: u32,
    ) -> (Option<egui::TextureId>, bool) {
        if !has_game_view {
            return (None, false);
        }

        let mut offscreen_resized = false;
        if let Some((w, h)) = self.game_view_pixel_size {
            let scale = offscreen_scale.max(1);
            let (nw, nh) = self.framebuffer.native_size();
            let ow = w.max(nw * scale);
            let oh = h.max(nh * scale);
            offscreen_resized = self.framebuffer.resize_offscreen(&self.gpu.device, ow, oh);
        }

        let tex_id = match self.game_egui_texture_id {
            Some(id) => {
                if offscreen_resized {
                    let tex_view = self.framebuffer.output_view();
                    self.egui.update_native_texture(
                        &self.gpu.device,
                        id,
                        tex_view,
                        wgpu::FilterMode::Nearest,
                    );
                }
                id
            }
            None => {
                let tex_view = self.framebuffer.output_view();
                let id = self.egui.register_native_texture(
                    &self.gpu.device,
                    tex_view,
                    wgpu::FilterMode::Nearest,
                );
                self.game_egui_texture_id = Some(id);
                id
            }
        };
        (Some(tex_id), offscreen_resized)
    }

    fn draw_rewind_overlay(&self, ctx_egui: &egui::Context, seconds_back: f32) {
        egui::Area::new(egui::Id::new("rewind_overlay"))
            .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(10.0, -10.0))
            .order(egui::Order::Foreground)
            .show(ctx_egui, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgba_unmultiplied(120, 50, 20, 210))
                    .inner_margin(egui::Margin::symmetric(12, 6))
                    .corner_radius(4.0)
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(format!("⏪ {seconds_back:.1}s back"))
                                .color(egui::Color32::WHITE)
                                .size(15.0),
                        );
                    });
            });
        ctx_egui.request_repaint();
    }

    fn submit_gpu_passes(
        &mut self,
        view: &wgpu::TextureView,
        full_output: &super::egui_integration::EguiFrameOutput,
        render_framebuffer_directly: bool,
        has_game_view_in_dock: bool,
        menu_bar_height: f32,
    ) {
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("main encoder"),
            });

        if has_game_view_in_dock {
            self.framebuffer.render_to_offscreen(&mut encoder);
        }

        if render_framebuffer_directly && self.framebuffer.needs_two_pass() {
            self.framebuffer.render_upscale_pass(&mut encoder);
        }

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("screen pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
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
            });

            if render_framebuffer_directly
                && let Some((x, y, w, h)) = {
                    let (gw, gh) = self.framebuffer.native_size();
                    calculate_viewport(
                        self.aspect_ratio_mode,
                        self.gpu.config.width,
                        self.gpu.config.height,
                        gw,
                        gh,
                        menu_bar_height,
                    )
                }
            {
                self.framebuffer.draw(&mut pass, x, y, w, h);
            }
        }

        let (paint_jobs, screen_desc) = self.egui.prepare(&self.gpu, &mut encoder, full_output);

        {
            let mut render_pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
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
                .render_to_pass(&mut render_pass, &paint_jobs, &screen_desc);
        }

        self.egui.cleanup(full_output);
        self.gpu.queue.submit(Some(encoder.finish()));
    }

    pub(crate) fn render(&mut self, ctx: RenderContext<'_>) -> Result<RenderResult, FrameError> {
        self.framebuffer.set_shader(&self.gpu.device, ctx.settings);
        self.framebuffer
            .update_params(&self.gpu.queue, ctx.settings);

        let (frame, view) = self.acquire_surface_frame()?;

        self.egui.begin_frame(&self.window);
        self.egui.apply_theme(ctx.settings.ui.theme_preset);

        let base_ppp = self.window.scale_factor() as f32;
        let target_ppp = base_ppp * ctx.settings.ui.ui_scale.clamp(0.5, 3.0);
        if (self.egui.context().pixels_per_point() - target_ppp).abs() > 0.01 {
            self.egui.context().set_pixels_per_point(target_ppp);
        }

        let show_menu = if ctx.autohide_menu_bar {
            let pointer_near_top = ctx.cursor_y.is_some_and(|y| y < 8.0);
            let any_menu_open = egui::Popup::is_any_open(self.egui.context());
            pointer_near_top || any_menu_open
        } else {
            true
        };

        let menu_actions = if show_menu {
            debug::draw_menu_bar(
                self.egui.context(),
                &debug::MenuBarContext {
                    current_mode: self.aspect_ratio_mode,
                    speed_mode_label: ctx.speed_mode_label,
                    is_recording_audio: ctx.is_recording_audio,
                    is_recording_replay: ctx.is_recording_replay,
                    is_playing_replay: ctx.is_playing_replay,
                    is_paused: ctx.is_paused,
                    slot_labels: ctx.slot_labels,
                    slot_occupied: &ctx.slot_occupied,
                    active_save_slot: ctx.active_save_slot,
                },
                ctx.dock_state,
                ctx.settings,
                ctx.debug_windows,
            )
        } else {
            debug::MenuBarResult::empty()
        };

        let mut forwarded_actions = Vec::new();
        for action in menu_actions.actions {
            match action {
                MenuAction::SetAspectRatio(mode) => self.aspect_ratio_mode = mode,
                MenuAction::OpenSettings => *ctx.show_settings_window = true,
                other => forwarded_actions.push(other),
            }
        }

        let content_rect = self.egui.context().content_rect();
        let menu_height = menu_actions.menu_bar_height_points;
        let content_min = content_rect.min + egui::vec2(0.0, menu_height);
        let content_size = egui::vec2(
            content_rect.width(),
            (content_rect.height() - menu_height).max(0.0),
        );
        let content_bounds = egui::Rect::from_min_size(content_min, content_size);

        let active_system = ctx.data.perf_info.map(|perf| {
            if perf.platform_name == "Game Boy" {
                crate::emu_backend::ActiveSystem::GameBoy
            } else {
                crate::emu_backend::ActiveSystem::Nes
            }
        });

        let gb_hardware_mode_label = ctx.data.perf_info.and_then(|perf| {
            if perf.platform_name != "Game Boy" {
                None
            } else {
                Some(perf.hardware_label.as_ref())
            }
        });

        if *ctx.show_settings_window {
            debug::draw_settings_window(
                self.egui.context(),
                ctx.settings,
                ctx.debug_windows,
                ctx.show_settings_window,
                content_bounds,
                &debug::SettingsContext {
                    active_system,
                    gb_hardware_mode_label,
                    is_pocket_camera: ctx.is_pocket_camera,
                },
            );
        }

        let debug_actions;
        let has_any_emu_data = ctx.data.cpu_debug.is_some()
            || ctx.data.perf_info.is_some()
            || ctx.data.memory_page.is_some()
            || ctx.data.rom_page.is_some();

        if has_any_emu_data {
            let has_game_view = debug::is_tab_open(ctx.dock_state, DebugTab::GameView);
            let (game_texture_id, _) =
                self.ensure_game_texture(has_game_view, ctx.settings.video.offscreen_scale);

            let mut tab_viewer = DebugTabViewer {
                data: ctx.data,
                window_state: ctx.debug_windows,
                actions: DebugUiActions::none(),
                game_texture_id,
                aspect_ratio_mode: self.aspect_ratio_mode,
                game_view_pixel_size: None,
            };

            egui::Area::new(egui::Id::new("dock_area"))
                .fixed_pos(content_min)
                .order(egui::Order::Background)
                .show(self.egui.context(), |ui| {
                    ui.set_min_size(content_size);
                    egui_dock::DockArea::new(ctx.dock_state)
                        .window_bounds(content_bounds)
                        .secondary_button_on_modifier(false)
                        .style(egui_dock::Style::from_egui(
                            self.egui.context().global_style().as_ref(),
                        ))
                        .show_inside(ui, &mut tab_viewer);
                });
            debug_actions = tab_viewer.actions;

            if let Some(size) = tab_viewer.game_view_pixel_size {
                self.game_view_pixel_size = Some(size);
            }
        } else {
            debug_actions = DebugUiActions::none();
            egui::Area::new(egui::Id::new("empty_state"))
                .fixed_pos(content_min)
                .show(self.egui.context(), |ui| {
                    ui.set_min_size(content_size);
                    ui.allocate_ui_with_layout(
                        content_size,
                        egui::Layout::centered_and_justified(egui::Direction::TopDown),
                        |ui| {
                            ui.heading(EMPTY_STATE_MESSAGE);
                        },
                    );
                });
        }

        ctx.toast_manager.set_recording(ctx.is_recording_audio);
        ctx.toast_manager.draw(self.egui.context());

        if ctx.is_rewinding {
            self.draw_rewind_overlay(self.egui.context(), ctx.rewind_seconds_back);
        }

        let egui_wants_keyboard = self.egui.context().egui_wants_keyboard_input();

        let game_view_focused = if has_any_emu_data {
            ctx.dock_state
                .focused_leaf()
                .and_then(|path| ctx.dock_state.leaf(path).ok())
                .and_then(|leaf| leaf.tabs.get(leaf.active.0))
                .is_none_or(|tab| *tab == DebugTab::GameView)
        } else {
            true
        };

        let full_output = self.egui.end_frame(&self.window);
        let menu_bar_height =
            menu_actions.menu_bar_height_points * full_output.full_output.pixels_per_point;

        let has_game_view_in_dock = has_any_emu_data
            && debug::is_tab_open(ctx.dock_state, DebugTab::GameView);
        let render_framebuffer_directly = has_any_emu_data
            && !debug::is_tab_open(ctx.dock_state, DebugTab::GameView);

        self.submit_gpu_passes(
            &view,
            &full_output,
            render_framebuffer_directly,
            has_game_view_in_dock,
            menu_bar_height,
        );

        frame.present();

        Ok(RenderResult {
            actions: forwarded_actions,
            debug_actions,
            egui_wants_keyboard,
            game_view_focused,
        })
    }
}
