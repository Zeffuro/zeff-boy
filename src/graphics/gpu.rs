use anyhow::{Result, anyhow};
use std::sync::Arc;
use winit::window::Window;

pub(crate) struct GpuContext {
    pub(crate) surface: wgpu::Surface<'static>,
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) config: wgpu::SurfaceConfiguration,
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
                memory_hints: Default::default(),
                experimental_features: Default::default(),
                trace: wgpu::Trace::Off,
            })
            .await?;

        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| anyhow!("surface not supported by adapter"))?;

        let capabilities = surface.get_capabilities(&adapter);

        // Prefer a non-sRGB surface format so egui colours are correct
        // (egui outputs sRGB-space colours directly and does not want the GPU
        // to apply an additional sRGB conversion on write).
        if let Some(&fmt) = capabilities.formats.iter().find(|f| !f.is_srgb()) {
            config.format = fmt;
        }

        // Always use vsync.  Switching present modes at runtime is unreliable
        // on systems with overlay layers (OBS, Steam, Overwolf, NVIDIA Optimus)
        // and causes wgpu device-lost panics.  Speed control is handled
        // entirely through frame count in compute_frames_to_step().
        config.present_mode = if capabilities.present_modes.contains(&wgpu::PresentMode::AutoVsync)
        {
            wgpu::PresentMode::AutoVsync
        } else {
            wgpu::PresentMode::Fifo
        };
        surface.configure(&device, &config);

        Ok(Self {
            surface,
            device,
            queue,
            config,
        })
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
