use egui::ColorImage;
use std::io::BufWriter;
use std::path::Path;

pub(crate) fn encode_color_image_as_png_bytes(image: &ColorImage) -> anyhow::Result<Vec<u8>> {
    let [w, h] = image.size;
    let mut buf = Vec::new();
    {
        let writer = BufWriter::new(&mut buf);
        let mut encoder = png::Encoder::new(writer, w as u32, h as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut png_writer = encoder.write_header()?;
        let rgba_bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
        png_writer.write_image_data(&rgba_bytes)?;
    }
    Ok(buf)
}

pub(crate) fn export_color_image_as_png(path: &Path, image: &ColorImage) -> anyhow::Result<()> {
    let bytes = encode_color_image_as_png_bytes(image)?;
    std::fs::write(path, &bytes)?;
    Ok(())
}

pub(crate) fn export_png_button(ui: &mut egui::Ui, default_name: &str, image: &ColorImage) -> bool {
    if ui.button("Export PNG").clicked() {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = crate::platform::FileDialog::new()
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

        #[cfg(target_arch = "wasm32")]
        match encode_color_image_as_png_bytes(image) {
            Ok(bytes) => {
                crate::platform::download_file(default_name, &bytes);
                log::info!("Triggered PNG download: {default_name}");
                return true;
            }
            Err(err) => {
                log::error!("Failed to encode PNG: {}", err);
            }
        }
    }
    false
}
