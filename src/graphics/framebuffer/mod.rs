mod render;
mod shader;

use anyhow::Result;

fn create_screen_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
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
    })
}

use super::pipeline::{
    combined_shader_source, create_offscreen_texture, create_pipeline, create_texture_bind_group,
    preferred_filter,
};
use crate::settings::{EffectPreset, ScalingMode};

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

impl FramebufferRenderer {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Result<Self> {
        let screen_texture =
            create_screen_texture(device, MIN_OFFSCREEN_WIDTH, MIN_OFFSCREEN_HEIGHT);

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
                native_width: MIN_OFFSCREEN_WIDTH,
                native_height: MIN_OFFSCREEN_HEIGHT,
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

    pub(crate) fn native_size(&self) -> (u32, u32) {
        (self.screen.native_width, self.screen.native_height)
    }

    pub(crate) fn set_native_size(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self.screen.native_width == width && self.screen.native_height == height {
            return;
        }
        self.screen.native_width = width;
        self.screen.native_height = height;

        self.screen.texture = create_screen_texture(device, width, height);
        self.screen.view = self
            .screen
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.rebuild_screen_bind_groups(device);
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

        self.offscreen.intermediate_texture = create_offscreen_texture(
            device,
            self.shader.format,
            w,
            h,
            "shader intermediate texture",
        );
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
}
