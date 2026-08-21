use egui::{ColorImage, TextureHandle, TextureOptions};

use crate::emu_thread::GameBoyPrinterImage;

const MAX_ROLL_PRINTOUTS: usize = 256;

struct PrinterOutput {
    image: ColorImage,
    textures: std::collections::HashMap<usize, TextureHandle>,
}

#[derive(Default)]
pub(crate) struct PrinterViewerState {
    outputs: Vec<PrinterOutput>,
    scroll_to_end: bool,
    discarded_printouts: usize,
}

impl PrinterViewerState {
    pub(crate) fn append(&mut self, images: Vec<GameBoyPrinterImage>) -> usize {
        let before = self.outputs.len();
        self.outputs.extend(images.into_iter().filter_map(|image| {
            let expected_len = image.width.checked_mul(image.height)?.checked_mul(4)?;
            if image.rgba.len() != expected_len {
                return None;
            }
            Some(PrinterOutput {
                image: ColorImage::from_rgba_unmultiplied([image.width, image.height], &image.rgba),
                textures: std::collections::HashMap::new(),
            })
        }));
        let appended = self.outputs.len() - before;
        if self.outputs.len() > MAX_ROLL_PRINTOUTS {
            let excess = self.outputs.len() - MAX_ROLL_PRINTOUTS;
            self.outputs.drain(..excess);
            self.discarded_printouts = self.discarded_printouts.saturating_add(excess);
        }
        self.scroll_to_end |= appended != 0;
        appended
    }

    pub(crate) fn len(&self) -> usize {
        self.outputs.len()
    }

    pub(crate) fn clear_textures(&mut self) {
        for output in &mut self.outputs {
            output.textures.clear();
        }
    }

    fn clear(&mut self) {
        self.outputs.clear();
        self.scroll_to_end = false;
        self.discarded_printouts = 0;
    }

    fn roll_image(&self) -> Option<ColorImage> {
        let width = self.outputs.first()?.image.size[0];
        if self
            .outputs
            .iter()
            .any(|output| output.image.size[0] != width)
        {
            return None;
        }
        let height = self.outputs.iter().try_fold(0usize, |height, output| {
            height.checked_add(output.image.size[1])
        })?;
        let pixel_count = width.checked_mul(height)?;
        let mut pixels = Vec::with_capacity(pixel_count);
        for output in &self.outputs {
            pixels.extend_from_slice(&output.image.pixels);
        }
        Some(ColorImage::new([width, height], pixels))
    }
}

pub(crate) fn draw_printer_viewer_content(ui: &mut egui::Ui, state: &mut PrinterViewerState) {
    let has_output = !state.outputs.is_empty();
    let mut save_latest = false;
    let mut save_roll = false;
    #[cfg(not(target_arch = "wasm32"))]
    let mut copy_roll = false;
    let mut clear = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(match state.len() {
            1 => "1 printout".to_string(),
            count => format!("{count} printouts"),
        });
        if state.discarded_printouts != 0 {
            ui.label(format!("{} older cleared", state.discarded_printouts));
        }
        ui.separator();
        save_latest = ui
            .add_enabled(has_output, egui::Button::new("Save Latest"))
            .clicked();
        save_roll = ui
            .add_enabled(has_output, egui::Button::new("Save Roll"))
            .clicked();
        #[cfg(not(target_arch = "wasm32"))]
        {
            copy_roll = ui
                .add_enabled(has_output, egui::Button::new("Copy Roll"))
                .clicked();
        }
        clear = ui
            .add_enabled(has_output, egui::Button::new("Clear"))
            .clicked();
    });
    if save_latest && let Some(latest) = state.outputs.last() {
        crate::debug::export::export_color_image_as_png_interactive(
            "game-boy-printer-latest.png",
            &latest.image,
        );
    }
    if save_roll && let Some(roll) = state.roll_image() {
        crate::debug::export::export_color_image_as_png_interactive(
            "game-boy-printer-roll.png",
            &roll,
        );
    }
    #[cfg(not(target_arch = "wasm32"))]
    if copy_roll && let Some(roll) = state.roll_image() {
        ui.ctx().copy_image(roll);
    }
    if clear {
        state.clear();
    }
    ui.separator();

    if state.outputs.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label("No printouts yet");
        });
        return;
    }

    let available_width = ui.available_width().max(1.0);
    let image_width = state.outputs[0].image.size[0] as f32;
    let scale = (available_width / image_width).floor().clamp(1.0, 4.0);
    let scroll_to_end = std::mem::take(&mut state.scroll_to_end);
    let output_count = state.outputs.len();
    egui::ScrollArea::both()
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let texture_manager = ui.ctx().tex_manager();
            let texture_manager_key = std::sync::Arc::as_ptr(&texture_manager) as usize;
            for (index, output) in state.outputs.iter_mut().enumerate() {
                let texture = output
                    .textures
                    .entry(texture_manager_key)
                    .or_insert_with(|| {
                        ui.ctx().load_texture(
                            format!("gb_printer_{index}"),
                            output.image.clone(),
                            TextureOptions::NEAREST,
                        )
                    });
                let size = egui::vec2(
                    output.image.size[0] as f32 * scale,
                    output.image.size[1] as f32 * scale,
                );
                let response = ui.add(egui::Image::new((texture.id(), size)));
                if scroll_to_end && index + 1 == output_count {
                    response.scroll_to_me(Some(egui::Align::Max));
                }
            }
        });
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn draw_printer_window(
    ctx: &egui::Context,
    state: &mut PrinterViewerState,
    open: &mut bool,
    bounds: egui::Rect,
) {
    egui::Window::new("Game Boy Printer Output")
        .open(open)
        .default_size([420.0, 560.0])
        .min_size([280.0, 300.0])
        .constrain_to(bounds)
        .show(ctx, |ui| draw_printer_viewer_content(ui, state));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(value: u8) -> GameBoyPrinterImage {
        GameBoyPrinterImage {
            width: 2,
            height: 1,
            rgba: vec![value, value, value, 255, value, value, value, 255],
        }
    }

    #[test]
    fn roll_stacks_printouts_without_resampling() {
        let mut state = PrinterViewerState::default();
        assert_eq!(state.append(vec![image(1), image(2)]), 2);

        let roll = state.roll_image().unwrap();

        assert_eq!(roll.size, [2, 2]);
        assert_eq!(roll.pixels[0], egui::Color32::from_rgb(1, 1, 1));
        assert_eq!(roll.pixels[2], egui::Color32::from_rgb(2, 2, 2));
    }

    #[test]
    fn malformed_printouts_are_not_added() {
        let mut state = PrinterViewerState::default();
        let appended = state.append(vec![GameBoyPrinterImage {
            width: 2,
            height: 1,
            rgba: vec![0; 7],
        }]);

        assert_eq!(appended, 0);
        assert_eq!(state.len(), 0);
    }

    #[test]
    fn roll_discards_oldest_printouts_at_its_limit() {
        let mut state = PrinterViewerState::default();
        let images = (0..=MAX_ROLL_PRINTOUTS)
            .map(|index| image(index as u8))
            .collect();

        assert_eq!(state.append(images), MAX_ROLL_PRINTOUTS + 1);
        assert_eq!(state.len(), MAX_ROLL_PRINTOUTS);
        assert_eq!(state.discarded_printouts, 1);
        assert_eq!(state.outputs[0].image.pixels[0].r(), 1);
    }
}
