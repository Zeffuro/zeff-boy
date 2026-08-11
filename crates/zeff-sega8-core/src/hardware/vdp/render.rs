use super::*;

mod mode4;
mod tms9918;

pub(super) fn render_mode4_background_rgba(
    vdp: &Vdp,
    framebuffer: &mut [u8],
    area: Mode4RenderArea,
) {
    mode4::render_background_rgba(vdp, framebuffer, area);
}

pub(super) fn render_mode4_frame_rgba(
    vdp: &Vdp,
    framebuffer: &mut [u8],
    area: Mode4RenderArea,
    color_mode: Mode4ColorMode,
) {
    mode4::render_frame_rgba(vdp, framebuffer, area, color_mode);
}

pub(super) fn render_tms9918_frame_rgba(
    vdp: &Vdp,
    framebuffer: &mut [u8],
    width: usize,
    height: usize,
) {
    tms9918::render_frame_rgba(vdp, framebuffer, width, height);
}
