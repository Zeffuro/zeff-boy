use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use crate::cli::types::HeadlessOptions;
pub(super) fn write_screenshot_if_requested(
    opts: &HeadlessOptions,
    frame: u64,
    framebuffer: &[u8],
    dimensions: (usize, usize),
    screenshot_written: &mut bool,
) -> anyhow::Result<()> {
    if *screenshot_written || opts.screenshot_frame != Some(frame) {
        return Ok(());
    }
    let Some(path) = &opts.screenshot_path else {
        return Ok(());
    };
    write_rgba_png(path, framebuffer, dimensions)?;
    *screenshot_written = true;
    println!("[headless] screenshot={} frame={}", path.display(), frame);
    Ok(())
}

pub(super) fn write_screenshot_sequence_if_requested(
    opts: &HeadlessOptions,
    frame: u64,
    framebuffer: &[u8],
    dimensions: (usize, usize),
) -> anyhow::Result<()> {
    if opts.screenshot_every == 0 || !frame.is_multiple_of(opts.screenshot_every) {
        return Ok(());
    }
    let Some(dir) = &opts.screenshot_dir else {
        return Ok(());
    };
    let path = screenshot_sequence_path(dir, frame);
    write_rgba_png(&path, framebuffer, dimensions)?;
    println!(
        "[headless] screenshot-sequence={} frame={}",
        path.display(),
        frame
    );
    Ok(())
}

pub(super) fn screenshot_sequence_path(dir: &Path, frame: u64) -> PathBuf {
    dir.join(format!("frame_{frame:06}.png"))
}

pub(super) fn write_final_screenshot_if_needed(
    opts: &HeadlessOptions,
    frame: u64,
    framebuffer: &[u8],
    dimensions: (usize, usize),
    screenshot_written: &mut bool,
) -> anyhow::Result<()> {
    if *screenshot_written {
        return Ok(());
    }
    let Some(path) = &opts.screenshot_path else {
        return Ok(());
    };
    if opts
        .screenshot_frame
        .is_some_and(|requested| requested <= frame)
    {
        return Ok(());
    }
    write_rgba_png(path, framebuffer, dimensions)?;
    *screenshot_written = true;
    println!("[headless] screenshot={} frame={}", path.display(), frame);
    Ok(())
}

fn write_rgba_png(
    path: &Path,
    framebuffer: &[u8],
    dimensions: (usize, usize),
) -> anyhow::Result<()> {
    let (width, height) = dimensions;
    let expected_len = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| anyhow::anyhow!("screenshot dimensions overflow"))?;
    if framebuffer.len() != expected_len {
        anyhow::bail!(
            "framebuffer length mismatch for screenshot: got {}, expected {} for {}x{} RGBA",
            framebuffer.len(),
            expected_len,
            width,
            height
        );
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let file = File::create(path)?;
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut png_writer = encoder.write_header()?;
    png_writer.write_image_data(framebuffer)?;
    Ok(())
}

pub(super) fn screenshot_path_if_written(
    opts: &HeadlessOptions,
    screenshot_written: bool,
) -> Option<&PathBuf> {
    screenshot_written.then_some(opts.screenshot_path.as_ref()?)
}
