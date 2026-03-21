struct ScreenUniform {
    screen_size: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> u_screen: ScreenUniform;

@group(1) @binding(0)
var t_tex: texture_2d<f32>;

@group(1) @binding(1)
var t_sampler: sampler;

struct VSIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VSOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(v: VSIn) -> VSOut {
    var o: VSOut;

    let w = max(u_screen.screen_size.x, 1.0);
    let h = max(u_screen.screen_size.y, 1.0);

    let clip_x = (v.pos.x / w) * 2.0 - 1.0;
    let clip_y = 1.0 - (v.pos.y / h) * 2.0;

    o.pos = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    o.uv = v.uv;
    o.color = v.color;
    return o;
}

@fragment
fn fs_main(v: VSOut) -> @location(0) vec4<f32> {
    return textureSample(t_tex, t_sampler, v.uv) * v.color;
}

