use crate::debug::types::ApuDebugInfo;

fn draw_channel_header(ui: &mut egui::Ui, title: &str, enabled: bool) {
    ui.horizontal(|ui| {
        ui.strong(title);
        ui.monospace(if enabled { "ON" } else { "OFF" });
    });
}

fn draw_compact_mute_solo_row(
    ui: &mut egui::Ui,
    muted: &mut [bool],
    idx: usize,
    mutes_changed: &mut bool,
) {
    let icon_size = egui::vec2(18.0, 18.0);
    let mute_button = egui::Button::new("M").small().selected(muted[idx]);
    if ui
        .add_sized(icon_size, mute_button)
        .on_hover_text("Mute channel")
        .clicked()
    {
        muted[idx] = !muted[idx];
        *mutes_changed = true;
    }

    if ui
        .add_sized(icon_size, egui::Button::new("S").small())
        .on_hover_text("Solo channel")
        .clicked()
    {
        for m in muted.iter_mut() {
            *m = true;
        }
        muted[idx] = false;
        *mutes_changed = true;
    }
}

fn draw_waveform(ui: &mut egui::Ui, samples: &[f32], height: f32) {
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
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "No samples yet",
            egui::TextStyle::Small.resolve(ui.style()),
            egui::Color32::from_gray(140),
        );
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
}

pub(super) fn draw_apu_viewer_content(ui: &mut egui::Ui, data: &ApuDebugInfo) -> Option<Vec<bool>> {
    ui.spacing_mut().item_spacing.y = 2.0;

    let mut muted: Vec<bool> = data.channels.iter().map(|ch| ch.muted).collect();
    let mut mutes_changed = false;

    ui.horizontal(|ui| {
        ui.heading("Master");
        if ui.small_button("Unmute All").clicked() {
            for m in muted.iter_mut() {
                *m = false;
            }
            mutes_changed = true;
        }
    });
    for line in &data.master_lines {
        ui.monospace(line);
    }
    draw_waveform(ui, &data.master_waveform, 30.0);

    for (idx, channel) in data.channels.iter().enumerate() {
        ui.separator();
        ui.horizontal(|ui| {
            draw_channel_header(ui, channel.name, channel.enabled);
            if !channel.detail_line.is_empty() {
                ui.label(egui::RichText::new(&channel.detail_line).small());
            }
        });

        if !channel.register_lines.is_empty() {
            ui.label(egui::RichText::new(channel.register_lines.join("  ")).small());
        }

        ui.horizontal(|ui| {
            draw_compact_mute_solo_row(ui, &mut muted, idx, &mut mutes_changed);
            draw_waveform(ui, &channel.waveform, 26.0);
        });
    }

    for section in &data.extra_sections {
        ui.separator();
        ui.heading(section.heading);
        for line in &section.lines {
            ui.monospace(line);
        }
    }

    if mutes_changed { Some(muted) } else { None }
}
