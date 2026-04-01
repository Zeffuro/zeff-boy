use crate::settings::{EffectPreset, ScalingMode};

pub(super) fn scaling_shader_source(mode: ScalingMode) -> &'static str {
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

pub(super) fn effect_shader_source(preset: EffectPreset) -> &'static str {
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

pub(super) fn combined_shader_source(scaling: ScalingMode, effect: EffectPreset) -> &'static str {
    if scaling.is_upscaler() {
        scaling_shader_source(scaling)
    } else {
        effect_shader_source(effect)
    }
}

pub(super) fn preferred_filter(scaling: ScalingMode) -> wgpu::FilterMode {
    match scaling {
        ScalingMode::Bilinear => wgpu::FilterMode::Linear,
        _ => wgpu::FilterMode::Nearest,
    }
}

pub(super) fn needs_two_pass(scaling: ScalingMode, effect: EffectPreset) -> bool {
    scaling.is_upscaler() && effect != EffectPreset::None
}

pub(super) fn create_pipeline(
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

pub(super) fn create_offscreen_texture(
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

pub(super) fn create_texture_bind_group(
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

