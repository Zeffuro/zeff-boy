use crate::audio_discovery::natsume::{NatsumeSong, NatsumeSongKind};

use super::{AudioWorkspace, span_button};

pub(super) fn draw(ui: &mut egui::Ui, workspace: &mut AudioWorkspace, song: &NatsumeSong) {
    ui.strong(&song.title);
    ui.label(format!("{} channels", song.channels.len()));
    match song.kind {
        NatsumeSongKind::Music => {
            ui.small("Original-driver preview, audio export and mapped-data export are available.");
        }
        NatsumeSongKind::Setup => {
            ui.small("This is a driver setup entry, not music. Its mapped data remains inspectable and exportable.");
        }
        NatsumeSongKind::Silence => {
            ui.small("This is a silence entry, not music. Its mapped data remains inspectable and exportable.");
        }
    }
    egui::CollapsingHeader::new("Advanced sequence data").show(ui, |ui| {
        ui.small(format!("Analysis profile: {}", song.profile));
        ui.small(format!(
            "Priority {} · channel mask {:03X}",
            song.priority, song.channel_mask
        ));
        span_button(ui, workspace, song.table_entry, "Song table entry");
        span_button(ui, workspace, song.header, "Song header");
        for warning in &song.warnings {
            ui.small(warning);
        }
        for channel in &song.channels {
            let hardware = match channel.hardware_kind {
                0 => "Pulse 1",
                1 => "Pulse 2",
                2 => "Wave",
                3 => "Noise",
                4 | 5 => "PCM",
                _ => "Unknown",
            };
            span_button(
                ui,
                workspace,
                channel.entry,
                &format!("Channel {} · {hardware}", channel.number + 1),
            );
            ui.small(format!(
                "{} commands · {} notes · {} wait units · {:?}",
                channel.event_count, channel.note_count, channel.wait_units, channel.termination
            ));
        }
        egui::CollapsingHeader::new("Mapped source ranges").show(ui, |ui| {
            for span in &song.mapped_spans {
                span_button(ui, workspace, *span, "Source range");
            }
        });
    });
}
