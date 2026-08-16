use anyhow::Result;
use std::sync::Arc;
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{Window, WindowAttributes, WindowId},
};

#[cfg(not(target_arch = "wasm32"))]
mod debugger_window;
mod egui_integration;
mod framebuffer;
mod gpu;
mod pipeline;
mod render_frame;
#[cfg(not(target_arch = "wasm32"))]
mod settings_window;
mod viewport;

use egui_integration::EguiRenderer;
use framebuffer::FramebufferRenderer;
use gpu::GpuContext;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use debugger_window::{DebuggerRenderContext, DebuggerRenderResult};
pub(crate) use render_frame::{FrameError, RenderContext};
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use settings_window::SettingsRenderContext;
pub(crate) use viewport::AspectRatioMode;

use crate::settings::VsyncMode;

pub(crate) struct Graphics {
    window: Arc<Window>,
    gpu: GpuContext,
    egui: EguiRenderer,
    framebuffer: FramebufferRenderer,
    size: PhysicalSize<u32>,
    aspect_ratio_mode: AspectRatioMode,
    game_egui_texture_id: Option<egui::TextureId>,
    game_view_pixel_size: Option<(u32, u32)>,
    last_direct_game_viewport: Option<(f32, f32, f32, f32, u32, u32)>,
    #[cfg(not(target_arch = "wasm32"))]
    debugger: Option<debugger_window::DebuggerWindow>,
    #[cfg(not(target_arch = "wasm32"))]
    settings_window: Option<settings_window::SettingsWindow>,
}

impl Graphics {
    pub(crate) fn create_window(event_loop: &ActiveEventLoop) -> Result<Arc<Window>> {
        let title = format!("zeff-boy v{}", env!("CARGO_PKG_VERSION"));
        #[allow(unused_mut)]
        let mut attrs = WindowAttributes::default().with_title(title);

        #[cfg(not(target_arch = "wasm32"))]
        {
            if crate::live_control::automation_mode_enabled() {
                attrs = attrs
                    .with_active(false)
                    .with_decorations(false)
                    .with_position(PhysicalPosition::new(-32_000, -32_000));

                #[cfg(target_os = "windows")]
                {
                    use winit::platform::windows::WindowAttributesExtWindows as _;

                    attrs = attrs.with_skip_taskbar(true);
                }
            }

            if let Some(icon) = Self::load_window_icon() {
                attrs = attrs.with_window_icon(Some(icon));
            }
        }

        let window = Arc::new(event_loop.create_window(attrs)?);

        #[cfg(not(target_arch = "wasm32"))]
        if crate::live_control::automation_mode_enabled() {
            window.set_minimized(true);
        }

        Ok(window)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn load_window_icon() -> Option<winit::window::Icon> {
        use std::io::Cursor;

        static ICON_PNG: &[u8] = include_bytes!("../../assets/icon.png");

        let decoder = png::Decoder::new(Cursor::new(ICON_PNG));
        let mut reader = decoder.read_info().ok()?;
        let mut buf = vec![0u8; reader.output_buffer_size()?];
        let info = reader.next_frame(&mut buf).ok()?;
        buf.truncate(info.buffer_size());

        let rgba = match info.color_type {
            png::ColorType::Rgba => buf,
            png::ColorType::Rgb => {
                let mut rgba = Vec::with_capacity(buf.len() / 3 * 4);
                for chunk in buf.chunks_exact(3) {
                    rgba.extend_from_slice(chunk);
                    rgba.push(255);
                }
                rgba
            }
            _ => return None,
        };

        winit::window::Icon::from_rgba(rgba, info.width, info.height).ok()
    }

    pub(crate) async fn new(window: Arc<Window>, vsync: VsyncMode) -> Result<Self> {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        let gpu = GpuContext::new(window.clone(), width, height, vsync).await?;
        let egui = EguiRenderer::new(&window, &gpu.device, gpu.config.format)?;
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
            last_direct_game_viewport: None,
            #[cfg(not(target_arch = "wasm32"))]
            debugger: None,
            #[cfg(not(target_arch = "wasm32"))]
            settings_window: None,
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

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn open_debugger_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        settings: &crate::settings::Settings,
    ) -> Result<WindowId> {
        if let Some(debugger) = self.debugger.as_ref() {
            debugger.window().focus_window();
            return Ok(debugger.id());
        }
        let debugger = debugger_window::DebuggerWindow::new(event_loop, &self.gpu, settings)?;
        let id = debugger.id();
        self.debugger = Some(debugger);
        Ok(id)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn close_debugger_window(&mut self) {
        self.debugger = None;
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn debugger_window_id(&self) -> Option<WindowId> {
        self.debugger
            .as_ref()
            .map(debugger_window::DebuggerWindow::id)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn debugger_window(&self) -> Option<&Window> {
        self.debugger
            .as_ref()
            .map(debugger_window::DebuggerWindow::window)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn focus_debugger_window(&self) {
        if let Some(window) = self.debugger_window() {
            window.focus_window();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn debugger_handles_event(&mut self, event: &WindowEvent) -> bool {
        self.debugger
            .as_mut()
            .is_some_and(|debugger| debugger.handle_event(event))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn resize_debugger(&mut self, width: u32, height: u32) {
        if let Some(debugger) = self.debugger.as_mut() {
            debugger.resize(width, height);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn render_debugger(
        &mut self,
        ctx: DebuggerRenderContext<'_>,
    ) -> Result<DebuggerRenderResult, FrameError> {
        self.debugger.as_mut().ok_or(FrameError::Lost)?.render(ctx)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn open_settings_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        settings: &crate::settings::Settings,
    ) -> Result<WindowId> {
        if let Some(window) = self.settings_window.as_ref() {
            window.window().focus_window();
            return Ok(window.id());
        }
        let window = settings_window::SettingsWindow::new(event_loop, &self.gpu, settings)?;
        let id = window.id();
        self.settings_window = Some(window);
        Ok(id)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn close_settings_window(&mut self) {
        self.settings_window = None;
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn settings_window_id(&self) -> Option<WindowId> {
        self.settings_window
            .as_ref()
            .map(settings_window::SettingsWindow::id)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn settings_window(&self) -> Option<&Window> {
        self.settings_window
            .as_ref()
            .map(settings_window::SettingsWindow::window)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn settings_handles_event(&mut self, event: &WindowEvent) -> bool {
        self.settings_window
            .as_mut()
            .is_some_and(|window| window.handle_event(event))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn resize_settings_window(&mut self, width: u32, height: u32) {
        if let Some(window) = self.settings_window.as_mut() {
            window.resize(width, height);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn render_settings_window(
        &mut self,
        ctx: SettingsRenderContext<'_>,
    ) -> Result<(), FrameError> {
        self.settings_window
            .as_mut()
            .ok_or(FrameError::Lost)?
            .render(ctx)
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

    pub(crate) fn game_pixel_at_window_pos(&self, x: f32, y: f32) -> Option<(u32, u32)> {
        let (vx, vy, vw, vh, native_w, native_h) = self.last_direct_game_viewport?;
        if vw <= 0.0 || vh <= 0.0 || x < vx || y < vy || x >= vx + vw || y >= vy + vh {
            return None;
        }

        let px = ((x - vx) * native_w as f32 / vw).floor() as u32;
        let py = ((y - vy) * native_h as f32 / vh).floor() as u32;
        Some((
            px.min(native_w.saturating_sub(1)),
            py.min(native_h.saturating_sub(1)),
        ))
    }
}
