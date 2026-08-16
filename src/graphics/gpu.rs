use anyhow::{Result, anyhow};
use std::sync::Arc;
use winit::window::Window;

use crate::settings::VsyncMode;

pub(crate) struct GpuContext {
    pub(crate) surface: wgpu::Surface<'static>,
    pub(crate) instance: Arc<wgpu::Instance>,
    pub(crate) adapter: Arc<wgpu::Adapter>,
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
    pub(crate) config: wgpu::SurfaceConfiguration,
    present_modes: Vec<wgpu::PresentMode>,
}

impl GpuContext {
    pub(crate) async fn new(
        window: Arc<Window>,
        width: u32,
        height: u32,
        vsync: VsyncMode,
    ) -> Result<Self> {
        #[cfg(not(target_arch = "wasm32"))]
        let instance = wgpu::Instance::default();

        #[cfg(target_arch = "wasm32")]
        let instance = wgpu::util::new_instance_with_webgpu_detection(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY | wgpu::Backends::SECONDARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        })
        .await;

        let surface = instance.create_surface(window)?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("zeff-boy device"),
                required_features: wgpu::Features::empty(),
                #[cfg(not(target_arch = "wasm32"))]
                required_limits: wgpu::Limits::default(),
                #[cfg(target_arch = "wasm32")]
                required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                memory_hints: Default::default(),
                experimental_features: Default::default(),
                trace: wgpu::Trace::Off,
            })
            .await?;

        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| anyhow!("surface not supported by adapter"))?;

        let capabilities = surface.get_capabilities(&adapter);

        if let Some(&fmt) = capabilities.formats.iter().find(|f| !f.is_srgb()) {
            config.format = fmt;
        }

        let present_modes = capabilities.present_modes.clone();
        config.present_mode = vsync.to_present_mode(&present_modes);
        surface.configure(&device, &config);

        Ok(Self {
            surface,
            instance: Arc::new(instance),
            adapter: Arc::new(adapter),
            device: Arc::new(device),
            queue: Arc::new(queue),
            config,
            present_modes,
        })
    }

    pub(crate) fn new_surface(
        &self,
        window: Arc<Window>,
        width: u32,
        height: u32,
        vsync: VsyncMode,
    ) -> Result<Self> {
        let surface = self.instance.create_surface(window)?;
        let mut config = surface
            .get_default_config(&self.adapter, width, height)
            .ok_or_else(|| anyhow!("surface not supported by adapter"))?;
        let capabilities = surface.get_capabilities(&self.adapter);
        if let Some(&format) = capabilities.formats.iter().find(|format| !format.is_srgb()) {
            config.format = format;
        }
        let present_modes = capabilities.present_modes.clone();
        config.present_mode = vsync.to_present_mode(&present_modes);
        surface.configure(&self.device, &config);

        Ok(Self {
            surface,
            instance: self.instance.clone(),
            adapter: self.adapter.clone(),
            device: self.device.clone(),
            queue: self.queue.clone(),
            config,
            present_modes,
        })
    }

    pub(crate) fn set_present_mode(&mut self, vsync: VsyncMode) {
        let mode = vsync.to_present_mode(&self.present_modes);
        if self.config.present_mode != mode {
            self.config.present_mode = mode;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width;
        self.config.height = height;
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
        self.surface.configure(&self.device, &self.config);
    }

    pub(crate) fn clear(&self, color: wgpu::Color) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            _ => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("surface clear"),
            });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("surface clear pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
    }
}
