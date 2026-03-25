fn cd(a: vec3<f32>, b: vec3<f32>) -> f32 {
    let d = a - b;
    let y = d.r * 0.299 + d.g * 0.587 + d.b * 0.114;
    return abs(y) + length(d) * 0.5;
}

@fragment
fn fs_main(v: VSOut) -> @location(0) vec4<f32> {
    let texel = vec2<f32>(1.0 / params.tex_width, 1.0 / params.tex_height);
    let tc = v.uv * vec2<f32>(params.tex_width, params.tex_height);
    let fp = fract(tc);
    let base = (floor(tc) + 0.5) * texel;

    let e  = textureSample(t_screen, s_screen, base).rgb;
    let n  = textureSample(t_screen, s_screen, base + vec2<f32>(0.0, -texel.y)).rgb;
    let s  = textureSample(t_screen, s_screen, base + vec2<f32>(0.0, texel.y)).rgb;
    let w  = textureSample(t_screen, s_screen, base + vec2<f32>(-texel.x, 0.0)).rgb;
    let ea = textureSample(t_screen, s_screen, base + vec2<f32>(texel.x, 0.0)).rgb;
    let nw = textureSample(t_screen, s_screen, base + vec2<f32>(-texel.x, -texel.y)).rgb;
    let ne = textureSample(t_screen, s_screen, base + vec2<f32>(texel.x, -texel.y)).rgb;
    let sw = textureSample(t_screen, s_screen, base + vec2<f32>(-texel.x, texel.y)).rgb;
    let se = textureSample(t_screen, s_screen, base + vec2<f32>(texel.x, texel.y)).rgb;

    let str_val = clamp(params.upscale_edge_strength, 0.1, 2.0);

    let smooth_nwse = cd(nw, e) + cd(e, se);
    let smooth_nesw = cd(ne, e) + cd(e, sw);

    let cross_nwse = cd(n, ea) + cd(w, s);
    let cross_nesw = cd(n, w) + cd(ea, s);

    let metric_nwse = smooth_nwse + cross_nwse * 0.5;
    let metric_nesw = smooth_nesw + cross_nesw * 0.5;

    var color = e;

    let diff = abs(metric_nwse - metric_nesw);
    let total = metric_nwse + metric_nesw + 0.001;
    let edge_confidence = diff / total;

    if edge_confidence > (0.2 / str_val) {
        if metric_nwse < metric_nesw {
            let d_to_nw = 1.0 - fp.x - fp.y;
            if d_to_nw > 0.0 {
                let t = clamp(d_to_nw * str_val, 0.0, 0.5);
                color = mix(e, nw, t);
            } else {
                let t = clamp(-d_to_nw * str_val, 0.0, 0.5);
                color = mix(e, se, t);
            }
        } else {
            let d_to_ne = fp.x - fp.y;
            if d_to_ne > 0.0 {
                let t = clamp(d_to_ne * str_val, 0.0, 0.5);
                color = mix(e, ne, t);
            } else {
                let t = clamp(-d_to_ne * str_val, 0.0, 0.5);
                color = mix(e, sw, t);
            }
        }
    }

    return apply_color_correction(vec4<f32>(color, 1.0));
}

