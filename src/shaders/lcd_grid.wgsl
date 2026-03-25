@fragment
fn fs_main(v: VSOut) -> @location(0) vec4<f32> {
    let texSize = vec2<f32>(params.tex_width, params.tex_height);
    let color = textureSample(t_screen, s_screen, v.uv);
    let pixelX = v.uv.x * texSize.x;
    let pixelY = v.uv.y * texSize.y;
    let gridX = fract(pixelX);
    let gridY = fract(pixelY);
    let borderSize = 0.12;
    var gridFade = 1.0;
    if gridX < borderSize || gridX > (1.0 - borderSize) {
        gridFade = 1.0 - params.grid_intensity;
    }
    if gridY < borderSize || gridY > (1.0 - borderSize) {
        gridFade = 1.0 - params.grid_intensity;
    }
    return apply_color_correction(vec4<f32>(color.rgb * gridFade, 1.0));
}