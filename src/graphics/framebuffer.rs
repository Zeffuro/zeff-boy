use crate::settings::{EffectPreset, ScalingMode};
use anyhow::Result;

const MIN_OFFSCREEN_WIDTH: u32 = 160;
const MIN_OFFSCREEN_HEIGHT: u32 = 144;
const DEFAULT_OFFSCREEN_WIDTH: u32 = 160 * 4;
const DEFAULT_OFFSCREEN_HEIGHT: u32 = 144 * 4;

struct ScreenInput {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    bind_group_no_cc: wgpu::BindGroup,
    native_width: u32,
    native_height: u32,
}

struct ShaderState {
    pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    effect_pipeline: Option<wgpu::RenderPipeline>,
    format: wgpu::TextureFormat,
    current_scaling: ScalingMode,
    current_effect: EffectPreset,
    current_custom_shader_path: String,
    two_pass: bool,
}

struct SamplerResources {
    params_buffer: wgpu::Buffer,
    params_buffer_no_cc: wgpu::Buffer,
    nearest_sampler: wgpu::Sampler,
    linear_sampler: wgpu::Sampler,
    current_filter: wgpu::FilterMode,
}

struct OffscreenTarget {
    output_texture: wgpu::Texture,
    output_view: wgpu::TextureView,
    intermediate_texture: wgpu::Texture,
    intermediate_view: wgpu::TextureView,
    intermediate_bind_group: wgpu::BindGroup,
    output_bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

pub(crate) struct FramebufferRenderer {
    screen: ScreenInput,
    shader: ShaderState,
    sampler: SamplerResources,
    offscreen: OffscreenTarget,
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
        EffectPreset::Crt => concat!(
            include_str!("../shaders/common_vertex.wgsl"),
            include_str!("../shaders/crt.wgsl")
        ),
        EffectPreset::Scanlines => concat!(
            include_str!("../shaders/common_vertex.wgsl"),
            include_str!("../shaders/scanlines.wgsl")
        ),
        EffectPreset::LcdGrid => concat!(
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
        bind_group_layouts: &[Some(bgl)],
        immediate_size: 0,
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
        multiview_mask: None,
        cache: None,
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
        let params_buffer_no_cc = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shader params buffer no color correction"),
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
        let screen_bind_group_no_cc = create_texture_bind_group(
            device,
            &screen_bgl,
            &screen_view,
            &nearest_sampler,
            &params_buffer_no_cc,
            "screen bind group no color correction",
        );

        let scaling = ScalingMode::PixelPerfect;
        let effect = EffectPreset::None;
        let screen_pipeline = create_pipeline(
            device,
            &screen_bgl,
            format,
            combined_shader_source(scaling, effect),
        );

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
            screen: ScreenInput {
                texture: screen_texture,
                view: screen_view,
                bind_group: screen_bind_group,
                bind_group_no_cc: screen_bind_group_no_cc,
                native_width: 160,
                native_height: 144,
            },
            shader: ShaderState {
                pipeline: screen_pipeline,
                bgl: screen_bgl,
                effect_pipeline: None,
                format,
                current_scaling: scaling,
                current_effect: effect,
                current_custom_shader_path: String::new(),
                two_pass: false,
            },
            sampler: SamplerResources {
                params_buffer,
                params_buffer_no_cc,
                nearest_sampler,
                linear_sampler,
                current_filter: wgpu::FilterMode::Nearest,
            },
            offscreen: OffscreenTarget {
                output_texture,
                output_view,
                intermediate_texture,
                intermediate_view,
                intermediate_bind_group,
                output_bind_group,
                width: DEFAULT_OFFSCREEN_WIDTH,
                height: DEFAULT_OFFSCREEN_HEIGHT,
            },
        })
    }

    pub(crate) fn set_shader(
        &mut self,
        device: &wgpu::Device,
        settings: &crate::settings::Settings,
    ) {
        let scaling = settings.video.scaling_mode;
        let effect = settings.video.effect_preset;
        let custom_path_changed = self.shader.current_custom_shader_path != settings.video.custom_shader_path;
        let desired_filter = preferred_filter(scaling);
        let filter_changed = self.sampler.current_filter != desired_filter;

        if self.shader.current_scaling == scaling
            && self.shader.current_effect == effect
            && (!matches!(effect, EffectPreset::Custom) || !custom_path_changed)
            && !filter_changed
        {
            return;
        }

        let want_two_pass = needs_two_pass(scaling, effect);

        if want_two_pass {
            let upscaler_source = scaling_shader_source(scaling);
            let effect_source = if matches!(effect, EffectPreset::Custom) {
                if settings.video.custom_shader_path.trim().is_empty() {
                    effect_shader_source(EffectPreset::None)
                } else {
                    match std::fs::read_to_string(&settings.video.custom_shader_path) {
                        Ok(fragment) => {
                            let combined = format!(
                                "{}\n{}",
                                include_str!("../shaders/common_vertex.wgsl"),
                                fragment
                            );
                            if self.shader.current_scaling != scaling
                                || self.shader.current_effect != effect
                                || custom_path_changed
                            {
                                self.shader.pipeline = create_pipeline(
                                    device,
                                    &self.shader.bgl,
                                    self.shader.format,
                                    upscaler_source,
                                );
                                self.shader.effect_pipeline = Some(create_pipeline(
                                    device,
                                    &self.shader.bgl,
                                    self.shader.format,
                                    &combined,
                                ));
                            }
                            self.shader.two_pass = true;
                            self.shader.current_scaling = scaling;
                            self.shader.current_effect = effect;
                            self.shader.current_custom_shader_path = settings.video.custom_shader_path.clone();
                            if filter_changed {
                                self.apply_filter_change(device, desired_filter);
                            }
                            return;
                        }
                        Err(err) => {
                            log::warn!(
                                "Failed to load custom shader '{}': {}",
                                settings.video.custom_shader_path,
                                err
                            );
                            effect_shader_source(EffectPreset::None)
                        }
                    }
                }
            } else {
                effect_shader_source(effect)
            };

            if self.shader.current_scaling != scaling
                || self.shader.current_effect != effect
                || (matches!(effect, EffectPreset::Custom) && custom_path_changed)
            {
                self.shader.pipeline =
                    create_pipeline(device, &self.shader.bgl, self.shader.format, upscaler_source);
                self.shader.effect_pipeline = Some(create_pipeline(
                    device,
                    &self.shader.bgl,
                    self.shader.format,
                    effect_source,
                ));
            }
            self.shader.two_pass = true;
        } else {
            // Single pass
            let dynamic_source: String;
            let source = if matches!(effect, EffectPreset::Custom) && !scaling.is_upscaler() {
                if settings.video.custom_shader_path.trim().is_empty() {
                    combined_shader_source(scaling, EffectPreset::None)
                } else {
                    match std::fs::read_to_string(&settings.video.custom_shader_path) {
                        Ok(fragment) => {
                            dynamic_source = format!(
                                "{}\n{}",
                                include_str!("../shaders/common_vertex.wgsl"),
                                fragment
                            );
                            &dynamic_source
                        }
                        Err(err) => {
                            log::warn!(
                                "Failed to load custom shader '{}': {}",
                                settings.video.custom_shader_path,
                                err
                            );
                            combined_shader_source(scaling, EffectPreset::None)
                        }
                    }
                }
            } else {
                combined_shader_source(scaling, effect)
            };

            if self.shader.current_scaling != scaling
                || self.shader.current_effect != effect
                || (matches!(effect, EffectPreset::Custom) && custom_path_changed)
            {
                self.shader.pipeline =
                    create_pipeline(device, &self.shader.bgl, self.shader.format, source);
            }
            self.shader.effect_pipeline = None;
            self.shader.two_pass = false;
        }

        if filter_changed {
            self.apply_filter_change(device, desired_filter);
        }

        self.shader.current_scaling = scaling;
        self.shader.current_effect = effect;
        self.shader.current_custom_shader_path = settings.video.custom_shader_path.clone();
    }

    fn apply_filter_change(&mut self, device: &wgpu::Device, desired_filter: wgpu::FilterMode) {
        let sampler = match desired_filter {
            wgpu::FilterMode::Linear => &self.sampler.linear_sampler,
            wgpu::FilterMode::Nearest => &self.sampler.nearest_sampler,
        };
        self.screen.bind_group = create_texture_bind_group(
            device,
            &self.shader.bgl,
            &self.screen.view,
            sampler,
            &self.sampler.params_buffer,
            "screen bind group",
        );
        self.screen.bind_group_no_cc = create_texture_bind_group(
            device,
            &self.shader.bgl,
            &self.screen.view,
            sampler,
            &self.sampler.params_buffer_no_cc,
            "screen bind group no color correction",
        );
        self.sampler.current_filter = desired_filter;
    }

    pub(crate) fn update_params(&self, queue: &wgpu::Queue, settings: &crate::settings::Settings) {
        let nw = self.screen.native_width as f32;
        let nh = self.screen.native_height as f32;
        let buf = crate::settings::build_gpu_params(
            &settings.video.shader_params,
            settings.video.color_correction,
            settings.video.color_correction_matrix,
            nw,
            nh,
        );
        let buf_no_cc = crate::settings::build_gpu_params(
            &settings.video.shader_params,
            crate::settings::ColorCorrection::None,
            settings.video.color_correction_matrix,
            nw,
            nh,
        );
        queue.write_buffer(&self.sampler.params_buffer, 0, &buf);
        queue.write_buffer(&self.sampler.params_buffer_no_cc, 0, &buf_no_cc);
    }

    pub(crate) fn native_size(&self) -> (u32, u32) {
        (self.screen.native_width, self.screen.native_height)
    }

    pub(crate) fn set_native_size(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self.screen.native_width == width && self.screen.native_height == height {
            return;
        }
        self.screen.native_width = width;
        self.screen.native_height = height;

        self.screen.texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("screen texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.screen.view = self.screen.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = match self.sampler.current_filter {
            wgpu::FilterMode::Linear => &self.sampler.linear_sampler,
            wgpu::FilterMode::Nearest => &self.sampler.nearest_sampler,
        };
        self.screen.bind_group = create_texture_bind_group(
            device,
            &self.shader.bgl,
            &self.screen.view,
            sampler,
            &self.sampler.params_buffer,
            "screen bind group",
        );
        self.screen.bind_group_no_cc = create_texture_bind_group(
            device,
            &self.shader.bgl,
            &self.screen.view,
            sampler,
            &self.sampler.params_buffer_no_cc,
            "screen bind group no color correction",
        );
    }

    pub(crate) fn upload_framebuffer(&self, queue: &wgpu::Queue, framebuffer: &[u8]) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.screen.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            framebuffer,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * self.screen.native_width),
                rows_per_image: Some(self.screen.native_height),
            },
            wgpu::Extent3d {
                width: self.screen.native_width,
                height: self.screen.native_height,
                depth_or_array_layers: 1,
            },
        );
    }

    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, x: f32, y: f32, w: f32, h: f32) {
        pass.set_viewport(x, y, w, h, 0.0, 1.0);
        if self.shader.two_pass {
            pass.set_pipeline(self.shader.effect_pipeline.as_ref().unwrap());
            pass.set_bind_group(0, &self.offscreen.output_bind_group, &[]);
        } else {
            pass.set_pipeline(&self.shader.pipeline);
            pass.set_bind_group(0, &self.screen.bind_group, &[]);
        }
        pass.draw(0..3, 0..1);
    }

    pub(crate) fn resize_offscreen(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> bool {
        let w = width.max(MIN_OFFSCREEN_WIDTH);
        let h = height.max(MIN_OFFSCREEN_HEIGHT);
        if self.offscreen.width == w && self.offscreen.height == h {
            return false;
        }
        self.offscreen.width = w;
        self.offscreen.height = h;

        self.offscreen.output_texture =
            create_offscreen_texture(device, self.shader.format, w, h, "shader output texture");
        self.offscreen.output_view = self
            .offscreen
            .output_texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.offscreen.intermediate_texture =
            create_offscreen_texture(device, self.shader.format, w, h, "shader intermediate texture");
        self.offscreen.intermediate_view = self
            .offscreen
            .intermediate_texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.rebuild_aux_bind_groups(device);
        true
    }

    fn rebuild_aux_bind_groups(&mut self, device: &wgpu::Device) {
        self.offscreen.intermediate_bind_group = create_texture_bind_group(
            device,
            &self.shader.bgl,
            &self.offscreen.intermediate_view,
            &self.sampler.nearest_sampler,
            &self.sampler.params_buffer,
            "intermediate bind group",
        );
        self.offscreen.output_bind_group = create_texture_bind_group(
            device,
            &self.shader.bgl,
            &self.offscreen.output_view,
            &self.sampler.nearest_sampler,
            &self.sampler.params_buffer,
            "output bind group",
        );
    }

    pub(crate) fn render_to_offscreen(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.shader.two_pass {
            // Pass 1: Upscaler:screen_texture → intermediate_texture
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("shader upscaler pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.offscreen.intermediate_view,
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
                    multiview_mask: None,
                });
                pass.set_viewport(
                    0.0,
                    0.0,
                    self.offscreen.width as f32,
                    self.offscreen.height as f32,
                    0.0,
                    1.0,
                );
                pass.set_pipeline(&self.shader.pipeline);
                pass.set_bind_group(0, &self.screen.bind_group_no_cc, &[]);
                pass.draw(0..3, 0..1);
            }
            // Pass 2: Effect:intermediate_texture → output_texture
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("shader effect pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.offscreen.output_view,
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
                    multiview_mask: None,
                });
                pass.set_viewport(
                    0.0,
                    0.0,
                    self.offscreen.width as f32,
                    self.offscreen.height as f32,
                    0.0,
                    1.0,
                );
                pass.set_pipeline(self.shader.effect_pipeline.as_ref().unwrap());
                pass.set_bind_group(0, &self.offscreen.intermediate_bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
        } else {
            // Single pass
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shader offscreen pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.offscreen.output_view,
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
                multiview_mask: None,
            });
            pass.set_viewport(
                0.0,
                0.0,
                self.offscreen.width as f32,
                self.offscreen.height as f32,
                0.0,
                1.0,
            );
            pass.set_pipeline(&self.shader.pipeline);
            pass.set_bind_group(0, &self.screen.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    pub(crate) fn render_upscale_pass(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shader upscale direct pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.offscreen.output_view,
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
            multiview_mask: None,
        });
        pass.set_viewport(
            0.0,
            0.0,
            self.offscreen.width as f32,
            self.offscreen.height as f32,
            0.0,
            1.0,
        );
        pass.set_pipeline(&self.shader.pipeline);
        pass.set_bind_group(0, &self.screen.bind_group_no_cc, &[]);
        pass.draw(0..3, 0..1);
    }

    pub(crate) fn needs_two_pass(&self) -> bool {
        self.shader.two_pass
    }

    pub(crate) fn output_view(&self) -> &wgpu::TextureView {
        &self.offscreen.output_view
    }
}
