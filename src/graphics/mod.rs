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

use egui_integration::EguiRenderer;
use framebuffer::FramebufferRenderer;
use gpu::GpuContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AspectRatioMode {
    Stretch,
    KeepAspect,
    IntegerScale,
}

fn calculate_viewport(
    mode: AspectRatioMode,
    window_width: u32,
    window_height: u32,
    game_width: u32,
    game_height: u32,
    top_offset: f32,
) -> Option<(f32, f32, f32, f32)> {
    let ww = window_width as f32;
    let wh = window_height as f32;
    let available_h = (wh - top_offset).max(0.0);
    if ww <= 0.0 || available_h <= 0.0 {
        return None;
    }

    match mode {
        AspectRatioMode::Stretch => Some((0.0, top_offset, ww, available_h)),
        AspectRatioMode::KeepAspect => {
            let scale_x = ww / game_width as f32;
            let scale_y = available_h / game_height as f32;
            let scale = scale_x.min(scale_y);
            let w = game_width as f32 * scale;
            let h = game_height as f32 * scale;
            let x = (ww - w) / 2.0;
            let y = top_offset + (available_h - h) / 2.0;
            Some((x, y, w, h))
        }
        AspectRatioMode::IntegerScale => {
            let scale_x = window_width / game_width;
            let visible_h = (available_h.floor() as u32).max(1);
            let scale_y = visible_h / game_height;
            let scale = scale_x.min(scale_y).max(1);
            let w = game_width * scale;
            let h = game_height * scale;
            let x = (window_width.saturating_sub(w)) / 2;
            let y = ((visible_h.saturating_sub(h)) / 2) as f32 + top_offset;
            Some((x as f32, y, w as f32, h as f32))
        }
    }
}

pub(crate) enum FrameError {
    Timeout,
    Outdated,
    Lost,
    OutOfMemory,
}

pub(crate) struct RenderContext<'a> {
    pub(crate) debug_info: Option<&'a crate::debug::DebugInfo>,
    pub(crate) viewer_data: Option<&'a crate::debug::DebugViewerData>,
    pub(crate) rom_info_view: Option<&'a crate::debug::RomInfoViewData>,
    pub(crate) disassembly_view: Option<&'a crate::debug::DisassemblyView>,
    pub(crate) memory_page: Option<&'a [(u16, u8)]>,
    pub(crate) rom_page: Option<&'a [(u32, u8)]>,
    pub(crate) rom_size: u32,
    pub(crate) debug_windows: &'a mut crate::debug::DebugWindowState,
    pub(crate) settings: &'a mut crate::settings::Settings,
    pub(crate) show_settings_window: &'a mut bool,
    pub(crate) dock_state: &'a mut egui_dock::DockState<crate::debug::DebugTab>,
    pub(crate) toast_manager: &'a mut crate::debug::ToastManager,
    pub(crate) speed_mode_label: Option<&'a str>,
    pub(crate) is_recording_audio: bool,
    pub(crate) is_recording_replay: bool,
    pub(crate) is_playing_replay: bool,
    pub(crate) is_rewinding: bool,
    pub(crate) rewind_seconds_back: f32,
    pub(crate) is_paused: bool,
    pub(crate) autohide_menu_bar: bool,
    pub(crate) cursor_y: Option<f32>,
}

pub(crate) struct RenderResult {
    pub(crate) open_file_requested: bool,
    pub(crate) reset_game_requested: bool,
    pub(crate) stop_game_requested: bool,
    pub(crate) save_state_file_requested: bool,
    pub(crate) load_state_file_requested: bool,
    pub(crate) save_state_slot: Option<u8>,
    pub(crate) load_state_slot: Option<u8>,
    pub(crate) load_recent_rom: Option<std::path::PathBuf>,
    pub(crate) toolbar_settings_changed: bool,
    pub(crate) toggle_fullscreen: bool,
    pub(crate) toggle_pause: bool,
    pub(crate) speed_change: i32,
    pub(crate) start_audio_recording: bool,
    pub(crate) stop_audio_recording: bool,
    pub(crate) start_replay_recording: bool,
    pub(crate) stop_replay_recording: bool,
    pub(crate) load_replay: bool,
    pub(crate) take_screenshot: bool,
    pub(crate) debug_actions: crate::debug::DebugUiActions,
    pub(crate) layer_toggles: Option<(bool, bool, bool)>,
    pub(crate) egui_wants_keyboard: bool,
}

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
    pub(crate) async fn new(event_loop: &ActiveEventLoop) -> Result<Self> {
        let window =
            Arc::new(event_loop.create_window(WindowAttributes::default().with_title("zeff-boy"))?);

        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        let gpu = GpuContext::new(window.clone(), width, height).await?;
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

    pub(crate) fn handle_event(&mut self, event: &WindowEvent) -> bool {
        self.egui.handle_event(&self.window, event)
    }

    pub(crate) fn upload_framebuffer(&self, framebuffer: &[u8]) {
        self.framebuffer
            .upload_framebuffer(&self.gpu.queue, framebuffer);
    }

    pub(crate) fn clear_framebuffer(&self) {
        let black = vec![0u8; 160 * 144 * 4];
        self.framebuffer.upload_framebuffer(&self.gpu.queue, &black);
    }

    pub(crate) fn render(&mut self, ctx: RenderContext<'_>) -> Result<RenderResult, FrameError> {
        self.framebuffer
            .set_shader(&self.gpu.device, ctx.settings);
        self.framebuffer
            .update_params(&self.gpu.queue, &ctx.settings.shader_params);

        let frame = self
            .gpu
            .surface
            .get_current_texture()
            .map_err(|e| match e {
                wgpu::SurfaceError::Timeout => FrameError::Timeout,
                wgpu::SurfaceError::Outdated => FrameError::Outdated,
                wgpu::SurfaceError::Lost => FrameError::Lost,
                wgpu::SurfaceError::OutOfMemory => FrameError::OutOfMemory,
                _ => FrameError::Lost,
            })?;

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.egui.begin_frame(&self.window);

        let show_menu = if ctx.autohide_menu_bar {
            let pointer_near_top = ctx.cursor_y.is_some_and(|y| y < 8.0);
            let any_menu_open = egui::Popup::is_any_open(self.egui.context());
            pointer_near_top || any_menu_open
        } else {
            true
        };

        let menu_actions = if show_menu {
            crate::debug::draw_menu_bar(
                self.egui.context(),
                self.aspect_ratio_mode,
                ctx.dock_state,
                ctx.settings,
                ctx.debug_windows,
                ctx.speed_mode_label,
                ctx.is_recording_audio,
                ctx.is_recording_replay,
                ctx.is_playing_replay,
                ctx.is_paused,
            )
        } else {
            crate::debug::MenuActions::default(ctx.settings.autohide_menu_bar)
        };
        if let Some(mode) = menu_actions.aspect_ratio_mode {
            self.aspect_ratio_mode = mode;
        }
        if menu_actions.open_settings_requested {
            *ctx.show_settings_window = true;
        }
        if *ctx.show_settings_window {
            crate::debug::draw_settings_window(
                self.egui.context(),
                ctx.settings,
                ctx.debug_windows,
                ctx.show_settings_window,
            );
        }

        let debug_actions;
        if ctx.debug_info.is_some() {
            let has_game_view = crate::debug::has_game_view_tab(ctx.dock_state);

            // Resize offscreen texture to match previous frame's GameView panel size
            if has_game_view {
                if let Some((w, h)) = self.game_view_pixel_size {
                    self.framebuffer.resize_offscreen(&self.gpu.device, w, h);
                }
            }

            let game_texture_id = if has_game_view {
                let tex_view = self.framebuffer.output_view();
                match self.game_egui_texture_id {
                    Some(id) => {
                        self.egui.update_native_texture(
                            &self.gpu.device,
                            id,
                            tex_view,
                            wgpu::FilterMode::Nearest,
                        );
                        Some(id)
                    }
                    None => {
                        let id = self.egui.register_native_texture(
                            &self.gpu.device,
                            tex_view,
                            wgpu::FilterMode::Nearest,
                        );
                        self.game_egui_texture_id = Some(id);
                        Some(id)
                    }
                }
            } else {
                None
            };

            let mut tab_viewer = crate::debug::DebugTabViewer {
                debug_info: ctx.debug_info,
                viewer_data: ctx.viewer_data,
                rom_info_view: ctx.rom_info_view,
                disassembly_view: ctx.disassembly_view,
                memory_page: ctx.memory_page,
                rom_page: ctx.rom_page,
                rom_size: ctx.rom_size,
                window_state: ctx.debug_windows,
                actions: crate::debug::DebugUiActions::none(),
                game_texture_id,
                aspect_ratio_mode: self.aspect_ratio_mode,
                game_view_pixel_size: None,
            };
            egui_dock::DockArea::new(ctx.dock_state)
                .style(egui_dock::Style::from_egui(
                    self.egui.context().style().as_ref(),
                ))
                .show(self.egui.context(), &mut tab_viewer);
            debug_actions = tab_viewer.actions;

            // Store the GameView pixel size for next frame's offscreen resize
            if let Some(size) = tab_viewer.game_view_pixel_size {
                self.game_view_pixel_size = Some(size);
            }
        } else {
            debug_actions = crate::debug::DebugUiActions::none();
            egui::CentralPanel::default().show(self.egui.context(), |ui| {
                ui.centered_and_justified(|ui| {
                    ui.heading("Drag & drop a ROM file, or use File > Open");
                });
            });
        }

        ctx.toast_manager.set_recording(ctx.is_recording_audio);
        ctx.toast_manager.draw(self.egui.context());

        if ctx.is_rewinding {
            egui::Area::new(egui::Id::new("rewind_overlay"))
                .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(10.0, -10.0))
                .order(egui::Order::Foreground)
                .show(self.egui.context(), |ui| {
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgba_unmultiplied(120, 50, 20, 210))
                        .inner_margin(egui::Margin::symmetric(12, 6))
                        .corner_radius(4.0)
                        .show(ui, |ui| {
                            let secs = ctx.rewind_seconds_back;
                            ui.label(
                                egui::RichText::new(format!("⏪ {secs:.1}s back"))
                                    .color(egui::Color32::WHITE)
                                    .size(15.0),
                            );
                        });
                });
            // Keep repainting while rewinding so the counter updates in realtime
            self.egui.context().request_repaint();
        }

        let egui_wants_keyboard = self.egui.context().wants_keyboard_input();
        let full_output = self.egui.end_frame(&self.window);
        let menu_bar_height =
            menu_actions.menu_bar_height_points * full_output.full_output.pixels_per_point;

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("main encoder"),
            });

        // Offscreen shader pass — renders framebuffer through shaders into
        // the output texture used as the egui game-view image.
        let has_game_view_in_dock =
            ctx.debug_info.is_some() && crate::debug::has_game_view_tab(ctx.dock_state);
        if has_game_view_in_dock {
            self.framebuffer.render_to_offscreen(&mut encoder);
        }

        // Emulator Framebuffer (only when not rendered inside a dock tab)
        let render_framebuffer_directly =
            ctx.debug_info.is_some() && !crate::debug::has_game_view_tab(ctx.dock_state);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("screen pass"),
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
            });

            if render_framebuffer_directly {
                if let Some((x, y, w, h)) = calculate_viewport(
                    self.aspect_ratio_mode,
                    self.gpu.config.width,
                    self.gpu.config.height,
                    160,
                    144,
                    menu_bar_height,
                ) {
                    self.framebuffer.draw(&mut pass, x, y, w, h);
                }
            }
        }

        // EGUI: prepare
        let (paint_jobs, screen_desc) = self.egui.prepare(&self.gpu, &mut encoder, &full_output);

        // EGUI: render
        {
            let mut render_pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
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
                })
                .forget_lifetime();
            self.egui
                .render_to_pass(&mut render_pass, &paint_jobs, &screen_desc);
        }

        self.egui.cleanup(&full_output);

        self.gpu.queue.submit(Some(encoder.finish()));
        frame.present();

        Ok(RenderResult {
            open_file_requested: menu_actions.open_file_requested,
            reset_game_requested: menu_actions.reset_game_requested,
            stop_game_requested: menu_actions.stop_game_requested,
            save_state_file_requested: menu_actions.save_state_file_requested,
            load_state_file_requested: menu_actions.load_state_file_requested,
            save_state_slot: menu_actions.save_state_slot,
            load_state_slot: menu_actions.load_state_slot,
            load_recent_rom: menu_actions.load_recent_rom,
            toolbar_settings_changed: menu_actions.toolbar_settings_changed,
            toggle_fullscreen: menu_actions.toggle_fullscreen,
            toggle_pause: menu_actions.toggle_pause,
            speed_change: menu_actions.speed_change,
            start_audio_recording: menu_actions.start_audio_recording,
            stop_audio_recording: menu_actions.stop_audio_recording,
            start_replay_recording: menu_actions.start_replay_recording,
            stop_replay_recording: menu_actions.stop_replay_recording,
            load_replay: menu_actions.load_replay,
            take_screenshot: menu_actions.take_screenshot,
            debug_actions,
            layer_toggles: menu_actions.layer_toggles,
            egui_wants_keyboard,
        })
    }
}
