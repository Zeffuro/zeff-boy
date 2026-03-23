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

pub(crate) struct RenderResult {
    pub(crate) open_file_requested: bool,
    pub(crate) save_state_file_requested: bool,
    pub(crate) load_state_file_requested: bool,
    pub(crate) save_state_slot: Option<u8>,
    pub(crate) load_state_slot: Option<u8>,
    pub(crate) debug_actions: crate::debug::DebugUiActions,
}

pub(crate) struct Graphics {
    window: Arc<Window>,
    gpu: GpuContext,
    egui: EguiRenderer,
    framebuffer: FramebufferRenderer,
    size: PhysicalSize<u32>,
    aspect_ratio_mode: AspectRatioMode,
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
            aspect_ratio_mode: AspectRatioMode::Stretch,
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


    pub(crate) fn render(
        &mut self,
        debug_info: Option<&crate::debug::DebugInfo>,
        viewer_data: Option<&crate::debug::DebugViewerData>,
        rom_info_view: Option<&crate::debug::RomInfoViewData>,
        disassembly_view: Option<&crate::debug::DisassemblyView>,
        memory_page: Option<&[(u16, u8)]>,
        debug_windows: &mut crate::debug::DebugWindowState,
        settings: &mut crate::settings::Settings,
        show_settings_window: &mut bool,
        dock_state: &mut egui_dock::DockState<crate::debug::DebugTab>,
    ) -> Result<RenderResult, FrameError> {
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
        let menu_actions =
            crate::debug::draw_menu_bar(self.egui.context(), self.aspect_ratio_mode, dock_state);
        if let Some(mode) = menu_actions.aspect_ratio_mode {
            self.aspect_ratio_mode = mode;
        }
        if menu_actions.open_settings_requested {
            *show_settings_window = true;
        }
        if *show_settings_window {
            crate::debug::draw_settings_window(
                self.egui.context(),
                settings,
                debug_windows,
                show_settings_window,
            );
        }

        let debug_actions;
        if debug_info.is_some() {
            let mut tab_viewer = crate::debug::DebugTabViewer {
                debug_info,
                viewer_data,
                rom_info_view,
                disassembly_view,
                memory_page,
                window_state: debug_windows,
                actions: crate::debug::DebugUiActions::none(),
            };
            egui_dock::DockArea::new(dock_state)
                .style(egui_dock::Style::from_egui(self.egui.context().style().as_ref()))
                .show(self.egui.context(), &mut tab_viewer);
            debug_actions = tab_viewer.actions;
        } else {
            debug_actions = crate::debug::DebugUiActions::none();
            egui::CentralPanel::default().show(self.egui.context(), |ui| {
                ui.centered_and_justified(|ui| {
                    ui.heading("Drag & drop a ROM file, or use File > Open");
                });
            });
        }

        let full_output = self.egui.end_frame(&self.window);
        let menu_bar_height =
            menu_actions.menu_bar_height_points * full_output.full_output.pixels_per_point;

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("main encoder"),
            });

        // Emulator Framebuffer
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

            if debug_info.is_some() {
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
        let (paint_jobs, screen_desc) =
            self.egui.prepare(&self.gpu, &mut encoder, &full_output);

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
            save_state_file_requested: menu_actions.save_state_file_requested,
            load_state_file_requested: menu_actions.load_state_file_requested,
            save_state_slot: menu_actions.save_state_slot,
            load_state_slot: menu_actions.load_state_slot,
            debug_actions,
        })
    }
}
