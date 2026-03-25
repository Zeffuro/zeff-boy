@fragment
fn fs_main(v: VSOut) -> @location(0) vec4<f32> {
    return apply_color_correction(textureSample(t_screen, s_screen, v.uv));
}