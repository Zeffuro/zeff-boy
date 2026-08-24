use crate::debug::{self, DebugTab, DebugTabViewer, DebugUiActions, DebugWindowState, MenuAction};
use crate::debug::{DebugDataRefs, ToastManager};
use crate::rom_archive::{ArchiveSelectionAction, PendingArchiveSelection};

use super::Graphics;
use super::viewport::calculate_viewport;

pub(crate) enum FrameError {
    Timeout,
    Outdated,
    Lost,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct PackageLoadView {
    pub(crate) filename: String,
    pub(crate) phase: &'static str,
    pub(crate) completed_bytes: u64,
    pub(crate) total_bytes: u64,
    pub(crate) eta: Option<String>,
}

pub(crate) struct RenderContext<'a> {
    pub(crate) data: DebugDataRefs<'a>,
    pub(crate) active_system: Option<crate::emu_backend::ActiveSystem>,
    pub(crate) media_slot_snapshot: Option<&'a zeff_emu_common::media::MediaSlotSnapshot>,
    pub(crate) media_event_change_allowed: bool,
    pub(crate) game_boy_serial_device: zeff_gb_core::hardware::GameBoySerialDevice,
    pub(crate) game_boy_serial_device_change_allowed: bool,
    pub(crate) debug_windows: &'a mut DebugWindowState,
    pub(crate) settings: &'a mut crate::settings::Settings,
    #[cfg(target_arch = "wasm32")]
    pub(crate) nes_palette_file_slot: crate::platform::FileDataSlot,
    pub(crate) show_settings_window: &'a mut bool,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) show_mods_window: &'a mut bool,
    #[cfg(target_arch = "wasm32")]
    pub(crate) show_printer_window: &'a mut bool,
    pub(crate) dock_state: &'a mut egui_dock::DockState<DebugTab>,
    pub(crate) toast_manager: &'a mut ToastManager,
    pub(crate) speed_mode_label: Option<&'a str>,
    pub(crate) is_recording_audio: bool,
    pub(crate) is_recording_replay: bool,
    pub(crate) is_playing_replay: bool,
    pub(crate) supports_save_states: bool,
    pub(crate) supports_rewind: bool,
    pub(crate) supports_replay: bool,
    pub(crate) supports_audio: bool,
    pub(crate) supports_debugger: bool,
    pub(crate) supports_execution_controls: bool,
    pub(crate) is_rewinding: bool,
    pub(crate) rewind_seconds_back: f32,
    pub(crate) is_paused: bool,
    pub(crate) ws_display_rotated: bool,
    #[cfg(target_arch = "wasm32")]
    pub(crate) is_pocket_camera: bool,
    pub(crate) autohide_menu_bar: bool,
    pub(crate) cursor_y: Option<f32>,
    pub(crate) slot_labels: &'a [String; 10],
    pub(crate) slot_occupied: [bool; 10],
    pub(crate) active_save_slot: u8,
    pub(crate) can_undo_load_state: bool,
    pub(crate) archive_selection: Option<&'a PendingArchiveSelection>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) package_load: Option<PackageLoadView>,
    pub(crate) show_debug_dock: bool,
    pub(crate) debugger_window_open: bool,
    pub(crate) debug_presentation: crate::settings::DebugPresentation,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) update_info: Option<&'a crate::update::UpdateInfo>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) show_update_dialog: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) update_install_state: crate::update::UpdateInstallState,
}

pub(crate) struct RenderResult {
    pub(crate) actions: Vec<MenuAction>,
    pub(crate) debug_actions: DebugUiActions,
    pub(crate) archive_selection_action: Option<ArchiveSelectionAction>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) cancel_package_load: bool,
    pub(crate) egui_wants_keyboard: bool,
    pub(crate) game_view_focused: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) update_action: Option<crate::update::UpdateAction>,
}

const EMPTY_STATE_MESSAGE: &str = "Drag & drop a ROM file, or use File > Open";

impl Graphics {
    fn acquire_surface_frame(
        &self,
    ) -> Result<(wgpu::SurfaceTexture, wgpu::TextureView), FrameError> {
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

    fn draw_archive_selection_window(
        &self,
        ctx_egui: &egui::Context,
        selection: &PendingArchiveSelection,
    ) -> Option<ArchiveSelectionAction> {
        let mut action = None;
        let archive_name = selection
            .archive_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("archive");

        egui::Window::new("Select ROM from archive")
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .show(ctx_egui, |ui| {
                ui.label(format!(
                    "{} contains multiple supported ROMs. Choose one to load:",
                    archive_name
                ));
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(280.0)
                    .show(ui, |ui| {
                        for entry in &selection.entries {
                            if ui.button(entry.display_label()).clicked() {
                                action = Some(ArchiveSelectionAction::Load {
                                    archive_path: selection.archive_path.clone(),
                                    entry_index: entry.index,
                                });
                            }
                        }
                    });
                ui.separator();
                if ui.button("Cancel").clicked() {
                    action = Some(ArchiveSelectionAction::Cancel);
                }
            });
        ctx_egui.request_repaint();
        action
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn draw_package_load_modal(&self, ctx_egui: &egui::Context, load: &PackageLoadView) -> bool {
        let mut cancel = false;
        let confirm_id = egui::Id::new("archive_load_cancel_confirmation");
        egui::Modal::new(egui::Id::new("archive_load")).show(ctx_egui, |ui| {
            ui.set_min_width(360.0);
            ui.heading("Loading archive");
            ui.label(&load.filename);
            ui.add_space(8.0);
            ui.label(load.phase);
            if load.total_bytes == 0 {
                ui.spinner();
            } else {
                let fraction = load.completed_bytes as f32 / load.total_bytes as f32;
                ui.add(
                    egui::ProgressBar::new(fraction.clamp(0.0, 1.0))
                        .show_percentage()
                        .animate(true),
                );
            }
            if let Some(eta) = &load.eta {
                ui.label(eta);
            }
            ui.add_space(8.0);
            let confirming = ui
                .ctx()
                .data(|data| data.get_temp::<bool>(confirm_id))
                .unwrap_or(false);
            if confirming {
                ui.label("Cancel this load?");
                ui.horizontal(|ui| {
                    if ui.button("Keep loading").clicked() {
                        ui.ctx().data_mut(|data| data.remove::<bool>(confirm_id));
                    }
                    if ui.button("Cancel load").clicked() {
                        ui.ctx().data_mut(|data| data.remove::<bool>(confirm_id));
                        cancel = true;
                    }
                });
            } else if ui.button("Cancel").clicked() {
                ui.ctx().data_mut(|data| data.insert_temp(confirm_id, true));
            }
        });
        ctx_egui.request_repaint();
        cancel
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn draw_update_window(
        &self,
        info: &crate::update::UpdateInfo,
        install_state: crate::update::UpdateInstallState,
    ) -> Option<crate::update::UpdateAction> {
        let mut action = None;
        egui::Window::new("Update available")
            .collapsible(false)
            .resizable(false)
            .default_width(360.0)
            .show(self.egui.context(), |ui| {
                ui.label(format!(
                    "zeff-boy {} is available. You are running {}.",
                    info.version,
                    env!("CARGO_PKG_VERSION")
                ));
                ui.add_space(8.0);
                match &info.strategy {
                    crate::update::UpdateStrategy::SelfUpdate(_) => match install_state {
                        crate::update::UpdateInstallState::Idle => {
                            if ui.button("Install Update").clicked() {
                                action = Some(crate::update::UpdateAction::Install);
                            }
                        }
                        crate::update::UpdateInstallState::Downloading => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Downloading and verifying update...");
                            });
                        }
                        crate::update::UpdateInstallState::Ready => {
                            if ui.button("Restart and Update").clicked() {
                                action = Some(crate::update::UpdateAction::Restart);
                            }
                        }
                    },
                    crate::update::UpdateStrategy::PackageManager { name, command } => {
                        ui.label(format!("This copy is managed by {name}."));
                        if let Some(command) = command {
                            ui.horizontal(|ui| {
                                ui.code(command);
                                if ui.button("Copy").clicked() {
                                    ui.ctx().copy_text(command.clone());
                                }
                            });
                        } else {
                            ui.label("Install the update through that package manager.");
                        }
                    }
                    crate::update::UpdateStrategy::Browser => {
                        if ui.button("Download Update").clicked() {
                            action = Some(crate::update::UpdateAction::Download);
                        }
                    }
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Release Notes").clicked() {
                        action = Some(crate::update::UpdateAction::ReleaseNotes);
                    }
                    if ui.button("Later").clicked() {
                        action = Some(crate::update::UpdateAction::Later);
                    }
                    if ui.button("Skip This Version").clicked() {
                        action = Some(crate::update::UpdateAction::SkipVersion);
                    }
                });
            });
        action
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

            let direct_viewport = if render_framebuffer_directly {
                let (gw, gh) = self.framebuffer.native_size();
                calculate_viewport(
                    self.aspect_ratio_mode,
                    self.gpu.config.width,
                    self.gpu.config.height,
                    gw,
                    gh,
                    menu_bar_height,
                )
                .map(|(x, y, w, h)| (x, y, w, h, gw, gh))
            } else {
                None
            };
            self.last_direct_game_viewport = direct_viewport;

            if let Some((x, y, w, h, _, _)) = direct_viewport {
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
            .update_params(&self.gpu.queue, ctx.settings, ctx.active_system);

        let (frame, view) = self.acquire_surface_frame()?;

        self.egui.begin_frame(&self.window);
        self.egui.apply_style(
            ctx.settings.ui.theme_preset,
            ctx.settings.ui.ui_density,
            ctx.settings.ui.debug_monospace_scale,
            ctx.settings.ui.effective_debug_colors(),
        );

        let base_ppp = self.window.scale_factor() as f32;
        let target_ppp = base_ppp * ctx.settings.ui.ui_scale.clamp(0.5, 3.0);
        if (self.egui.context().pixels_per_point() - target_ppp).abs() > 0.01 {
            self.egui.context().set_pixels_per_point(target_ppp);
        }

        let mut root_ui = egui::Ui::new(
            self.egui.context().clone(),
            egui::Id::new("main_root_ui"),
            egui::UiBuilder::new()
                .layer_id(egui::LayerId::background())
                .max_rect(self.egui.context().content_rect()),
        );

        let show_menu = if ctx.autohide_menu_bar {
            let pointer_near_top = ctx.cursor_y.is_some_and(|y| y < 8.0);
            let any_menu_open = egui::Popup::is_any_open(self.egui.context());
            pointer_near_top || any_menu_open
        } else {
            true
        };

        let menu_actions = if show_menu {
            debug::draw_menu_bar(
                &mut root_ui,
                &debug::MenuBarContext {
                    current_mode: self.aspect_ratio_mode,
                    speed_mode_label: ctx.speed_mode_label,
                    is_recording_audio: ctx.is_recording_audio,
                    is_recording_replay: ctx.is_recording_replay,
                    is_playing_replay: ctx.is_playing_replay,
                    supports_save_states: ctx.supports_save_states,
                    supports_replay: ctx.supports_replay,
                    supports_audio: ctx.supports_audio,
                    supports_debugger: ctx.supports_debugger,
                    is_paused: ctx.is_paused,
                    active_system: ctx
                        .active_system
                        .unwrap_or(crate::emu_backend::ActiveSystem::GameBoy),
                    media_slot_snapshot: ctx.media_slot_snapshot,
                    media_event_change_allowed: ctx.media_event_change_allowed,
                    game_boy_serial_device: ctx.game_boy_serial_device,
                    game_boy_serial_device_change_allowed: ctx
                        .game_boy_serial_device_change_allowed,
                    ws_display_rotated: ctx.ws_display_rotated,
                    slot_labels: ctx.slot_labels,
                    slot_occupied: &ctx.slot_occupied,
                    active_save_slot: ctx.active_save_slot,
                    can_undo_load_state: ctx.can_undo_load_state,
                    external_debugger: !ctx.show_debug_dock,
                    debugger_window_open: ctx.debugger_window_open,
                    debug_presentation: ctx.debug_presentation,
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
                #[cfg(not(target_arch = "wasm32"))]
                MenuAction::OpenMods => *ctx.show_mods_window = true,
                other => forwarded_actions.push(other),
            }
        }

        let barcode_boy_selected =
            ctx.game_boy_serial_device == zeff_gb_core::hardware::GameBoySerialDevice::BarcodeBoy;
        if !barcode_boy_selected {
            ctx.debug_windows.barcode_boy_scan_open = false;
        } else if let Some(digits) = debug::draw_barcode_boy_scan_window(
            self.egui.context(),
            ctx.debug_windows,
            ctx.game_boy_serial_device_change_allowed,
        ) {
            forwarded_actions.push(MenuAction::TriggerBarcodeBoyScan(digits));
        }

        let content_rect = root_ui.available_rect_before_wrap();
        let content_min = content_rect.min;
        let content_size = content_rect.size();
        #[cfg(target_arch = "wasm32")]
        let content_bounds = content_rect;

        #[cfg(target_arch = "wasm32")]
        let gb_hardware_mode_label = ctx.data.perf_info.and_then(|perf| {
            if perf.platform_name != "Game Boy" {
                None
            } else {
                Some(perf.hardware_label.as_ref())
            }
        });

        #[cfg(target_arch = "wasm32")]
        if *ctx.show_settings_window {
            debug::draw_settings_window(
                self.egui.context(),
                ctx.settings,
                ctx.debug_windows,
                ctx.show_settings_window,
                content_bounds,
                &debug::SettingsContext {
                    active_system: ctx.active_system,
                    gb_hardware_mode_label,
                    is_pocket_camera: ctx.is_pocket_camera,
                    #[cfg(target_arch = "wasm32")]
                    nes_palette_file_slot: ctx.nes_palette_file_slot.clone(),
                },
            );
        }

        #[cfg(target_arch = "wasm32")]
        if *ctx.show_printer_window {
            debug::draw_printer_window(
                self.egui.context(),
                &mut ctx.debug_windows.printer,
                ctx.show_printer_window,
                content_bounds,
            );
        }

        let mut debug_actions;
        let has_any_emu_data = ctx.active_system.is_some()
            || ctx.data.cpu_debug.is_some()
            || ctx.data.perf_info.is_some()
            || ctx.data.memory_page.is_some()
            || ctx.data.rom_page.is_some();

        if has_any_emu_data && ctx.show_debug_dock {
            let has_game_view = debug::is_tab_open(ctx.dock_state, DebugTab::GameView);
            let (game_texture_id, _) =
                self.ensure_game_texture(has_game_view, ctx.settings.video.offscreen_scale);

            let mut tab_viewer = DebugTabViewer {
                data: ctx.data,
                window_state: ctx.debug_windows,
                actions: DebugUiActions::none(),
                supports_rewind: ctx.supports_rewind,
                supports_debugger: ctx.supports_debugger,
                supports_execution_controls: ctx.supports_execution_controls,
                game_texture_id,
                game_native_size: self.framebuffer.native_size(),
                aspect_ratio_mode: self.aspect_ratio_mode,
                game_view_pixel_size: None,
            };

            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(&mut root_ui, |ui| {
                    egui_dock::DockArea::new(ctx.dock_state)
                        .secondary_button_on_modifier(false)
                        .style(super::egui_integration::dock_style(
                            self.egui.context(),
                            ctx.settings.ui.ui_density,
                        ))
                        .show_inside(ui, &mut tab_viewer)
                });
            debug_actions = tab_viewer.actions;
            if let Some(tab) = debug_actions.focus_tab.take() {
                debug::activate_dock_tab(ctx.dock_state, tab);
            }

            if let Some(size) = tab_viewer.game_view_pixel_size {
                self.game_view_pixel_size = Some(size);
            }
        } else if !has_any_emu_data {
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
        } else {
            debug_actions = DebugUiActions::none();
        }

        ctx.toast_manager.set_recording(ctx.is_recording_audio);
        let archive_selection_action = ctx.archive_selection.and_then(|selection| {
            self.draw_archive_selection_window(self.egui.context(), selection)
        });
        #[cfg(not(target_arch = "wasm32"))]
        if ctx.package_load.is_none() {
            self.egui.context().data_mut(|data| {
                data.remove::<bool>(egui::Id::new("archive_load_cancel_confirmation"));
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        let cancel_package_load = ctx
            .package_load
            .as_ref()
            .is_some_and(|load| self.draw_package_load_modal(self.egui.context(), load));
        #[cfg(not(target_arch = "wasm32"))]
        let update_action = ctx
            .update_info
            .filter(|_| ctx.show_update_dialog)
            .and_then(|info| self.draw_update_window(info, ctx.update_install_state));
        ctx.toast_manager.draw(self.egui.context());

        if ctx.is_rewinding {
            self.draw_rewind_overlay(self.egui.context(), ctx.rewind_seconds_back);
        }

        let egui_wants_keyboard = self.egui.context().egui_wants_keyboard_input();

        let has_game_view_in_dock = has_any_emu_data
            && ctx.show_debug_dock
            && debug::is_tab_open(ctx.dock_state, DebugTab::GameView);
        let render_framebuffer_directly = has_any_emu_data && !has_game_view_in_dock;

        let game_view_focused = if render_framebuffer_directly {
            true
        } else if has_any_emu_data && ctx.show_debug_dock {
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
            archive_selection_action,
            #[cfg(not(target_arch = "wasm32"))]
            cancel_package_load,
            egui_wants_keyboard,
            game_view_focused,
            #[cfg(not(target_arch = "wasm32"))]
            update_action,
        })
    }
}
