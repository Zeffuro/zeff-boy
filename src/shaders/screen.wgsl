@group(0) @binding(0) var t_screen: texture_2d<f32>;
@group(0) @binding(1) var s_screen: sampler;

struct VSOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VSOut {
    // Fullscreen triangle
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var uv = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(2.0, 1.0),
        vec2<f32>(0.0, -1.0),
    );
    var o: VSOut;
    o.pos = vec4<f32>(pos[idx], 0.0, 1.0);
    o.uv = uv[idx];
    return o;
}

@fragment
fn fs_main(v: VSOut) -> @location(0) vec4<f32> {
    return textureSample(t_screen, s_screen, v.uv);
}

