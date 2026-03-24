@fragment
fn fs_main(v: VSOut) -> @location(0) vec4<f32> {
    let texSize = vec2<f32>(160.0, 144.0);
    let color = textureSample(t_screen, s_screen, v.uv);
    // Barrel distortion
    let center = v.uv - 0.5;
    let dist = dot(center, center);
    let barrel = center * (1.0 + params.curvature * dist);
    let warped_uv = barrel + 0.5;
    // Clamp to bounds
    if warped_uv.x < 0.0 || warped_uv.x > 1.0 || warped_uv.y < 0.0 || warped_uv.y > 1.0 {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let warped_color = textureSample(t_screen, s_screen, warped_uv);
    // Scanlines
    let pixelY = warped_uv.y * texSize.y;
    let scanline = (1.0 - params.scanline_intensity) + params.scanline_intensity * sin(pixelY * 3.14159265);
    // Vignette
    let edge = warped_uv * (1.0 - warped_uv);
    let vignette = clamp(edge.x * edge.y * 15.0, 0.0, 1.0);
    // Slight green phosphor tint
    var tinted = warped_color.rgb;
    tinted.g = tinted.g * 1.05;
    return vec4<f32>(tinted * scanline * vignette, 1.0);
}