use crate::debug::types::PerfInfo;

const HISTORY_LEN: usize = 120;

pub(crate) struct PerfHistory {
    fps_samples: Vec<f64>,
}

impl PerfHistory {
    pub(crate) fn new() -> Self {
        Self {
            fps_samples: Vec::with_capacity(HISTORY_LEN),
        }
    }

    pub(crate) fn push(&mut self, fps: f64) {
        if self.fps_samples.len() >= HISTORY_LEN {
            self.fps_samples.remove(0);
        }
        self.fps_samples.push(fps);
    }
}

pub(super) fn draw_performance_content(
    ui: &mut egui::Ui,
    info: &PerfInfo,
    history: &mut PerfHistory,
) {
    if info.fps > 0.0 {
        history.push(info.fps);
    }

    ui.heading("Timing");
    let frame_time_ms = if info.fps > 0.0 {
        1000.0 / info.fps
    } else {
        0.0
    };
    let target_fps = 59.7;
    let target_ms = 1000.0 / target_fps;

    ui.monospace(format!("FPS:        {:.1}", info.fps));
    ui.monospace(format!("Frame time: {:.2} ms", frame_time_ms));
    ui.monospace(format!(
        "Target:     {:.2} ms ({:.1} Hz)",
        target_ms, target_fps
    ));
    let deviation = frame_time_ms - target_ms;
    let dev_str = if deviation.abs() < 0.01 {
        "±0.00 ms".to_string()
    } else {
        format!("{:+.2} ms", deviation)
    };
    ui.monospace(format!("Deviation:  {}", dev_str));

    ui.separator();
    ui.heading("Pipeline");
    ui.monospace(format!("Speed:            {}", info.speed_mode_label));
    ui.monospace(format!("Frames in flight: {}", info.frames_in_flight));
    ui.monospace(format!("Total cycles:     {}", info.cycles));

    ui.separator();
    ui.heading("Hardware");
    ui.monospace(format!("Platform: {}", info.platform_name));
    ui.monospace(format!(
        "Mode: {} (pref: {})",
        info.hardware_label, info.hardware_pref_label
    ));

    if history.fps_samples.len() >= 2 {
        ui.separator();
        ui.heading("FPS History");

        let desired_size = egui::vec2(ui.available_width().min(300.0), 60.0);
        let (rect, _) = ui.allocate_exact_size(desired_size, egui::Sense::hover());

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, egui::Color32::from_rgb(20, 20, 30));

        let samples = &history.fps_samples;
        let min_fps = samples
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min)
            .max(0.0);
        let max_fps = samples.iter().copied().fold(0.0_f64, f64::max);
        let range = (max_fps - min_fps).max(1.0);

        let n = samples.len();
        let dx = rect.width() / (n.max(2) - 1) as f32;

        let points: Vec<egui::Pos2> = samples
            .iter()
            .enumerate()
            .map(|(i, &fps)| {
                let x = rect.left() + i as f32 * dx;
                let t = ((fps - min_fps) / range) as f32;
                let y = rect.bottom() - t * rect.height();
                egui::pos2(x, y)
            })
            .collect();

        if points.len() >= 2 {
            painter.add(egui::Shape::line(
                points,
                egui::Stroke::new(1.5, egui::Color32::from_rgb(100, 200, 100)),
            ));
        }

        if target_fps >= min_fps && target_fps <= max_fps {
            let t = ((target_fps - min_fps) / range) as f32;
            let y = rect.bottom() - t * rect.height();
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                egui::Stroke::new(
                    1.0,
                    egui::Color32::from_rgba_unmultiplied(255, 200, 60, 120),
                ),
            );
        }

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{:.0}", min_fps))
                    .small()
                    .color(egui::Color32::GRAY),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{:.0}", max_fps))
                        .small()
                        .color(egui::Color32::GRAY),
                );
            });
        });
    }
}
