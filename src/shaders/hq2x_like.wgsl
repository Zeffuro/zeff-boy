// HQ2x-like: sub-pixel quadrant edge-directed interpolation
// Uses YUV color distance for edge detection and per-quadrant directional blending.

fn yuv_dist(a: vec3<f32>, b: vec3<f32>) -> f32 {
    let d = a - b;
    let y = d.r * 0.299 + d.g * 0.587 + d.b * 0.114;
    let u = d.b * 0.493 - d.r * 0.147 - d.g * 0.289;
    let v_val = d.r * 0.615 - d.g * 0.515 - d.b * 0.100;
    return y * y + u * u * 0.25 + v_val * v_val * 0.25;
}

fn is_similar(a: vec3<f32>, b: vec3<f32>, threshold: f32) -> bool {
    return yuv_dist(a, b) < threshold;
}

@fragment
fn fs_main(v: VSOut) -> @location(0) vec4<f32> {
    let texel = vec2<f32>(1.0 / params.tex_width, 1.0 / params.tex_height);
    let tc = v.uv * vec2<f32>(params.tex_width, params.tex_height);
    let fp = fract(tc);
    let base = (floor(tc) + 0.5) * texel;

    let c  = textureSample(t_screen, s_screen, base).rgb;
    let n  = textureSample(t_screen, s_screen, base + vec2<f32>(0.0, -texel.y)).rgb;
    let s  = textureSample(t_screen, s_screen, base + vec2<f32>(0.0, texel.y)).rgb;
    let w  = textureSample(t_screen, s_screen, base + vec2<f32>(-texel.x, 0.0)).rgb;
    let e  = textureSample(t_screen, s_screen, base + vec2<f32>(texel.x, 0.0)).rgb;
    let nw = textureSample(t_screen, s_screen, base + vec2<f32>(-texel.x, -texel.y)).rgb;
    let ne = textureSample(t_screen, s_screen, base + vec2<f32>(texel.x, -texel.y)).rgb;
    let sw = textureSample(t_screen, s_screen, base + vec2<f32>(-texel.x, texel.y)).rgb;
    let se = textureSample(t_screen, s_screen, base + vec2<f32>(texel.x, texel.y)).rgb;

    let strength = clamp(params.upscale_edge_strength, 0.1, 2.0);
    let threshold = 0.035 / (strength * strength);

    var pattern: u32 = 0u;
    if is_similar(c, nw, threshold) { pattern |= 1u; }
    if is_similar(c, n,  threshold) { pattern |= 2u; }
    if is_similar(c, ne, threshold) { pattern |= 4u; }
    if is_similar(c, w,  threshold) { pattern |= 8u; }
    if is_similar(c, e,  threshold) { pattern |= 16u; }
    if is_similar(c, sw, threshold) { pattern |= 32u; }
    if is_similar(c, s,  threshold) { pattern |= 64u; }
    if is_similar(c, se, threshold) { pattern |= 128u; }

    var color = c;

    if fp.x < 0.5 && fp.y < 0.5 {
        let has_n = (pattern & 2u) != 0u;
        let has_w = (pattern & 8u) != 0u;

        if !has_n && !has_w {
            let t = (1.0 - fp.x - fp.y) * 0.5;
            color = mix(c, nw, clamp(t, 0.0, 0.4));
        } else if !has_n {
            let t = (0.5 - fp.y);
            color = mix(c, n, clamp(t, 0.0, 0.4));
        } else if !has_w {
            let t = (0.5 - fp.x);
            color = mix(c, w, clamp(t, 0.0, 0.4));
        }
    } else if fp.x >= 0.5 && fp.y < 0.5 {
        let has_n = (pattern & 2u) != 0u;
        let has_e = (pattern & 16u) != 0u;

        if !has_n && !has_e {
            let t = (fp.x - 0.5 + 0.5 - fp.y) * 0.5;
            color = mix(c, ne, clamp(t, 0.0, 0.4));
        } else if !has_n {
            let t = (0.5 - fp.y);
            color = mix(c, n, clamp(t, 0.0, 0.4));
        } else if !has_e {
            let t = (fp.x - 0.5);
            color = mix(c, e, clamp(t, 0.0, 0.4));
        }
    } else if fp.x < 0.5 && fp.y >= 0.5 {
        let has_s = (pattern & 64u) != 0u;
        let has_w = (pattern & 8u) != 0u;

        if !has_s && !has_w {
            let t = (0.5 - fp.x + fp.y - 0.5) * 0.5;
            color = mix(c, sw, clamp(t, 0.0, 0.4));
        } else if !has_s {
            let t = (fp.y - 0.5);
            color = mix(c, s, clamp(t, 0.0, 0.4));
        } else if !has_w {
            let t = (0.5 - fp.x);
            color = mix(c, w, clamp(t, 0.0, 0.4));
        }
    } else {
        let has_s = (pattern & 64u) != 0u;
        let has_e = (pattern & 16u) != 0u;

        if !has_s && !has_e {
            let t = (fp.x - 0.5 + fp.y - 0.5) * 0.5;
            color = mix(c, se, clamp(t, 0.0, 0.4));
        } else if !has_s {
            let t = (fp.y - 0.5);
            color = mix(c, s, clamp(t, 0.0, 0.4));
        } else if !has_e {
            let t = (fp.x - 0.5);
            color = mix(c, e, clamp(t, 0.0, 0.4));
        }
    }

    return apply_color_correction(vec4<f32>(color, 1.0));
}
