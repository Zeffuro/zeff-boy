use super::FramebufferRenderer;
use crate::settings::EffectPreset;

use super::{create_pipeline, create_texture_bind_group, preferred_filter};

use super::super::pipeline::{
    combined_shader_source, effect_shader_source, needs_two_pass, scaling_shader_source,
};

fn load_custom_shader_source(path: &str) -> Option<String> {
    if path.trim().is_empty() {
        return None;
    }
    match std::fs::read_to_string(path) {
        Ok(fragment) => Some(format!(
            "{}\n{}",
            include_str!("../../shaders/common_vertex.wgsl"),
            fragment
        )),
        Err(err) => {
            log::warn!("Failed to load custom shader '{}': {}", path, err);
            None
        }
    }
}

impl FramebufferRenderer {
    pub(crate) fn set_shader(
        &mut self,
        device: &wgpu::Device,
        settings: &crate::settings::Settings,
    ) {
        let scaling = settings.video.scaling_mode;
        let effect = settings.video.effect_preset;
        let custom_path_changed =
            self.shader.current_custom_shader_path != settings.video.custom_shader_path;
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
                if let Some(combined) = load_custom_shader_source(&settings.video.custom_shader_path) {
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
                    self.shader.current_custom_shader_path =
                        settings.video.custom_shader_path.clone();
                    if filter_changed {
                        self.apply_filter_change(device, desired_filter);
                    }
                    return;
                } else {
                    effect_shader_source(EffectPreset::None)
                }
            } else {
                effect_shader_source(effect)
            };

            if self.shader.current_scaling != scaling
                || self.shader.current_effect != effect
                || (matches!(effect, EffectPreset::Custom) && custom_path_changed)
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
                    effect_source,
                ));
            }
            self.shader.two_pass = true;
        } else {
            // Single pass
            let dynamic_source: String;
            let source = if matches!(effect, EffectPreset::Custom) && !scaling.is_upscaler() {
                if let Some(src) = load_custom_shader_source(&settings.video.custom_shader_path) {
                    dynamic_source = src;
                    &dynamic_source
                } else {
                    combined_shader_source(scaling, EffectPreset::None)
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
        self.sampler.current_filter = desired_filter;
        self.rebuild_screen_bind_groups(device);
    }

    pub(crate) fn rebuild_screen_bind_groups(&mut self, device: &wgpu::Device) {
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
}
