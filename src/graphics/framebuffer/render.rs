use super::FramebufferRenderer;

impl FramebufferRenderer {
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, x: f32, y: f32, w: f32, h: f32) {
        pass.set_viewport(x, y, w, h, 0.0, 1.0);
        if self.shader.two_pass {
            pass.set_pipeline(
                self.shader
                    .effect_pipeline
                    .as_ref()
                    .expect("effect_pipeline must exist in two-pass mode"),
            );
            pass.set_bind_group(0, &self.offscreen.output_bind_group, &[]);
        } else {
            pass.set_pipeline(&self.shader.pipeline);
            pass.set_bind_group(0, &self.screen.bind_group, &[]);
        }
        pass.draw(0..3, 0..1);
    }

    pub(crate) fn render_to_offscreen(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.shader.two_pass {
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
                pass.set_pipeline(
                    self.shader
                        .effect_pipeline
                        .as_ref()
                        .expect("effect_pipeline must exist in two-pass mode"),
                );
                pass.set_bind_group(0, &self.offscreen.intermediate_bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
        } else {
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
