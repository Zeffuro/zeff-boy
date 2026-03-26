use crate::debug::common::ApuDebugInfo;

fn duty_from_nrx1(value: u8) -> &'static str {
    match (value >> 6) & 0x03 {
        0 => "12.5%",
        1 => "25%",
        2 => "50%",
        3 => "75%",
        _ => "?",
    }
}

fn draw_channel_header(ui: &mut egui::Ui, title: &str, enabled: bool) {
    ui.horizontal(|ui| {
        ui.strong(title);
        ui.monospace(if enabled { "ON" } else { "OFF" });
    });
}

fn draw_waveform(ui: &mut egui::Ui, id: &str, samples: &[f32], height: f32) {
    let desired_size = egui::vec2(ui.available_width().max(260.0), height);
    let (rect, _) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let mid_y = rect.center().y;

    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, egui::Color32::from_gray(70)),
        egui::StrokeKind::Outside,
    );
    painter.line_segment(
        [
            egui::pos2(rect.left(), mid_y),
            egui::pos2(rect.right(), mid_y),
        ],
        egui::Stroke::new(1.0, egui::Color32::from_gray(60)),
    );

    if samples.len() < 2 {
        return;
    }

    let mut points = Vec::with_capacity(samples.len());
    let width = rect.width().max(1.0);
    let denom = (samples.len() - 1) as f32;
    for (i, sample) in samples.iter().enumerate() {
        let x = rect.left() + width * (i as f32 / denom);
        let y = mid_y - sample.clamp(-1.0, 1.0) * (rect.height() * 0.45);
        points.push(egui::pos2(x, y));
    }

    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(1.25, egui::Color32::from_rgb(90, 220, 140)),
    ));
    ui.label(
        egui::RichText::new(id)
            .small()
            .color(egui::Color32::from_gray(150)),
    );
}

pub(super) fn draw_apu_viewer_content(
    ui: &mut egui::Ui,
    data: &ApuDebugInfo,
) -> Option<Vec<bool>> {
    let mut muted: Vec<bool> = data.channels.iter().map(|ch| ch.muted).collect();
    let mut mutes_changed = false;

    ui.heading("Master");
    for line in &data.master_lines {
        ui.monospace(line);
    }

    // Unmute All button
    ui.horizontal(|ui| {
        if ui.small_button("Unmute All").clicked() {
            for m in muted.iter_mut() {
                *m = false;
            }
            mutes_changed = true;
        }
    });

    draw_waveform(ui, "Master mix", &data.master_waveform, 84.0);

    for (idx, channel) in data.channels.iter().enumerate() {
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong(&channel.name);
            ui.monospace(if channel.enabled { "ON" } else { "OFF" });
        });
        ui.horizontal(|ui| {
            mutes_changed |= ui.checkbox(&mut muted[idx], "Mute").changed();
            if ui.small_button("Solo").clicked() {
                for m in muted.iter_mut() {
                    *m = true;
                }
                muted[idx] = false;
                mutes_changed = true;
            }
        });
        for line in &channel.register_lines {
            ui.monospace(line);
        }
        if !channel.detail_line.is_empty() {
            ui.monospace(&channel.detail_line);
        }
        draw_waveform(ui, &channel.name, &channel.waveform, 64.0);
    }

    for section in &data.extra_sections {
        ui.separator();
        ui.heading(&section.heading);
        for line in &section.lines {
            ui.monospace(line);
        }
    }

    if mutes_changed { Some(muted) } else { None }
}
