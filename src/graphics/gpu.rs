use anyhow::{Result, anyhow};
use std::sync::Arc;
use winit::window::Window;

use crate::settings::VsyncMode;

pub(crate) struct GpuContext {
    pub(crate) surface: wgpu::Surface<'static>,
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
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
        let instance = wgpu::Instance::default();
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
                required_limits: wgpu::Limits::default(),
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
            device,
            queue,
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
        let _ = self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
        self.surface.configure(&self.device, &self.config);
    }
}
