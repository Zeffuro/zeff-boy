use crate::debug::DebugViewerData;
use crate::hardware::types::constants::*;

fn reg_index(addr: u16) -> usize {
    (addr - NR10) as usize
}

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

fn draw_waveform(ui: &mut egui::Ui, id: &str, samples: &[f32; 512], height: f32) {
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
    data: &DebugViewerData,
) -> Option<[bool; 4]> {
    let regs = &data.apu_regs;
    let mut muted = data.apu_channel_muted;
    let mut mutes_changed = false;

    ui.heading("Master");
    ui.monospace(format!(
        "NR50:{:02X}  NR51:{:02X}  NR52:{:02X}",
        regs[reg_index(NR50)],
        regs[reg_index(NR51)],
        data.apu_nr52
    ));
    ui.monospace(format!(
        "Power:{}  CH1:{} CH2:{} CH3:{} CH4:{}",
        if data.apu_nr52 & 0x80 != 0 {
            "ON"
        } else {
            "OFF"
        },
        if data.apu_nr52 & 0x01 != 0 { "1" } else { "-" },
        if data.apu_nr52 & 0x02 != 0 { "1" } else { "-" },
        if data.apu_nr52 & 0x04 != 0 { "1" } else { "-" },
        if data.apu_nr52 & 0x08 != 0 { "1" } else { "-" },
    ));

    // Unmute All button
    ui.horizontal(|ui| {
        if ui.small_button("Unmute All").clicked() {
            muted = [false; 4];
            mutes_changed = true;
        }
    });

    draw_waveform(ui, "Master mix", &data.apu_master_samples, 84.0);

    ui.separator();
    draw_channel_header(ui, "CH1 (Square + Sweep)", data.apu_nr52 & 0x01 != 0);
    ui.horizontal(|ui| {
        mutes_changed |= ui.checkbox(&mut muted[0], "Mute").changed();
        if ui.small_button("Solo").clicked() {
            muted = [true, true, true, true];
            muted[0] = false;
            mutes_changed = true;
        }
    });
    ui.monospace(format!(
        "NR10:{:02X} NR11:{:02X} NR12:{:02X} NR13:{:02X} NR14:{:02X}",
        regs[reg_index(NR10)],
        regs[reg_index(NR11)],
        regs[reg_index(NR12)],
        regs[reg_index(NR13)],
        regs[reg_index(NR14)]
    ));
    ui.monospace(format!(
        "Duty:{} Len:{} Vol:{} Env:{} P:{} Freq:{:03X}",
        duty_from_nrx1(regs[reg_index(NR11)]),
        regs[reg_index(NR11)] & 0x3F,
        regs[reg_index(NR12)] >> 4,
        if regs[reg_index(NR12)] & 0x08 != 0 {
            "+"
        } else {
            "-"
        },
        regs[reg_index(NR12)] & 0x07,
        (u16::from(regs[reg_index(NR14)] & 0x07) << 8) | u16::from(regs[reg_index(NR13)]),
    ));
    draw_waveform(ui, "CH1", &data.apu_channel_samples[0], 64.0);

    ui.separator();
    draw_channel_header(ui, "CH2 (Square)", data.apu_nr52 & 0x02 != 0);
    ui.horizontal(|ui| {
        mutes_changed |= ui.checkbox(&mut muted[1], "Mute").changed();
        if ui.small_button("Solo").clicked() {
            muted = [true, true, true, true];
            muted[1] = false;
            mutes_changed = true;
        }
    });
    ui.monospace(format!(
        "NR21:{:02X} NR22:{:02X} NR23:{:02X} NR24:{:02X}",
        regs[reg_index(NR21)],
        regs[reg_index(NR22)],
        regs[reg_index(NR23)],
        regs[reg_index(NR24)]
    ));
    ui.monospace(format!(
        "Duty:{} Len:{} Vol:{} Env:{} P:{} Freq:{:03X}",
        duty_from_nrx1(regs[reg_index(NR21)]),
        regs[reg_index(NR21)] & 0x3F,
        regs[reg_index(NR22)] >> 4,
        if regs[reg_index(NR22)] & 0x08 != 0 {
            "+"
        } else {
            "-"
        },
        regs[reg_index(NR22)] & 0x07,
        (u16::from(regs[reg_index(NR24)] & 0x07) << 8) | u16::from(regs[reg_index(NR23)]),
    ));
    draw_waveform(ui, "CH2", &data.apu_channel_samples[1], 64.0);

    ui.separator();
    draw_channel_header(ui, "CH3 (Wave)", data.apu_nr52 & 0x04 != 0);
    ui.horizontal(|ui| {
        mutes_changed |= ui.checkbox(&mut muted[2], "Mute").changed();
        if ui.small_button("Solo").clicked() {
            muted = [true, true, true, true];
            muted[2] = false;
            mutes_changed = true;
        }
    });
    ui.monospace(format!(
        "NR30:{:02X} NR31:{:02X} NR32:{:02X} NR33:{:02X} NR34:{:02X}",
        regs[reg_index(NR30)],
        regs[reg_index(NR31)],
        regs[reg_index(NR32)],
        regs[reg_index(NR33)],
        regs[reg_index(NR34)]
    ));
    ui.monospace(format!(
        "DAC:{} Len:{} Level:{} Freq:{:03X}",
        if regs[reg_index(NR30)] & 0x80 != 0 {
            "ON"
        } else {
            "OFF"
        },
        regs[reg_index(NR31)],
        (regs[reg_index(NR32)] >> 5) & 0x03,
        (u16::from(regs[reg_index(NR34)] & 0x07) << 8) | u16::from(regs[reg_index(NR33)]),
    ));
    draw_waveform(ui, "CH3", &data.apu_channel_samples[2], 64.0);

    ui.separator();
    draw_channel_header(ui, "CH4 (Noise)", data.apu_nr52 & 0x08 != 0);
    ui.horizontal(|ui| {
        mutes_changed |= ui.checkbox(&mut muted[3], "Mute").changed();
        if ui.small_button("Solo").clicked() {
            muted = [true, true, true, true];
            muted[3] = false;
            mutes_changed = true;
        }
    });
    ui.monospace(format!(
        "NR41:{:02X} NR42:{:02X} NR43:{:02X} NR44:{:02X}",
        regs[reg_index(NR41)],
        regs[reg_index(NR42)],
        regs[reg_index(NR43)],
        regs[reg_index(NR44)]
    ));
    ui.monospace(format!(
        "Len:{} Vol:{} Env:{} P:{} Poly(s={},w={},r={})",
        regs[reg_index(NR41)] & 0x3F,
        regs[reg_index(NR42)] >> 4,
        if regs[reg_index(NR42)] & 0x08 != 0 {
            "+"
        } else {
            "-"
        },
        regs[reg_index(NR42)] & 0x07,
        regs[reg_index(NR43)] >> 4,
        if regs[reg_index(NR43)] & 0x08 != 0 {
            "7"
        } else {
            "15"
        },
        regs[reg_index(NR43)] & 0x07,
    ));
    draw_waveform(ui, "CH4", &data.apu_channel_samples[3], 64.0);

    ui.separator();
    ui.heading("Wave RAM");
    egui::Grid::new("apu_wave_ram_grid").show(ui, |ui| {
        for row in 0..4usize {
            for col in 0..4usize {
                let idx = row * 4 + col;
                ui.monospace(format!("{:02X}", data.apu_wave_ram[idx]));
            }
            ui.end_row();
        }
    });

    if mutes_changed { Some(muted) } else { None }
}
