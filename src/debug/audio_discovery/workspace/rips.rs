use super::{AudioWorkspace, span_button};
use crate::audio_discovery::rips::{MusicRip, RipDetails, RipFormat};

pub(super) fn draw(ui: &mut egui::Ui, workspace: &mut AudioWorkspace, rip: &MusicRip) {
    ui.label(match rip.version {
        Some(version) => format!("{} · version {version}", rip.format.label()),
        None => rip.format.label().to_owned(),
    });
    if let (Some(count), Some(first)) = (rip.song_count, rip.first_song) {
        ui.small(format!("{count} declared songs · first song {first}"));
    } else {
        ui.small("Song count is not declared.");
    }
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
    let (header_label, program_label) = if rip.format == RipFormat::Wsr {
        ("Trailer", "ROM body")
    } else {
        ("Header", "Program payload")
    };
    span_button(ui, workspace, rip.header, header_label);
    span_button(ui, workspace, rip.program, program_label);
    if let Some(span) = rip.opaque_metadata {
        span_button(ui, workspace, span, "Opaque appended metadata");
    }
    if let Some(address) = rip.load_address {
        ui.small(format!("Initial load address {address:04X}"));
    }
    let init_label = if rip.format == RipFormat::Hes {
        "Request"
    } else {
        "Init"
    };
    for (label, entry) in rip
        .init
        .map(|entry| (init_label, entry))
        .into_iter()
        .chain(rip.play.map(|entry| ("Play", entry)))
    {
        ui.small(format!(
            "{label}: CPU {:04X} · {}",
            entry.cpu_address,
            entry.initial_source_offset.map_or_else(
                || if matches!(rip.format, RipFormat::Nsfe | RipFormat::Hes)
                    || rip.version == Some(2)
                {
                    "initial file mapping not resolved"
                } else {
                    "no initial file mapping"
                }
                .to_owned(),
                |offset| format!("initial file +{offset:06X}")
            )
        ));
    }
    if rip.init.is_some() || rip.play.is_some() {
        ui.small("These initial mappings do not establish callable code or later bank mappings.");
    }
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
        RipDetails::Nsf2 {
            irq_enabled,
            init_non_returning,
            play_suppressed,
            metadata_required,
            header_ntsc_period_us,
            header_pal_period_us,
            metadata,
            ..
        } => {
            ui.small(format!("IRQ: {irq_enabled} · non-returning init: {init_non_returning} · play suppressed: {play_suppressed}"));
            ui.small(format!("Metadata required: {metadata_required} · header NTSC period {header_ntsc_period_us} µs · PAL period {header_pal_period_us} µs"));
            ui.small("Effective timing and region overrides have not been resolved.");
            draw_chunks(ui, workspace, metadata);
        }
        RipDetails::Nsfe { chunks, .. } => {
            draw_chunks(ui, workspace, chunks);
        }
        RipDetails::Hes {
            raw_start_song,
            initial_mprs,
            physical_load_address,
            ..
        } => {
            ui.small(format!(
                "Start selector: {raw_start_song} · physical load: {physical_load_address:06X}"
            ));
            ui.small(format!("Initial MPRs: {initial_mprs:02X?}"));
        }
        RipDetails::Wsr {
            raw_start_song,
            raw_byte_4,
            reset_entry,
            cartridge_footer,
            ..
        } => {
            ui.small(format!(
                "Start selector: {raw_start_song} · raw trailer byte 4: {raw_byte_4:02X}"
            ));
            span_button(ui, workspace, *reset_entry, "Reset bytes");
            span_button(ui, workspace, *cartridge_footer, "Cartridge footer");
        }
    }
    for warning in &rip.warnings {
        ui.small(format!("{warning:?}"));
    }
}

fn draw_chunks(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    chunks: &[crate::audio_discovery::rips::NsfeChunk],
) {
    ui.small(format!("{} preserved chunks", chunks.len()));
    egui::CollapsingHeader::new("Chunks").show(ui, |ui| {
        for (index, chunk) in chunks.iter().enumerate() {
            span_button(
                ui,
                workspace,
                crate::audio_discovery::tracker::FileSpan {
                    offset: chunk.header.offset,
                    byte_len: chunk.header.byte_len + chunk.payload.byte_len,
                },
                &format!("{index}: {:?}", String::from_utf8_lossy(&chunk.id)),
            );
        }
    });
}
