fn periodic_border_integral(position: f32, borderSize: f32) -> f32 {
    let shifted = position + borderSize;
    return floor(shifted) * (2.0 * borderSize)
        + min(fract(shifted), 2.0 * borderSize);
}

fn edge_coverage(pixelPosition: f32, footprint: f32) -> f32 {
    let borderSize = 0.12;
    let halfFootprint = footprint * 0.5;
    let covered = periodic_border_integral(pixelPosition + halfFootprint, borderSize)
        - periodic_border_integral(pixelPosition - halfFootprint, borderSize);
    return clamp(covered / footprint, 0.0, 1.0);
}

@fragment
fn fs_main(v: VSOut) -> @location(0) vec4<f32> {
    let texSize = vec2<f32>(params.tex_width, params.tex_height);
    let color = textureSample(t_screen, s_screen, v.uv);
    let pixelX = v.uv.x * texSize.x;
    let pixelY = v.uv.y * texSize.y;
    let footprintX = max(fwidth(pixelX), 0.0001);
    let footprintY = max(fwidth(pixelY), 0.0001);

    // A grid cannot be represented at 2x without dimming every output pixel.
    // Fade it in between 2x and 3x, then preserve its source-pixel coverage.
    let resolvableX = smoothstep(2.0, 3.0, 1.0 / footprintX);
    let resolvableY = smoothstep(2.0, 3.0, 1.0 / footprintY);
    let gridX = edge_coverage(pixelX, footprintX) * resolvableX;
    let gridY = edge_coverage(pixelY, footprintY) * resolvableY;
    let gridCoverage = 1.0 - (1.0 - gridX) * (1.0 - gridY);
    let gridFade = 1.0 - params.grid_intensity * gridCoverage;
    return apply_color_correction(vec4<f32>(color.rgb * gridFade, 1.0));
}
