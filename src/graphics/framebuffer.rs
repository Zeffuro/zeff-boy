use crate::settings::ShaderPreset;
use anyhow::Result;

const MIN_OFFSCREEN_WIDTH: u32 = 160;
const MIN_OFFSCREEN_HEIGHT: u32 = 144;
const DEFAULT_OFFSCREEN_WIDTH: u32 = 160 * 4;
const DEFAULT_OFFSCREEN_HEIGHT: u32 = 144 * 4;

pub(crate) struct FramebufferRenderer {
    screen_texture: wgpu::Texture,
    screen_view: wgpu::TextureView,
    screen_bind_group: wgpu::BindGroup,
    screen_pipeline: wgpu::RenderPipeline,
    screen_bgl: wgpu::BindGroupLayout,
    params_buffer: wgpu::Buffer,
    format: wgpu::TextureFormat,
    current_preset: ShaderPreset,
    current_custom_shader_path: String,
    output_texture: wgpu::Texture,
    output_view: wgpu::TextureView,
    offscreen_width: u32,
    offscreen_height: u32,
}

fn shader_source_builtin(preset: ShaderPreset) -> &'static str {
    match preset {
        ShaderPreset::None => concat!(
            include_str!("../shaders/common_vertex.wgsl"),
            include_str!("../shaders/screen.wgsl")
        ),
        ShaderPreset::CRT => concat!(
            include_str!("../shaders/common_vertex.wgsl"),
            include_str!("../shaders/crt.wgsl")
        ),
        ShaderPreset::Scanlines => concat!(
            include_str!("../shaders/common_vertex.wgsl"),
            include_str!("../shaders/scanlines.wgsl")
        ),
        ShaderPreset::LCDGrid => concat!(
            include_str!("../shaders/common_vertex.wgsl"),
            include_str!("../shaders/lcd_grid.wgsl")
        ),
        ShaderPreset::HQ2xLike => concat!(
            include_str!("../shaders/common_vertex.wgsl"),
            include_str!("../shaders/hq2x_like.wgsl")
        ),
        ShaderPreset::GbcPalette => concat!(
            include_str!("../shaders/common_vertex.wgsl"),
            include_str!("../shaders/gbc_palette.wgsl")
        ),
        ShaderPreset::Custom => concat!(
            include_str!("../shaders/common_vertex.wgsl"),
            include_str!("../shaders/screen.wgsl")
        ),
    }
}

fn create_pipeline(
    device: &wgpu::Device,
    bgl: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    source: &str,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("screen shader"),
        source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(source)),
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
            size: 32,
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
        let screen_pipeline = create_pipeline(device, &screen_bgl, format, shader_source_builtin(preset));

        let output_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shader output texture"),
            size: wgpu::Extent3d {
                width: DEFAULT_OFFSCREEN_WIDTH,
                height: DEFAULT_OFFSCREEN_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let output_view = output_texture.create_view(&wgpu::TextureViewDescriptor::default());

        Ok(Self {
            screen_texture,
            screen_view,
            screen_bind_group,
            screen_pipeline,
            screen_bgl,
            params_buffer,
            format,
            current_preset: preset,
            current_custom_shader_path: String::new(),
            output_texture,
            output_view,
            offscreen_width: DEFAULT_OFFSCREEN_WIDTH,
            offscreen_height: DEFAULT_OFFSCREEN_HEIGHT,
        })
    }

    pub(crate) fn set_shader(&mut self, device: &wgpu::Device, settings: &crate::settings::Settings) {
        let preset = settings.shader_preset;
        let custom_path_changed = self.current_custom_shader_path != settings.custom_shader_path;
        if self.current_preset == preset
            && (!matches!(preset, ShaderPreset::Custom) || !custom_path_changed)
        {
            return;
        }

        let mut dynamic_source: Option<String> = None;
        let source = if matches!(preset, ShaderPreset::Custom) {
            if settings.custom_shader_path.trim().is_empty() {
                shader_source_builtin(ShaderPreset::None)
            } else {
                match std::fs::read_to_string(&settings.custom_shader_path) {
                    Ok(fragment) => {
                        dynamic_source = Some(format!(
                            "{}\n{}",
                            include_str!("../shaders/common_vertex.wgsl"),
                            fragment
                        ));
                        dynamic_source.as_deref().unwrap_or(shader_source_builtin(ShaderPreset::None))
                    }
                    Err(err) => {
                        log::warn!(
                            "Failed to load custom shader '{}': {}",
                            settings.custom_shader_path,
                            err
                        );
                        shader_source_builtin(ShaderPreset::None)
                    }
                }
            }
        } else {
            shader_source_builtin(preset)
        };

        self.screen_pipeline = create_pipeline(device, &self.screen_bgl, self.format, source);
        self.current_preset = preset;
        self.current_custom_shader_path = settings.custom_shader_path.clone();
    }

    pub(crate) fn update_params(
        &self,
        queue: &wgpu::Queue,
        params: &crate::settings::ShaderParams,
    ) {
        let mut buf = [0u8; 32];
        buf[0..4].copy_from_slice(&params.scanline_intensity.to_le_bytes());
        buf[4..8].copy_from_slice(&params.crt_curvature.to_le_bytes());
        buf[8..12].copy_from_slice(&params.grid_intensity.to_le_bytes());
        buf[12..16].copy_from_slice(&params.upscale_edge_strength.to_le_bytes());
        buf[16..20].copy_from_slice(&params.palette_mix.to_le_bytes());
        buf[20..24].copy_from_slice(&params.palette_warmth.to_le_bytes());
        buf[24..28].copy_from_slice(&160.0_f32.to_le_bytes());
        buf[28..32].copy_from_slice(&144.0_f32.to_le_bytes());
        queue.write_buffer(&self.params_buffer, 0, &buf);
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

    pub(crate) fn resize_offscreen(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let w = width.max(MIN_OFFSCREEN_WIDTH);
        let h = height.max(MIN_OFFSCREEN_HEIGHT);
        if self.offscreen_width == w && self.offscreen_height == h {
            return;
        }
        self.offscreen_width = w;
        self.offscreen_height = h;
        self.output_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shader output texture"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        self.output_view = self
            .output_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
    }

    pub(crate) fn render_to_offscreen(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shader offscreen pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.output_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_viewport(
            0.0,
            0.0,
            self.offscreen_width as f32,
            self.offscreen_height as f32,
            0.0,
            1.0,
        );
        pass.set_pipeline(&self.screen_pipeline);
        pass.set_bind_group(0, &self.screen_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    pub(crate) fn output_view(&self) -> &wgpu::TextureView {
        &self.output_view
    }

    pub(crate) fn texture_view(&self) -> &wgpu::TextureView {
        &self.screen_view
    }
}
