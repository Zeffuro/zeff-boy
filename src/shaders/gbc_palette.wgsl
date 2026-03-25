@fragment
fn fs_main(v: VSOut) -> @location(0) vec4<f32> {
    let color = textureSample(t_screen, s_screen, v.uv).rgb;

    let gbc = vec3<f32>(
        color.r * (26.0 / 32.0) + color.g * (4.0 / 32.0) + color.b * (2.0 / 32.0),
        color.g * (24.0 / 32.0) + color.b * (8.0 / 32.0),
        color.r * (6.0 / 32.0) + color.g * (4.0 / 32.0) + color.b * (22.0 / 32.0)
    );

    let mixed = mix(color, gbc, clamp(params.palette_mix, 0.0, 1.0));

    let warm = vec3<f32>(
        mixed.r + params.palette_warmth * 0.08,
        mixed.g,
        mixed.b - params.palette_warmth * 0.05
    );

    return apply_color_correction(vec4<f32>(clamp(warm, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0));
}

