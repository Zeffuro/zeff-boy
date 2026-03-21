use anyhow::{Result, anyhow};
use std::sync::Arc;
use winit::window::Window;

pub(crate) struct GpuContext {
    pub(crate) surface: wgpu::Surface<'static>,
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) config: wgpu::SurfaceConfiguration,
    present_modes: Vec<wgpu::PresentMode>,
}

impl GpuContext {
    pub(crate) async fn new(window: Arc<Window>, width: u32, height: u32) -> Result<Self> {
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
                experimental_features: Default::default(),
                memory_hints: Default::default(),
                trace: wgpu::Trace::Off,
            })
            .await?;

        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| anyhow!("surface not supported by adapter"))?;

        let capabilities = surface.get_capabilities(&adapter);
        let present_modes = capabilities.present_modes;
        config.present_mode = select_present_mode(false, &present_modes);
        surface.configure(&device, &config);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            present_modes,
        })
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    pub(crate) fn set_uncapped_present_mode(&mut self, uncapped: bool) {
        let desired = select_present_mode(uncapped, &self.present_modes);
        if self.config.present_mode == desired {
            return;
        }

        self.config.present_mode = desired;
        self.surface.configure(&self.device, &self.config);
    }
}

fn select_present_mode(uncapped: bool, supported_modes: &[wgpu::PresentMode]) -> wgpu::PresentMode {
    if uncapped {
        if supported_modes.contains(&wgpu::PresentMode::AutoNoVsync) {
            return wgpu::PresentMode::AutoNoVsync;
        }
        if supported_modes.contains(&wgpu::PresentMode::Immediate) {
            return wgpu::PresentMode::Immediate;
        }
        if supported_modes.contains(&wgpu::PresentMode::Mailbox) {
            return wgpu::PresentMode::Mailbox;
        }
    }

    if supported_modes.contains(&wgpu::PresentMode::AutoVsync) {
        return wgpu::PresentMode::AutoVsync;
    }
    if supported_modes.contains(&wgpu::PresentMode::Fifo) {
        return wgpu::PresentMode::Fifo;
    }

    supported_modes
        .first()
        .copied()
        .unwrap_or(wgpu::PresentMode::Fifo)
}

pub(crate) fn texture_sampler_bind_group_layout(
    device: &wgpu::Device,
    label: &str,
) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    multisampled: false,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

