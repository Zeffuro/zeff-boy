use crate::settings::ShaderPreset;
use anyhow::Result;

pub(crate) struct FramebufferRenderer {
    screen_texture: wgpu::Texture,
    screen_bind_group: wgpu::BindGroup,
    screen_pipeline: wgpu::RenderPipeline,
    screen_bgl: wgpu::BindGroupLayout,
    params_buffer: wgpu::Buffer,
    format: wgpu::TextureFormat,
    current_preset: ShaderPreset,
}

fn shader_source(preset: ShaderPreset) -> &'static str {
    match preset {
        ShaderPreset::None => concat!(include_str!("../shaders/common_vertex.wgsl"), include_str!("../shaders/screen.wgsl")),
        ShaderPreset::CRT => concat!(include_str!("../shaders/common_vertex.wgsl"), include_str!("../shaders/crt.wgsl")),
        ShaderPreset::Scanlines => concat!(include_str!("../shaders/common_vertex.wgsl"), include_str!("../shaders/scanlines.wgsl")),
        ShaderPreset::LCDGrid => concat!(include_str!("../shaders/common_vertex.wgsl"), include_str!("../shaders/lcd_grid.wgsl")),
    }
}

fn create_pipeline(
    device: &wgpu::Device,
    bgl: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    preset: ShaderPreset,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("screen shader"),
        source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(shader_source(preset))),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("screen pipeline layout"),
        bind_group_layouts: &[bgl],
        push_constant_ranges: &[],
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("screen pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        cache: None,
        multiview: None,
    })
}

impl FramebufferRenderer {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Result<Self> {
        let screen_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("screen texture"),
            size: wgpu::Extent3d {
                width: 160,
                height: 144,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let screen_view = screen_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let screen_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("screen sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shader params buffer"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let screen_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("screen bgl"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let screen_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("screen bind group"),
            layout: &screen_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&screen_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&screen_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params_buffer.as_entire_binding(),
                },
            ],
        });

        let preset = ShaderPreset::None;
        let screen_pipeline = create_pipeline(device, &screen_bgl, format, preset);

        Ok(Self {
            screen_texture,
            screen_bind_group,
            screen_pipeline,
            screen_bgl,
            params_buffer,
            format,
            current_preset: preset,
        })
    }

    pub(crate) fn set_shader(&mut self, device: &wgpu::Device, preset: ShaderPreset) {
        if self.current_preset == preset {
            return;
        }
        self.screen_pipeline = create_pipeline(device, &self.screen_bgl, self.format, preset);
        self.current_preset = preset;
    }


    pub(crate) fn update_params(&self, queue: &wgpu::Queue, params: &crate::settings::ShaderParams) {
        queue.write_buffer(&self.params_buffer, 0, &params.to_gpu_bytes());
    }

    pub(crate) fn upload_framebuffer(&self, queue: &wgpu::Queue, framebuffer: &[u8]) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.screen_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            framebuffer,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * 160),
                rows_per_image: Some(144),
            },
            wgpu::Extent3d {
                width: 160,
                height: 144,
                depth_or_array_layers: 1,
            },
        );
    }

    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, x: f32, y: f32, w: f32, h: f32) {
        pass.set_viewport(x, y, w, h, 0.0, 1.0);
        pass.set_pipeline(&self.screen_pipeline);
        pass.set_bind_group(0, &self.screen_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
