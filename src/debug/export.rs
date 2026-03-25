use egui::ColorImage;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

pub(crate) fn export_color_image_as_png(path: &Path, image: &ColorImage) -> anyhow::Result<()> {
    let [w, h] = image.size;
    let file = File::create(path)?;
    let buf = BufWriter::new(file);

    let mut encoder = png::Encoder::new(buf, w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);

    let mut writer = encoder.write_header()?;

    let rgba_bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();

    writer.write_image_data(&rgba_bytes)?;
    Ok(())
}

pub(crate) fn export_png_button(ui: &mut egui::Ui, default_name: &str, image: &ColorImage) -> bool {
    if ui.button("Export PNG").clicked()
        && let Some(path) = rfd::FileDialog::new()
            .set_title("Export as PNG")
            .add_filter("PNG Image", &["png"])
            .set_file_name(default_name)
            .save_file()
        {
            match export_color_image_as_png(&path, image) {
                Ok(()) => {
                    log::info!("Exported PNG to {}", path.display());
                    return true;
                }
                Err(err) => {
                    log::error!("Failed to export PNG: {}", err);
                }
            }
        }
    false
}
