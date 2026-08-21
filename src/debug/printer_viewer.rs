use egui::{Color32, ColorImage, TextureHandle, TextureOptions};
use zeff_gb_core::hardware::{
    GAME_BOY_PRINTER_FEED_HEIGHT, GAME_BOY_PRINTER_WIDTH, GameBoyPrinterJob,
};

const MAX_ROLL_JOBS: usize = 256;
const MAX_DISPLAY_SEGMENTS: usize = 2048;
const MAX_EXPORT_PIXELS: usize = 16 * 1024 * 1024;

struct PrinterOutput {
    job: GameBoyPrinterJob,
    body: Option<ColorImage>,
    textures: std::collections::HashMap<usize, TextureHandle>,
}

#[derive(Default)]
pub(crate) struct PrinterViewerState {
    outputs: Vec<PrinterOutput>,
    scroll_to_end: bool,
    discarded_jobs: usize,
}

impl PrinterViewerState {
    pub(crate) fn append(&mut self, jobs: Vec<GameBoyPrinterJob>) -> usize {
        let before = self.outputs.len();
        self.outputs.extend(jobs.into_iter().filter_map(|job| {
            let body = render_body(&job)?;
            (job_roll_height(&job)? != 0).then_some(PrinterOutput {
                job,
                body,
                textures: std::collections::HashMap::new(),
            })
        }));
        let appended = self.outputs.len() - before;
        if self.outputs.len() > MAX_ROLL_JOBS {
            let excess = self.outputs.len() - MAX_ROLL_JOBS;
            self.outputs.drain(..excess);
            self.discarded_jobs = self.discarded_jobs.saturating_add(excess);
        }
        let mut display_segments: usize = self.outputs.iter().map(output_segments).sum();
        while display_segments > MAX_DISPLAY_SEGMENTS && self.outputs.len() > 1 {
            display_segments -= output_segments(&self.outputs[0]);
            self.outputs.remove(0);
            self.discarded_jobs = self.discarded_jobs.saturating_add(1);
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
        self.discarded_jobs = 0;
    }

    fn latest_image(&self) -> Option<ColorImage> {
        self.outputs
            .iter()
            .rev()
            .find_map(|output| output.body.clone())
    }

    fn can_export_latest(&self) -> bool {
        self.outputs.iter().any(|output| output.body.is_some())
    }

    fn can_export_roll(&self) -> bool {
        let Some(height) = self.outputs.iter().try_fold(0usize, |height, output| {
            height.checked_add(job_roll_height(&output.job)?)
        }) else {
            return false;
        };
        GAME_BOY_PRINTER_WIDTH
            .checked_mul(height)
            .is_some_and(|pixels| (1..=MAX_EXPORT_PIXELS).contains(&pixels))
    }

    fn roll_image(&self) -> Option<ColorImage> {
        let height = self.outputs.iter().try_fold(0usize, |height, output| {
            height.checked_add(job_roll_height(&output.job)?)
        })?;
        let pixel_count = GAME_BOY_PRINTER_WIDTH.checked_mul(height)?;
        if pixel_count == 0 || pixel_count > MAX_EXPORT_PIXELS {
            return None;
        }
        let mut pixels = Vec::with_capacity(pixel_count);
        for output in &self.outputs {
            append_rendered_job(&output.job, &mut pixels);
        }
        Some(ColorImage::new([GAME_BOY_PRINTER_WIDTH, height], pixels))
    }
}

fn render_body(job: &GameBoyPrinterJob) -> Option<Option<ColorImage>> {
    job.validate().ok()?;
    if !job.has_image() {
        return Some(None);
    }
    let pixels = job.pixels.iter().map(|pixel| {
        let mapped = (job.palette >> (pixel * 2)) & 0x03;
        let base_gray = [255u32, 192, 64, 0][usize::from(mapped)];
        let darkness = 255 - base_gray;
        let adjusted_darkness = (darkness * (192 + u32::from(job.density)) / 256).min(255);
        Color32::from_gray((255 - adjusted_darkness) as u8)
    });
    Some(Some(ColorImage::new(
        [GAME_BOY_PRINTER_WIDTH, job.height],
        pixels.collect(),
    )))
}

fn job_repetitions(job: &GameBoyPrinterJob) -> usize {
    usize::from(job.copies).max(1)
}

fn output_segments(output: &PrinterOutput) -> usize {
    let mut per_copy = usize::from(output.job.has_image());
    if output.job.has_image() && output.job.feed_before != 0 {
        per_copy += 1;
    }
    if output.job.feed_after != 0 {
        per_copy += 1;
    }
    per_copy * job_repetitions(&output.job)
}

fn job_roll_height(job: &GameBoyPrinterJob) -> Option<usize> {
    let image_height = if job.has_image() {
        usize::from(job.feed_before)
            .checked_mul(GAME_BOY_PRINTER_FEED_HEIGHT)?
            .checked_add(job.height)?
    } else {
        0
    };
    let height_per_copy = image_height
        .checked_add(usize::from(job.feed_after).checked_mul(GAME_BOY_PRINTER_FEED_HEIGHT)?)?;
    height_per_copy.checked_mul(job_repetitions(job))
}

fn append_rendered_job(job: &GameBoyPrinterJob, destination: &mut Vec<Color32>) {
    let body = render_body(job).flatten();
    for _ in 0..job_repetitions(job) {
        if let Some(body) = &body {
            destination.resize(
                destination.len()
                    + usize::from(job.feed_before)
                        * GAME_BOY_PRINTER_FEED_HEIGHT
                        * GAME_BOY_PRINTER_WIDTH,
                Color32::WHITE,
            );
            destination.extend_from_slice(&body.pixels);
        }
        destination.resize(
            destination.len()
                + usize::from(job.feed_after)
                    * GAME_BOY_PRINTER_FEED_HEIGHT
                    * GAME_BOY_PRINTER_WIDTH,
            Color32::WHITE,
        );
    }
}

#[cfg(test)]
fn render_job_image(job: &GameBoyPrinterJob, max_pixels: usize) -> Option<ColorImage> {
    let height = job_roll_height(job)?;
    let pixel_count = GAME_BOY_PRINTER_WIDTH.checked_mul(height)?;
    if pixel_count == 0 || pixel_count > max_pixels {
        return None;
    }
    let mut pixels = Vec::with_capacity(pixel_count);
    append_rendered_job(job, &mut pixels);
    Some(ColorImage::new([GAME_BOY_PRINTER_WIDTH, height], pixels))
}

pub(crate) fn draw_printer_viewer_content(ui: &mut egui::Ui, state: &mut PrinterViewerState) {
    let has_output = !state.outputs.is_empty();
    let can_export_latest = state.can_export_latest();
    let can_export_roll = state.can_export_roll();
    let mut save_latest = false;
    let mut save_roll = false;
    #[cfg(not(target_arch = "wasm32"))]
    let mut copy_roll = false;
    let mut clear = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(match state.len() {
            1 => "1 print job".to_string(),
            count => format!("{count} print jobs"),
        });
        if state.discarded_jobs != 0 {
            ui.label(format!("{} older cleared", state.discarded_jobs));
        }
        ui.separator();
        save_latest = ui
            .add_enabled(can_export_latest, egui::Button::new("Save Latest"))
            .clicked();
        save_roll = ui
            .add_enabled(can_export_roll, egui::Button::new("Save Roll"))
            .clicked();
        #[cfg(not(target_arch = "wasm32"))]
        {
            copy_roll = ui
                .add_enabled(can_export_roll, egui::Button::new("Copy Roll"))
                .clicked();
        }
        clear = ui
            .add_enabled(has_output, egui::Button::new("Clear"))
            .clicked();
    });
    if save_latest && let Some(latest) = state.latest_image() {
        crate::debug::export::export_color_image_as_png_interactive(
            "game-boy-printer-latest.png",
            &latest,
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
            ui.label("No print jobs yet");
        });
        return;
    }

    let available_width = ui.available_width().max(1.0);
    let scale = (available_width / GAME_BOY_PRINTER_WIDTH as f32)
        .floor()
        .clamp(1.0, 4.0);
    let scroll_to_end = std::mem::take(&mut state.scroll_to_end);
    let output_count = state.outputs.len();
    egui::ScrollArea::both()
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let texture_manager = ui.ctx().tex_manager();
            let texture_manager_key = std::sync::Arc::as_ptr(&texture_manager) as usize;
            for (index, output) in state.outputs.iter_mut().enumerate() {
                let texture = output.body.as_ref().map(|body| {
                    output
                        .textures
                        .entry(texture_manager_key)
                        .or_insert_with(|| {
                            ui.ctx().load_texture(
                                format!("gb_printer_{index}"),
                                body.clone(),
                                TextureOptions::NEAREST,
                            )
                        })
                        .id()
                });
                let mut last_response = None;
                for _ in 0..job_repetitions(&output.job) {
                    if let Some(texture) = texture {
                        if output.job.feed_before != 0 {
                            paint_feed(ui, output.job.feed_before, scale);
                        }
                        last_response = Some(ui.add(egui::Image::new((
                            texture,
                            egui::vec2(
                                GAME_BOY_PRINTER_WIDTH as f32 * scale,
                                output.job.height as f32 * scale,
                            ),
                        ))));
                    }
                    if output.job.feed_after != 0 {
                        last_response = Some(paint_feed(ui, output.job.feed_after, scale));
                    }
                }
                if scroll_to_end
                    && index + 1 == output_count
                    && let Some(response) = last_response
                {
                    response.scroll_to_me(Some(egui::Align::Max));
                }
            }
        });
}

fn paint_feed(ui: &mut egui::Ui, bands: u8, scale: f32) -> egui::Response {
    let size = egui::vec2(
        GAME_BOY_PRINTER_WIDTH as f32 * scale,
        usize::from(bands) as f32 * GAME_BOY_PRINTER_FEED_HEIGHT as f32 * scale,
    );
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, Color32::WHITE);
    response
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

    fn job(value: u8) -> GameBoyPrinterJob {
        GameBoyPrinterJob {
            pixels: vec![value; GAME_BOY_PRINTER_WIDTH * 8],
            height: 8,
            copies: 1,
            feed_before: 0,
            feed_after: 0,
            palette: 0xE4,
            density: 0x40,
        }
    }

    #[test]
    fn roll_stacks_jobs_without_resampling() {
        let mut state = PrinterViewerState::default();
        assert_eq!(state.append(vec![job(1), job(2)]), 2);
        let roll = state.roll_image().unwrap();
        assert_eq!(roll.size, [GAME_BOY_PRINTER_WIDTH, 16]);
        assert_eq!(roll.pixels[0], Color32::from_gray(192));
        assert_eq!(
            roll.pixels[GAME_BOY_PRINTER_WIDTH * 8],
            Color32::from_gray(64)
        );
    }

    #[test]
    fn copies_and_feed_expand_only_when_rendered() {
        let mut value = job(3);
        value.copies = 2;
        value.feed_before = 1;
        value.feed_after = 2;
        let image = render_job_image(&value, MAX_EXPORT_PIXELS).unwrap();
        assert_eq!(image.size, [GAME_BOY_PRINTER_WIDTH, 112]);
        assert_eq!(image.pixels[0], Color32::WHITE);
        assert_eq!(
            image.pixels[GAME_BOY_PRINTER_WIDTH * GAME_BOY_PRINTER_FEED_HEIGHT],
            Color32::BLACK
        );
    }

    #[test]
    fn feed_only_repeats_post_feed_and_ignores_pre_feed() {
        let feed_only = GameBoyPrinterJob {
            pixels: Vec::new(),
            height: 0,
            copies: 5,
            feed_before: 9,
            feed_after: 3,
            palette: 0,
            density: 0,
        };
        assert_eq!(job_roll_height(&feed_only), Some(240));
        let mut zero_copy = feed_only;
        zero_copy.copies = 0;
        assert_eq!(job_roll_height(&zero_copy), Some(48));
    }

    #[test]
    fn density_scales_darkness_around_the_legacy_default() {
        let mut value = job(1);
        let default = render_body(&value).flatten().unwrap();
        assert_eq!(default.pixels[0], Color32::from_gray(192));
        value.density = 0;
        let light = render_body(&value).flatten().unwrap();
        assert_eq!(light.pixels[0], Color32::from_gray(208));
        value.density = 0x7F;
        let dark = render_body(&value).flatten().unwrap();
        assert_eq!(dark.pixels[0], Color32::from_gray(177));

        value.pixels.fill(3);
        let black = render_body(&value).flatten().unwrap();
        assert_eq!(black.pixels[0], Color32::BLACK);
    }

    #[test]
    fn palette_maps_all_four_logical_colors_before_density() {
        let mut value = job(0);
        value.pixels[..4].copy_from_slice(&[0, 1, 2, 3]);
        let identity = render_body(&value).flatten().unwrap();
        assert_eq!(
            &identity.pixels[..4],
            &[
                Color32::from_gray(255),
                Color32::from_gray(192),
                Color32::from_gray(64),
                Color32::from_gray(0),
            ]
        );

        value.palette = 0x1B;
        let reversed = render_body(&value).flatten().unwrap();
        assert_eq!(
            &reversed.pixels[..4],
            &[
                Color32::from_gray(0),
                Color32::from_gray(64),
                Color32::from_gray(192),
                Color32::from_gray(255),
            ]
        );
    }

    #[test]
    fn latest_export_is_one_body_and_skips_trailing_feed_only_jobs() {
        let mut printed = job(2);
        printed.copies = 3;
        printed.feed_before = 2;
        printed.feed_after = 4;
        let feed_only = GameBoyPrinterJob {
            pixels: Vec::new(),
            height: 0,
            copies: 1,
            feed_before: 0,
            feed_after: 1,
            palette: 0xE4,
            density: 0x40,
        };
        let mut state = PrinterViewerState::default();
        state.append(vec![printed, feed_only]);

        let latest = state.latest_image().unwrap();
        assert_eq!(latest.size, [GAME_BOY_PRINTER_WIDTH, 8]);
        assert_eq!(latest.pixels[0], Color32::from_gray(64));
    }

    #[test]
    fn malformed_jobs_are_not_added() {
        let mut state = PrinterViewerState::default();
        let mut malformed = job(0);
        malformed.pixels.pop();
        assert_eq!(state.append(vec![malformed]), 0);
        assert_eq!(state.len(), 0);
    }

    #[test]
    fn roll_discards_oldest_jobs_at_its_limit() {
        let mut state = PrinterViewerState::default();
        let jobs = (0..=MAX_ROLL_JOBS)
            .map(|index| job((index & 3) as u8))
            .collect();
        assert_eq!(state.append(jobs), MAX_ROLL_JOBS + 1);
        assert_eq!(state.len(), MAX_ROLL_JOBS);
        assert_eq!(state.discarded_jobs, 1);
        assert_eq!(state.outputs[0].job.pixels[0], 1);
    }

    #[test]
    fn retained_jobs_bound_the_number_of_widgets_drawn_per_frame() {
        let mut value = job(0);
        value.copies = u8::MAX;
        let mut state = PrinterViewerState::default();

        assert_eq!(state.append(vec![value; 10]), 10);
        assert_eq!(state.len(), 8);
        assert_eq!(state.discarded_jobs, 2);
        assert!(state.outputs.iter().map(output_segments).sum::<usize>() <= MAX_DISPLAY_SEGMENTS);
    }

    #[test]
    fn oversized_exports_are_refused_without_expanding_the_job() {
        let mut value = job(0);
        value.copies = u8::MAX;
        value.feed_before = 0x0F;
        value.feed_after = 0x0F;
        assert!(job_roll_height(&value).unwrap() > 100_000);
        assert!(render_job_image(&value, MAX_EXPORT_PIXELS).is_none());
    }
}
