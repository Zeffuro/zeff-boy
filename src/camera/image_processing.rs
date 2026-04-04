use super::checkerboard_frame;

#[cfg_attr(not(feature = "camera"), allow(dead_code))]
pub(super) fn avg_luma(frame: &[u8]) -> u8 {
    if frame.is_empty() {
        return 0;
    }
    let sum: u64 = frame.iter().map(|&v| v as u64).sum();
    (sum / frame.len() as u64) as u8
}

#[cfg_attr(not(feature = "camera"), allow(dead_code))]
pub(super) fn rgb_to_grayscale_nearest(
    rgb: &[u8],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
) -> Vec<u8> {
    downsample_rgb_box(rgb, src_w, src_h, dst_w, dst_h)
}

#[cfg_attr(not(feature = "camera"), allow(dead_code))]
pub(super) fn rgba_to_grayscale_nearest(
    rgba: &[u8],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
) -> Vec<u8> {
    downsample_rgba_box(rgba, src_w, src_h, dst_w, dst_h)
}

fn downsample_rgb_box(
    rgb: &[u8],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
) -> Vec<u8> {
    if src_w == 0 || src_h == 0 {
        return checkerboard_frame();
    }

    let mut out = vec![0u8; dst_w * dst_h];
    for y in 0..dst_h {
        let y0 = y * src_h / dst_h;
        let y1 = ((y + 1) * src_h / dst_h).max(y0 + 1).min(src_h);
        for x in 0..dst_w {
            let x0 = x * src_w / dst_w;
            let x1 = ((x + 1) * src_w / dst_w).max(x0 + 1).min(src_w);
            let mut sum: u64 = 0;
            let mut count: u64 = 0;
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let idx = (sy * src_w + sx) * 3;
                    if idx + 2 >= rgb.len() {
                        continue;
                    }
                    let r = rgb[idx] as u64;
                    let g = rgb[idx + 1] as u64;
                    let b = rgb[idx + 2] as u64;
                    sum = sum.saturating_add((r * 77 + g * 150 + b * 29) >> 8);
                    count = count.saturating_add(1);
                }
            }
            out[y * dst_w + x] = if count == 0 { 0 } else { (sum / count) as u8 };
        }
    }
    out
}

#[cfg_attr(not(feature = "camera"), allow(dead_code))]
fn downsample_rgba_box(
    rgba: &[u8],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
) -> Vec<u8> {
    if src_w == 0 || src_h == 0 {
        return checkerboard_frame();
    }

    let mut out = vec![0u8; dst_w * dst_h];
    for y in 0..dst_h {
        let y0 = y * src_h / dst_h;
        let y1 = ((y + 1) * src_h / dst_h).max(y0 + 1).min(src_h);
        for x in 0..dst_w {
            let x0 = x * src_w / dst_w;
            let x1 = ((x + 1) * src_w / dst_w).max(x0 + 1).min(src_w);
            let mut sum: u64 = 0;
            let mut count: u64 = 0;
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let idx = (sy * src_w + sx) * 4;
                    if idx + 2 >= rgba.len() {
                        continue;
                    }
                    let r = rgba[idx] as u64;
                    let g = rgba[idx + 1] as u64;
                    let b = rgba[idx + 2] as u64;
                    sum = sum.saturating_add((r * 77 + g * 150 + b * 29) >> 8);
                    count = count.saturating_add(1);
                }
            }
            out[y * dst_w + x] = if count == 0 { 0 } else { (sum / count) as u8 };
        }
    }
    out
}

use super::CameraHostSettings;

#[cfg_attr(not(feature = "camera"), allow(dead_code))]
pub(super) fn apply_host_postprocess(frame: &mut [u8], settings: CameraHostSettings) {
    if frame.is_empty() {
        return;
    }

    if settings.auto_levels {
        auto_levels_in_place(frame);
    }

    apply_brightness_contrast_in_place(frame, settings.brightness, settings.contrast);

    if (settings.gamma - 1.0).abs() > f32::EPSILON {
        apply_gamma_in_place(frame, settings.gamma);
    }
}

#[cfg_attr(not(feature = "camera"), allow(dead_code))]
fn apply_brightness_contrast_in_place(frame: &mut [u8], brightness: f32, contrast: f32) {
    let brightness = brightness.clamp(-1.0, 1.0) * 255.0;
    let contrast = contrast.clamp(0.25, 3.0);
    if brightness.abs() < f32::EPSILON && (contrast - 1.0).abs() < f32::EPSILON {
        return;
    }

    for p in frame.iter_mut() {
        let v = *p as f32;
        let adjusted = (v - 128.0) * contrast + 128.0 + brightness;
        *p = adjusted.clamp(0.0, 255.0) as u8;
    }
}

#[cfg_attr(not(feature = "camera"), allow(dead_code))]
fn auto_levels_in_place(frame: &mut [u8]) {
    let mut hist = [0u32; 256];
    for &v in frame.iter() {
        hist[v as usize] = hist[v as usize].saturating_add(1);
    }

    let total = frame.len() as u32;
    if total < 8 {
        return;
    }
    let low_target = total / 50;
    let high_target = total - low_target;

    let mut acc = 0u32;
    let mut low = 0usize;
    while low < 255 {
        acc = acc.saturating_add(hist[low]);
        if acc >= low_target {
            break;
        }
        low += 1;
    }

    acc = 0;
    let mut high = 255usize;
    while high > 0 {
        acc = acc.saturating_add(hist[high]);
        if total.saturating_sub(acc) <= high_target {
            break;
        }
        high -= 1;
    }

    if high <= low + 4 {
        return;
    }

    let span = (high - low) as u32;
    for p in frame.iter_mut() {
        let v = (*p as i32 - low as i32).clamp(0, span as i32) as u32;
        *p = ((v * 255) / span) as u8;
    }
}

#[cfg_attr(not(feature = "camera"), allow(dead_code))]
fn apply_gamma_in_place(frame: &mut [u8], gamma: f32) {
    let mut lut = [0u8; 256];
    for (i, out) in lut.iter_mut().enumerate() {
        let norm = (i as f32) / 255.0;
        *out = (norm.powf(gamma).mul_add(255.0, 0.5).clamp(0.0, 255.0)) as u8;
    }
    for p in frame.iter_mut() {
        *p = lut[*p as usize];
    }
}

#[cfg(feature = "camera")]
pub(super) fn decode_compressed_to_grayscale_nearest(
    compressed: &[u8],
    dst_w: usize,
    dst_h: usize,
) -> Option<Vec<u8>> {
    if compressed.is_empty() {
        return None;
    }

    let decoded = image::load_from_memory_with_format(compressed, image::ImageFormat::Jpeg)
        .or_else(|_| image::load_from_memory(compressed))
        .ok()?;
    let rgb = decoded.to_rgb8();
    let (src_w, src_h) = rgb.dimensions();
    Some(rgb_to_grayscale_nearest(
        rgb.as_raw(),
        src_w as usize,
        src_h as usize,
        dst_w,
        dst_h,
    ))
}
