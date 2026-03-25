fn color_dist(a: vec3<f32>, b: vec3<f32>) -> f32 {
    return length(a - b);
}

@fragment
fn fs_main(v: VSOut) -> @location(0) vec4<f32> {
    let texel = vec2<f32>(1.0 / params.tex_width, 1.0 / params.tex_height);
    let tc = v.uv * vec2<f32>(params.tex_width, params.tex_height);
    let fp = fract(tc);
    let base = (floor(tc) + 0.5) * texel;

    let a = textureSample(t_screen, s_screen, base + vec2<f32>(-texel.x, -texel.y)).rgb;
    let b = textureSample(t_screen, s_screen, base + vec2<f32>(0.0, -texel.y)).rgb;
    let c = textureSample(t_screen, s_screen, base + vec2<f32>(texel.x, -texel.y)).rgb;
    let d = textureSample(t_screen, s_screen, base + vec2<f32>(-texel.x, 0.0)).rgb;
    let e = textureSample(t_screen, s_screen, base).rgb;
    let f = textureSample(t_screen, s_screen, base + vec2<f32>(texel.x, 0.0)).rgb;
    let g = textureSample(t_screen, s_screen, base + vec2<f32>(-texel.x, texel.y)).rgb;
    let h = textureSample(t_screen, s_screen, base + vec2<f32>(0.0, texel.y)).rgb;
    let i = textureSample(t_screen, s_screen, base + vec2<f32>(texel.x, texel.y)).rgb;

    let thr = 0.25 * (1.1 - clamp(params.upscale_edge_strength, 0.0, 1.0));

    var color = e;

    if fp.x < 0.5 && fp.y < 0.5 {
        if color_dist(a, b) < thr && color_dist(a, d) < thr {
            color = a;
        }
    } else if fp.x >= 0.5 && fp.y < 0.5 {
        if color_dist(c, b) < thr && color_dist(c, f) < thr {
            color = c;
        }
    } else if fp.x < 0.5 && fp.y >= 0.5 {
        if color_dist(g, d) < thr && color_dist(g, h) < thr {
            color = g;
        }
    } else {
        if color_dist(i, f) < thr && color_dist(i, h) < thr {
            color = i;
        }
    }

    return apply_color_correction(vec4<f32>(color, 1.0));
}

