@group(0) @binding(0) var t_screen: texture_2d<f32>;
@group(0) @binding(1) var s_screen: sampler;

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

@fragment
fn fs_main(v: VSOut) -> @location(0) vec4<f32> {
    let texSize = vec2<f32>(160.0, 144.0);
    let color = textureSample(t_screen, s_screen, v.uv);

    // Barrel distortion
    let center = v.uv - 0.5;
    let dist = dot(center, center);
    let barrel = center * (1.0 + 0.3 * dist);
    let warped_uv = barrel + 0.5;

    // Clamp to bounds
    if warped_uv.x < 0.0 || warped_uv.x > 1.0 || warped_uv.y < 0.0 || warped_uv.y > 1.0 {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    let warped_color = textureSample(t_screen, s_screen, warped_uv);

    // Scanlines
    let pixelY = warped_uv.y * texSize.y;
    let scanline = 0.82 + 0.18 * sin(pixelY * 3.14159265);

    // Vignette
    let edge = warped_uv * (1.0 - warped_uv);
    let vignette = clamp(edge.x * edge.y * 15.0, 0.0, 1.0);

    // Slight green phosphor tint
    var tinted = warped_color.rgb;
    tinted.g = tinted.g * 1.05;

    return vec4<f32>(tinted * scanline * vignette, 1.0);
}

