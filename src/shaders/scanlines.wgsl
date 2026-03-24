@fragment
fn fs_main(v: VSOut) -> @location(0) vec4<f32> {
    let texSize = vec2<f32>(160.0, 144.0);
    let color = textureSample(t_screen, s_screen, v.uv);
    let pixelY = v.uv.y * texSize.y;
    let scanline = (1.0 - params.scanline_intensity) + params.scanline_intensity * sin(pixelY * 3.14159265);
    return vec4<f32>(color.rgb * scanline, 1.0);
}