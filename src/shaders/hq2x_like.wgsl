@fragment
fn fs_main(v: VSOut) -> @location(0) vec4<f32> {
    let texel = vec2<f32>(1.0 / params.tex_width, 1.0 / params.tex_height);

    let c = textureSample(t_screen, s_screen, v.uv).rgb;
    let n = textureSample(t_screen, s_screen, v.uv + vec2<f32>(0.0, -texel.y)).rgb;
    let s = textureSample(t_screen, s_screen, v.uv + vec2<f32>(0.0, texel.y)).rgb;
    let e = textureSample(t_screen, s_screen, v.uv + vec2<f32>(texel.x, 0.0)).rgb;
    let w = textureSample(t_screen, s_screen, v.uv + vec2<f32>(-texel.x, 0.0)).rgb;

    let edge = (length(c - n) + length(c - s) + length(c - e) + length(c - w)) * 0.25;
    let edge_factor = clamp(edge * params.upscale_edge_strength, 0.0, 1.0);
    let neighbor_avg = (n + s + e + w) * 0.25;

    let smoothed = mix(c, neighbor_avg, edge_factor * 0.35);
    return vec4<f32>(smoothed, 1.0);
}

