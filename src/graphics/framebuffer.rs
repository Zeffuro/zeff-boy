use crate::settings::{EffectPreset, ScalingMode};
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
    nearest_sampler: wgpu::Sampler,
    linear_sampler: wgpu::Sampler,
    current_filter: wgpu::FilterMode,
    format: wgpu::TextureFormat,
    current_scaling: ScalingMode,
    current_effect: EffectPreset,
    current_custom_shader_path: String,
    output_texture: wgpu::Texture,
    output_view: wgpu::TextureView,
    offscreen_width: u32,
    offscreen_height: u32,
    effect_pipeline: Option<wgpu::RenderPipeline>,
    intermediate_texture: wgpu::Texture,
    intermediate_view: wgpu::TextureView,
    intermediate_bind_group: wgpu::BindGroup,
    output_bind_group: wgpu::BindGroup,
    two_pass: bool,
}

fn scaling_shader_source(mode: ScalingMode) -> &'static str {
    match mode {
        ScalingMode::PixelPerfect | ScalingMode::Bilinear => concat!(
            include_str!("../shaders/common_vertex.wgsl"),
            include_str!("../shaders/screen.wgsl")
        ),
        ScalingMode::HQ2xLike => concat!(
            include_str!("../shaders/common_vertex.wgsl"),
            include_str!("../shaders/hq2x_like.wgsl")
        ),
        ScalingMode::XBR2x => concat!(
            include_str!("../shaders/common_vertex.wgsl"),
            include_str!("../shaders/xbr2x.wgsl")
        ),
        ScalingMode::Eagle2x => concat!(
            include_str!("../shaders/common_vertex.wgsl"),
            include_str!("../shaders/eagle2x.wgsl")
        ),
    }
}

fn effect_shader_source(preset: EffectPreset) -> &'static str {
    match preset {
        EffectPreset::None => concat!(
            include_str!("../shaders/common_vertex.wgsl"),
            include_str!("../shaders/screen.wgsl")
        ),
        EffectPreset::CRT => concat!(
            include_str!("../shaders/common_vertex.wgsl"),
            include_str!("../shaders/crt.wgsl")
        ),
        EffectPreset::Scanlines => concat!(
            include_str!("../shaders/common_vertex.wgsl"),
            include_str!("../shaders/scanlines.wgsl")
        ),
        EffectPreset::LCDGrid => concat!(
            include_str!("../shaders/common_vertex.wgsl"),
            include_str!("../shaders/lcd_grid.wgsl")
        ),
        EffectPreset::GbcPalette => concat!(
            include_str!("../shaders/common_vertex.wgsl"),
            include_str!("../shaders/gbc_palette.wgsl")
        ),
        EffectPreset::Custom => concat!(
            include_str!("../shaders/common_vertex.wgsl"),
            include_str!("../shaders/screen.wgsl")
        ),
    }
}

fn combined_shader_source(scaling: ScalingMode, effect: EffectPreset) -> &'static str {
    if scaling.is_upscaler() {
        scaling_shader_source(scaling)
    } else {
        effect_shader_source(effect)
    }
}

fn preferred_filter(scaling: ScalingMode) -> wgpu::FilterMode {
    match scaling {
        ScalingMode::Bilinear => wgpu::FilterMode::Linear,
        _ => wgpu::FilterMode::Nearest,
    }
}

fn needs_two_pass(scaling: ScalingMode, effect: EffectPreset) -> bool {
    scaling.is_upscaler() && effect != EffectPreset::None
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

fn create_offscreen_texture(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
    label: &str,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn create_texture_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    params_buffer: &wgpu::Buffer,
    label: &str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: params_buffer.as_entire_binding(),
            },
        ],
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
        let nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("screen sampler nearest"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("screen sampler linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shader params buffer"),
            size: 96,
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

        let screen_bind_group = create_texture_bind_group(
            device,
            &screen_bgl,
            &screen_view,
            &nearest_sampler,
            &params_buffer,
            "screen bind group",
        );

        let scaling = ScalingMode::PixelPerfect;
        let effect = EffectPreset::None;
        let screen_pipeline = create_pipeline(device, &screen_bgl, format, combined_shader_source(scaling, effect));

        let output_texture = create_offscreen_texture(
            device,
            format,
            DEFAULT_OFFSCREEN_WIDTH,
            DEFAULT_OFFSCREEN_HEIGHT,
            "shader output texture",
        );
        let output_view = output_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let intermediate_texture = create_offscreen_texture(
            device,
            format,
            DEFAULT_OFFSCREEN_WIDTH,
            DEFAULT_OFFSCREEN_HEIGHT,
            "shader intermediate texture",
        );
        let intermediate_view =
            intermediate_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let intermediate_bind_group = create_texture_bind_group(
            device,
            &screen_bgl,
            &intermediate_view,
            &nearest_sampler,
            &params_buffer,
            "intermediate bind group",
        );

        let output_bind_group = create_texture_bind_group(
            device,
            &screen_bgl,
            &output_view,
            &nearest_sampler,
            &params_buffer,
            "output bind group",
        );

        Ok(Self {
            screen_texture,
            screen_view,
            screen_bind_group,
            screen_pipeline,
            screen_bgl,
            params_buffer,
            nearest_sampler,
            linear_sampler,
            current_filter: wgpu::FilterMode::Nearest,
            format,
            current_scaling: scaling,
            current_effect: effect,
            current_custom_shader_path: String::new(),
            output_texture,
            output_view,
            offscreen_width: DEFAULT_OFFSCREEN_WIDTH,
            offscreen_height: DEFAULT_OFFSCREEN_HEIGHT,
            effect_pipeline: None,
            intermediate_texture,
            intermediate_view,
            intermediate_bind_group,
            output_bind_group,
            two_pass: false,
        })
    }

    pub(crate) fn set_shader(&mut self, device: &wgpu::Device, settings: &crate::settings::Settings) {
        let scaling = settings.scaling_mode;
        let effect = settings.effect_preset;
        let custom_path_changed = self.current_custom_shader_path != settings.custom_shader_path;
        let desired_filter = preferred_filter(scaling);
        let filter_changed = self.current_filter != desired_filter;

        if self.current_scaling == scaling
            && self.current_effect == effect
            && (!matches!(effect, EffectPreset::Custom) || !custom_path_changed)
            && !filter_changed
        {
            return;
        }

        let want_two_pass = needs_two_pass(scaling, effect);

        if want_two_pass {
            // Two-pass: upscaler pipeline + effect pipeline
            let upscaler_source = scaling_shader_source(scaling);
            let effect_source = if matches!(effect, EffectPreset::Custom) {
                // Custom + upscaler: load custom shader for effect pass
                if settings.custom_shader_path.trim().is_empty() {
                    effect_shader_source(EffectPreset::None)
                } else {
                    match std::fs::read_to_string(&settings.custom_shader_path) {
                        Ok(fragment) => {
                            // We can't return a reference to a local, so handle below
                            let combined = format!(
                                "{}\n{}",
                                include_str!("../shaders/common_vertex.wgsl"),
                                fragment
                            );
                            if self.current_scaling != scaling
                                || self.current_effect != effect
                                || custom_path_changed
                            {
                                self.screen_pipeline =
                                    create_pipeline(device, &self.screen_bgl, self.format, upscaler_source);
                                self.effect_pipeline = Some(create_pipeline(
                                    device,
                                    &self.screen_bgl,
                                    self.format,
                                    &combined,
                                ));
                            }
                            self.two_pass = true;
                            self.current_scaling = scaling;
                            self.current_effect = effect;
                            self.current_custom_shader_path = settings.custom_shader_path.clone();
                            if filter_changed {
                                self.apply_filter_change(device, desired_filter);
                            }
                            return;
                        }
                        Err(err) => {
                            log::warn!(
                                "Failed to load custom shader '{}': {}",
                                settings.custom_shader_path,
                                err
                            );
                            effect_shader_source(EffectPreset::None)
                        }
                    }
                }
            } else {
                effect_shader_source(effect)
            };

            if self.current_scaling != scaling
                || self.current_effect != effect
                || (matches!(effect, EffectPreset::Custom) && custom_path_changed)
            {
                self.screen_pipeline =
                    create_pipeline(device, &self.screen_bgl, self.format, upscaler_source);
                self.effect_pipeline = Some(create_pipeline(
                    device,
                    &self.screen_bgl,
                    self.format,
                    effect_source,
                ));
            }
            self.two_pass = true;
        } else {
            // Single pass
            let mut dynamic_source: Option<String> = None;
            let source = if matches!(effect, EffectPreset::Custom) && !scaling.is_upscaler() {
                if settings.custom_shader_path.trim().is_empty() {
                    combined_shader_source(scaling, EffectPreset::None)
                } else {
                    match std::fs::read_to_string(&settings.custom_shader_path) {
                        Ok(fragment) => {
                            dynamic_source = Some(format!(
                                "{}\n{}",
                                include_str!("../shaders/common_vertex.wgsl"),
                                fragment
                            ));
                            dynamic_source.as_deref().unwrap_or(combined_shader_source(scaling, EffectPreset::None))
                        }
                        Err(err) => {
                            log::warn!(
                                "Failed to load custom shader '{}': {}",
                                settings.custom_shader_path,
                                err
                            );
                            combined_shader_source(scaling, EffectPreset::None)
                        }
                    }
                }
            } else {
                combined_shader_source(scaling, effect)
            };

            if self.current_scaling != scaling
                || self.current_effect != effect
                || (matches!(effect, EffectPreset::Custom) && custom_path_changed)
            {
                self.screen_pipeline = create_pipeline(device, &self.screen_bgl, self.format, source);
            }
            self.effect_pipeline = None;
            self.two_pass = false;
        }

        if filter_changed {
            self.apply_filter_change(device, desired_filter);
        }

        self.current_scaling = scaling;
        self.current_effect = effect;
        self.current_custom_shader_path = settings.custom_shader_path.clone();
    }

    fn apply_filter_change(&mut self, device: &wgpu::Device, desired_filter: wgpu::FilterMode) {
        let sampler = match desired_filter {
            wgpu::FilterMode::Linear => &self.linear_sampler,
            wgpu::FilterMode::Nearest => &self.nearest_sampler,
        };
        self.screen_bind_group = create_texture_bind_group(
            device,
            &self.screen_bgl,
            &self.screen_view,
            sampler,
            &self.params_buffer,
            "screen bind group",
        );
        self.current_filter = desired_filter;
    }

    pub(crate) fn update_params(
        &self,
        queue: &wgpu::Queue,
        settings: &crate::settings::Settings,
    ) {
        let buf = crate::settings::build_gpu_params(
            &settings.shader_params,
            settings.color_correction,
            settings.color_correction_matrix,
        );
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
        if self.two_pass {
            // Two-pass direct rendering: upscaler already rendered to output_texture
            // via render_upscale_pass(). Now draw output_texture with effect shader.
            pass.set_pipeline(self.effect_pipeline.as_ref().unwrap());
            pass.set_bind_group(0, &self.output_bind_group, &[]);
        } else {
            pass.set_pipeline(&self.screen_pipeline);
            pass.set_bind_group(0, &self.screen_bind_group, &[]);
        }
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

        self.output_texture = create_offscreen_texture(device, self.format, w, h, "shader output texture");
        self.output_view = self
            .output_texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.intermediate_texture =
            create_offscreen_texture(device, self.format, w, h, "shader intermediate texture");
        self.intermediate_view = self
            .intermediate_texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.rebuild_aux_bind_groups(device);
    }

    fn rebuild_aux_bind_groups(&mut self, device: &wgpu::Device) {
        self.intermediate_bind_group = create_texture_bind_group(
            device,
            &self.screen_bgl,
            &self.intermediate_view,
            &self.nearest_sampler,
            &self.params_buffer,
            "intermediate bind group",
        );
        self.output_bind_group = create_texture_bind_group(
            device,
            &self.screen_bgl,
            &self.output_view,
            &self.nearest_sampler,
            &self.params_buffer,
            "output bind group",
        );
    }

    pub(crate) fn render_to_offscreen(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.two_pass {
            // Pass 1: Upscaler — screen_texture → intermediate_texture
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("shader upscaler pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.intermediate_view,
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
            // Pass 2: Effect — intermediate_texture → output_texture
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("shader effect pass"),
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
                pass.set_pipeline(self.effect_pipeline.as_ref().unwrap());
                pass.set_bind_group(0, &self.intermediate_bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
        } else {
            // Single pass
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
    }

    /// Render the upscaler pass to `output_texture` for direct-rendering two-pass mode.
    /// Called before `draw()` when `needs_two_pass()` is true.
    pub(crate) fn render_upscale_pass(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shader upscale direct pass"),
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

    pub(crate) fn needs_two_pass(&self) -> bool {
        self.two_pass
    }

    pub(crate) fn output_view(&self) -> &wgpu::TextureView {
        &self.output_view
    }

    pub(crate) fn texture_view(&self) -> &wgpu::TextureView {
        &self.screen_view
    }
}
