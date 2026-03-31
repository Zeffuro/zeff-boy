use anyhow::Result;
use std::sync::Arc;
use winit::{
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{Window, WindowAttributes},
};

use crate::debug::{
    self, ApuDebugInfo, ConsoleGraphicsData, CpuDebugSnapshot, DebugTab, DebugTabViewer,
    DebugUiActions, DebugWindowState, DisassemblyView, InputDebugInfo, MenuAction, OamDebugInfo,
    PaletteDebugInfo, PerfInfo, RomDebugInfo, ToastManager,
};

mod egui_integration;
mod framebuffer;
mod gpu;
mod viewport;

use egui_integration::EguiRenderer;
use framebuffer::FramebufferRenderer;
use gpu::GpuContext;

pub(crate) use viewport::AspectRatioMode;
use viewport::calculate_viewport;

pub(crate) enum FrameError {
    Timeout,
    Outdated,
    Lost,
}

pub(crate) struct RenderContext<'a> {
    pub(crate) cpu_debug: Option<&'a CpuDebugSnapshot>,
    pub(crate) perf_info: Option<&'a PerfInfo>,
    pub(crate) apu_debug: Option<&'a ApuDebugInfo>,
    pub(crate) oam_debug: Option<&'a OamDebugInfo>,
    pub(crate) palette_debug: Option<&'a PaletteDebugInfo>,
    pub(crate) rom_debug: Option<&'a RomDebugInfo>,
    pub(crate) input_debug: Option<&'a InputDebugInfo>,
    pub(crate) graphics_data: Option<&'a ConsoleGraphicsData>,
    pub(crate) disassembly_view: Option<&'a DisassemblyView>,
    pub(crate) memory_page: Option<&'a [(u16, u8)]>,
    pub(crate) rom_page: Option<&'a [(u32, u8)]>,
    pub(crate) rom_size: u32,
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
    pub(crate) slot_labels: [String; 10],
}

pub(crate) struct RenderResult {
    pub(crate) actions: Vec<MenuAction>,
    pub(crate) debug_actions: DebugUiActions,
    pub(crate) egui_wants_keyboard: bool,
    pub(crate) game_view_focused: bool,
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

use crate::settings::VsyncMode;

const EMPTY_STATE_MESSAGE: &str = "Drag & drop a ROM file, or use File > Open";

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

    pub(crate) fn render(&mut self, ctx: RenderContext<'_>) -> Result<RenderResult, FrameError> {
        self.framebuffer.set_shader(&self.gpu.device, ctx.settings);
        self.framebuffer
            .update_params(&self.gpu.queue, ctx.settings);

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
                    slot_labels: &ctx.slot_labels,
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

        let gb_hardware_mode_label = ctx.perf_info.and_then(|perf| {
            if perf.platform_name != "Game Boy" {
                None
            } else {
                Some(perf.hardware_label.clone())
            }
        });

        if *ctx.show_settings_window {
            debug::draw_settings_window(
                self.egui.context(),
                ctx.settings,
                ctx.debug_windows,
                ctx.show_settings_window,
                content_bounds,
                gb_hardware_mode_label.as_deref(),
                ctx.is_pocket_camera,
            );
        }

        let debug_actions;
        let has_any_emu_data = ctx.cpu_debug.is_some()
            || ctx.perf_info.is_some()
            || ctx.memory_page.is_some()
            || ctx.rom_page.is_some();
        if has_any_emu_data {
            let has_game_view = debug::is_tab_open(ctx.dock_state, DebugTab::GameView);

            let mut offscreen_resized = false;
            if has_game_view && let Some((w, h)) = self.game_view_pixel_size {
                let scale = ctx.settings.video.offscreen_scale.max(1);
                let (nw, nh) = self.framebuffer.native_size();
                let ow = w.max(nw * scale);
                let oh = h.max(nh * scale);
                offscreen_resized = self.framebuffer.resize_offscreen(&self.gpu.device, ow, oh);
            }

            let game_texture_id = if has_game_view {
                match self.game_egui_texture_id {
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
                        Some(id)
                    }
                    None => {
                        let tex_view = self.framebuffer.output_view();
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

            let mut tab_viewer = DebugTabViewer {
                cpu_debug: ctx.cpu_debug,
                perf_info: ctx.perf_info,
                apu_debug: ctx.apu_debug,
                oam_debug: ctx.oam_debug,
                palette_debug: ctx.palette_debug,
                rom_debug: ctx.rom_debug,
                input_debug: ctx.input_debug,
                graphics_data: ctx.graphics_data,
                disassembly_view: ctx.disassembly_view,
                memory_page: ctx.memory_page,
                rom_page: ctx.rom_page,
                rom_size: ctx.rom_size,
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

            // Store the GameView pixel size for next frame's offscreen resize
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

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("main encoder"),
            });

        // Offscreen shader pass:renders framebuffer through shaders into
        // the output texture used as the egui game-view image.
        let has_game_view_in_dock = (ctx.cpu_debug.is_some() || ctx.perf_info.is_some())
            && debug::is_tab_open(ctx.dock_state, DebugTab::GameView);
        if has_game_view_in_dock {
            self.framebuffer.render_to_offscreen(&mut encoder);
        }

        // Emulator Framebuffer (only when not rendered inside a dock tab)
        let render_framebuffer_directly = (ctx.cpu_debug.is_some() || ctx.perf_info.is_some())
            && !debug::is_tab_open(ctx.dock_state, DebugTab::GameView);

        if render_framebuffer_directly && self.framebuffer.needs_two_pass() {
            self.framebuffer.render_upscale_pass(&mut encoder);
        }
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
                    multiview_mask: None,
                })
                .forget_lifetime();
            self.egui
                .render_to_pass(&mut render_pass, &paint_jobs, &screen_desc);
        }

        self.egui.cleanup(&full_output);

        self.gpu.queue.submit(Some(encoder.finish()));
        frame.present();

        Ok(RenderResult {
            actions: forwarded_actions,
            debug_actions,
            egui_wants_keyboard,
            game_view_focused,
        })
    }
}
