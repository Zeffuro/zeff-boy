@group(0) @binding(0) var t_screen: texture_2d<f32>;
@group(0) @binding(1) var s_screen: sampler;

struct ShaderParams {
    scanline_intensity: f32,
    curvature: f32,
    grid_intensity: f32,
    upscale_edge_strength: f32,
    palette_mix: f32,
    palette_warmth: f32,
    tex_width: f32,
    tex_height: f32,
    color_mode: u32,
    _pad0: u32,
    color_matrix_r: vec4<f32>,
    color_matrix_g: vec4<f32>,
    color_matrix_b: vec4<f32>,
};
@group(0) @binding(2) var<uniform> params: ShaderParams;

fn apply_color_correction(c: vec4<f32>) -> vec4<f32> {
    if (params.color_mode == 0u) {
        return c;
    }
    let r = dot(params.color_matrix_r.xyz, c.rgb);
    let g = dot(params.color_matrix_g.xyz, c.rgb);
    let b = dot(params.color_matrix_b.xyz, c.rgb);
    return vec4<f32>(clamp(vec3<f32>(r, g, b), vec3<f32>(0.0), vec3<f32>(1.0)), c.a);
}

struct VSOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VSOut {
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

