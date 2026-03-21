use std::collections::HashMap;

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use egui::epaint::Primitive;
use egui::{ClippedPrimitive, TextureId};
use wgpu::util::DeviceExt;
use crate::graphics::gpu::texture_sampler_bind_group_layout;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ScreenUniform {
    screen_size: [f32; 4],
}

struct TextureBundle {
    texture: wgpu::Texture,
    _view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    size: [u32; 2],
}

pub(crate) struct EguiPainter {
    pipeline: wgpu::RenderPipeline,
    screen_buffer: wgpu::Buffer,
    screen_bind_group: wgpu::BindGroup,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    textures: HashMap<TextureId, TextureBundle>,
}

fn image_to_rgba8(image: &egui::ImageData) -> Vec<u8> {
    match image {
        egui::ImageData::Color(color) => {
            color
                .pixels
                .iter()
                .flat_map(|c| c.to_array())
                .collect()
        }
    }
}

impl EguiPainter {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Result<Self> {
        let screen_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("egui screen bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let texture_bind_group_layout = texture_sampler_bind_group_layout(device, "egui texture bgl");

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("egui shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(
                include_str!("../shaders/egui.wgsl"),
            )),
        });

        let uniform = ScreenUniform {
            screen_size: [1.0, 1.0, 0.0, 0.0],
        };

        let screen_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("egui screen buffer"),
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let screen_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("egui screen bind group"),
            layout: &screen_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("egui pipeline layout"),
            bind_group_layouts: &[
                Option::from(&screen_bind_group_layout),
                Option::from(&texture_bind_group_layout),
            ],
            immediate_size: 0,
        });

        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GpuVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    shader_location: 0,
                    offset: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    shader_location: 1,
                    offset: 8,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    shader_location: 2,
                    offset: 16,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("egui pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            cache: None,
            multiview_mask: None,
        });

        Ok(Self {
            pipeline,
            screen_buffer,
            screen_bind_group,
            texture_bind_group_layout,
            textures: HashMap::new(),
        })
    }

    pub(crate) fn update_screen_size(&self, queue: &wgpu::Queue, width: u32, height: u32) {
        let uniform = ScreenUniform {
            screen_size: [width as f32, height as f32, 0.0, 0.0],
        };
        queue.write_buffer(&self.screen_buffer, 0, bytemuck::bytes_of(&uniform));
    }

    pub(crate) fn update_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: egui::TextureId,
        delta: &egui::epaint::image::ImageDelta,
    ) -> Result<()> {
        let data_width = delta.image.width() as u32;
        let data_height = delta.image.height() as u32;
        let pixels = image_to_rgba8(&delta.image);

        let is_partial = delta.pos.is_some();

        if !is_partial {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("egui texture"),
                size: wgpu::Extent3d {
                    width: data_width.max(1),
                    height: data_height.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let mag_filter = match delta.options.magnification {
                egui::TextureFilter::Nearest => wgpu::FilterMode::Nearest,
                egui::TextureFilter::Linear => wgpu::FilterMode::Linear,
            };
            let min_filter = match delta.options.minification {
                egui::TextureFilter::Nearest => wgpu::FilterMode::Nearest,
                egui::TextureFilter::Linear => wgpu::FilterMode::Linear,
            };
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("egui sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter,
                min_filter,
                mipmap_filter: wgpu::MipmapFilterMode::Nearest,
                ..Default::default()
            });

            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("egui texture bind group"),
                layout: &self.texture_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });

            self.textures.insert(
                id,
                TextureBundle {
                    texture,
                    _view: view,
                    bind_group,
                    size: [data_width, data_height],
                },
            );
        }

        let tex = self
            .textures
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("partial update for unknown texture {:?}", id))?;

        let origin = if let Some(pos) = delta.pos {
            wgpu::Origin3d {
                x: pos[0] as u32,
                y: pos[1] as u32,
                z: 0,
            }
        } else {
            wgpu::Origin3d::ZERO
        };

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex.texture,
                mip_level: 0,
                origin,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * data_width),
                rows_per_image: Some(data_height),
            },
            wgpu::Extent3d {
                width: data_width,
                height: data_height,
                depth_or_array_layers: 1,
            },
        );

        Ok(())
    }

    pub(crate) fn free_texture(&mut self, id: &TextureId) {
        self.textures.remove(id);
    }

    pub(crate) fn paint(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        render_pass: &mut wgpu::RenderPass<'_>,
        screen_width: u32,
        screen_height: u32,
        pixels_per_point: f32,
        paint_jobs: &[ClippedPrimitive],
        textures_delta: &egui::TexturesDelta,
    ) -> Result<()> {
        self.update_screen_size(queue, screen_width, screen_height);

        for (id, delta) in &textures_delta.set {
            self.update_texture(device, queue, *id, delta)?;
        }

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.screen_bind_group, &[]);

        for clipped in paint_jobs {
            let clip_rect = clipped.clip_rect;

            let Primitive::Mesh(mesh) = &clipped.primitive else {
                continue;
            };

            let Some(tex) = self.textures.get(&mesh.texture_id) else {
                continue;
            };

            if mesh.vertices.is_empty() || mesh.indices.is_empty() {
                continue;
            }

            let vertices: Vec<GpuVertex> = mesh
                .vertices
                .iter()
                .map(|v| {
                    let [r, g, b, a] = v.color.to_array();
                    GpuVertex {
                        pos: [v.pos.x, v.pos.y],
                        uv: [v.uv.x, v.uv.y],
                        color: [
                            r as f32 / 255.0,
                            g as f32 / 255.0,
                            b as f32 / 255.0,
                            a as f32 / 255.0,
                        ],
                    }
                })
                .collect();

            let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("egui vertex buffer"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

            let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("egui index buffer"),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });

            let ppp = pixels_per_point;
            let size_in_pixels = [screen_width, screen_height];

            let x0 =
                (clip_rect.min.x * ppp).floor().clamp(0.0, size_in_pixels[0] as f32) as u32;
            let y0 =
                (clip_rect.min.y * ppp).floor().clamp(0.0, size_in_pixels[1] as f32) as u32;
            let x1 =
                (clip_rect.max.x * ppp).ceil().clamp(0.0, size_in_pixels[0] as f32) as u32;
            let y1 =
                (clip_rect.max.y * ppp).ceil().clamp(0.0, size_in_pixels[1] as f32) as u32;

            if x1 <= x0 || y1 <= y0 {
                continue;
            }

            render_pass.set_scissor_rect(x0, y0, x1 - x0, y1 - y0);
            render_pass.set_bind_group(1, &tex.bind_group, &[]);
            render_pass.set_vertex_buffer(0, vb.slice(..));
            render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
            render_pass.draw_indexed(0..mesh.indices.len() as u32, 0, 0..1);
        }

        for id in &textures_delta.free {
            self.free_texture(id);
        }

        Ok(())
    }
}