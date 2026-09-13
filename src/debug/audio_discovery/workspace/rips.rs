use super::{AudioWorkspace, span_button};
use crate::audio_discovery::rips::{MusicRip, RipDetails};

pub(super) fn draw(ui: &mut egui::Ui, workspace: &mut AudioWorkspace, rip: &MusicRip) {
    ui.label(format!("{} · version {}", rip.format.label(), rip.version));
    ui.small(format!(
        "{} declared songs · first song {}",
        rip.song_count, rip.first_song
    ));
    for (label, text) in [
        ("Title", &rip.title),
        ("Author", &rip.author),
        ("Copyright", &rip.copyright),
    ] {
        if !text.is_empty() {
            ui.small(format!("{label}: {text}"));
        }
    }
    ui.small("Imported music rip. Exports preserve the complete source; individual song data and playback have not been verified.");
    span_button(ui, workspace, rip.source, "Complete source");
    span_button(ui, workspace, rip.header, "Header");
    span_button(ui, workspace, rip.program, "Program payload");
    if let Some(span) = rip.opaque_metadata {
        span_button(ui, workspace, span, "Opaque appended metadata");
    }
    ui.small(format!("Initial load address {:04X}", rip.load_address));
    for (label, entry) in [("Init", rip.init), ("Play", rip.play)] {
        ui.small(format!(
            "{label}: CPU {:04X} · {}",
            entry.cpu_address,
            entry.initial_source_offset.map_or_else(
                || "no initial file mapping".to_owned(),
                |offset| format!("initial file +{offset:06X}")
            )
        ));
    }
    ui.small("These initial mappings do not establish callable code or later bank mappings.");
    match &rip.details {
        RipDetails::Gbs {
            stack_pointer,
            timer_modulo,
            timer_control,
            double_speed,
            initial_play_rate_hz,
            logical_page_count,
            ..
        } => {
            ui.small(format!("Stack {stack_pointer:04X} · timer modulo {timer_modulo:02X} · timer control {timer_control:02X} · double speed {double_speed}"));
            ui.small(format!("{logical_page_count} logical 16 KiB pages"));
            if let Some(rate) = initial_play_rate_hz {
                ui.small(format!(
                    "Initial play rate: {:.6} Hz ({}/{})",
                    rate.numerator as f64 / rate.denominator as f64,
                    rate.numerator,
                    rate.denominator
                ));
            } else {
                ui.small("Initial play rate is not inferred for these timer/vector settings.");
            }
        }
        RipDetails::Nsf {
            ntsc_period_us,
            pal_period_us,
            region,
            expansion_chips,
            initial_banks,
            banking_enabled,
            bank_count,
            ..
        } => {
            ui.small(format!("Region: {region:?} · NTSC period {ntsc_period_us} µs · PAL period {pal_period_us} µs"));
            ui.small(format!(
                "Expansion chips: {}",
                if expansion_chips.is_empty() {
                    "none".to_owned()
                } else {
                    expansion_chips.join(", ")
                }
            ));
            ui.small(format!(
                "Banking enabled: {banking_enabled} · initial banks {initial_banks:02X?}"
            ));
            if let Some(count) = bank_count {
                ui.small(format!("{count} logical 4 KiB banks"));
            }
        }
    }
    for warning in &rip.warnings {
        ui.small(format!("{warning:?}"));
    }
}
