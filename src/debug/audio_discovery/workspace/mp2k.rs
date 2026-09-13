use super::*;
use crate::audio_discovery::{InstrumentInventory, ToneInventory, TrackInventory};

pub(super) fn draw_mp2k_details(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    candidate: &SongCandidate,
) {
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut workspace.detail_tab, DetailTab::Structure, "Structure");
        ui.selectable_value(
            &mut workspace.detail_tab,
            DetailTab::Instruments,
            "Instruments",
        );
        ui.selectable_value(&mut workspace.detail_tab, DetailTab::Samples, "Samples");
        ui.selectable_value(&mut workspace.detail_tab, DetailTab::Hex, "Hex");
    });
    ui.separator();
    match workspace.detail_tab {
        DetailTab::Structure => draw_structure(ui, workspace, candidate),
        DetailTab::Instruments => draw_instruments(ui, workspace, candidate),
        DetailTab::Samples => draw_samples(ui, workspace, candidate),
        DetailTab::Hex => draw_selected_summary(ui, workspace),
    }
}

fn draw_structure(ui: &mut egui::Ui, workspace: &mut AudioWorkspace, candidate: &SongCandidate) {
    ui.label(format!(
        "{} · priority {} · reverb {}",
        song_title(candidate),
        candidate.priority,
        candidate.reverb
    ));
    ui.small(format!(
        "Voicegroup ROM +{:06X}",
        candidate.voicegroup_offset
    ));
    ui.small(if candidate.evidence.song_table_verified {
        "Linked to a recognized MP2k selector and song table"
    } else {
        "Structural candidate; engine and table identity are unverified"
    });
    span_button(ui, workspace, candidate.header, "Header");
    for table_entry in &candidate.table_entries {
        span_button(
            ui,
            workspace,
            table_entry.entry,
            &format!(
                "Song {} table entry · player {} · table +{:06X}",
                table_entry.index, table_entry.player, table_entry.table_offset
            ),
        );
    }
    ui.small(format!(
        "{} validated instruments · {} decoded tracks",
        candidate.evidence.validated_instruments, candidate.evidence.decoded_tracks
    ));
    for (track_index, track) in candidate.tracks.iter().enumerate() {
        egui::CollapsingHeader::new(format!(
            "Track {track_index} · {} events · {:?}",
            track.event_count, track.termination
        ))
        .default_open(track_index == 0)
        .show(ui, |ui| draw_track(ui, workspace, track_index, track));
    }
    if !candidate.warnings.is_empty() {
        ui.separator();
        ui.label("Warnings");
        for warning in &candidate.warnings {
            ui.small(super::super::warning_text(warning));
        }
    }
}

fn draw_track(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    index: usize,
    track: &TrackInventory,
) {
    ui.small(format!("Entry CPU 0x{:08X}", track.entry_address));
    ui.small(format!("Voices: {:?}", track.voices));
    for voice_keys in &track.voice_keys {
        ui.small(format!(
            "Voice {} keys: {:?}",
            voice_keys.voice, voice_keys.keys
        ));
    }
    for (span_index, span) in track.spans.iter().copied().enumerate() {
        span_button(
            ui,
            workspace,
            span,
            &format!("Track {index} command span {span_index}"),
        );
    }
}

fn draw_instruments(ui: &mut egui::Ui, workspace: &mut AudioWorkspace, candidate: &SongCandidate) {
    ui.label(format!("{} instruments", song_title(candidate)));
    if candidate.instruments.is_empty() {
        ui.small("No validated instruments were retained for this candidate.");
    }
    for instrument in &candidate.instruments {
        let keys = used_keys(candidate, instrument.voice);
        egui::CollapsingHeader::new(format!(
            "Voice {} · kind {:02X}",
            instrument.voice, instrument.kind
        ))
        .show(ui, |ui| draw_instrument(ui, workspace, instrument, &keys));
    }
}

fn draw_instrument(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    instrument: &InstrumentInventory,
    keys: &[u8],
) {
    draw_tone(ui, workspace, instrument.voice, "base", &instrument.tone);
    if let Some(key_map) = instrument.key_map {
        span_button(
            ui,
            workspace,
            key_map,
            &format!("Voice {} key map", instrument.voice),
        );
    }
    for region in instrument.regions.iter().filter(|region| {
        keys.iter()
            .any(|key| (region.key_start..=region.key_end).contains(key))
    }) {
        egui::CollapsingHeader::new(format!(
            "Keys {}–{} · descriptor {}",
            region.key_start, region.key_end, region.descriptor_index
        ))
        .show(ui, |ui| {
            if let Some(tone) = &region.tone {
                draw_tone(
                    ui,
                    workspace,
                    instrument.voice,
                    &format!("keys {}–{}", region.key_start, region.key_end),
                    tone,
                );
            }
            if let Some(warning) = &region.warning {
                ui.small(super::super::warning_text(warning));
            }
        });
    }
}

fn draw_samples(ui: &mut egui::Ui, workspace: &mut AudioWorkspace, candidate: &SongCandidate) {
    ui.label(format!("{} samples and waveforms", song_title(candidate)));
    let mut found = false;
    for instrument in &candidate.instruments {
        if instrument.tone.sample_header.is_some()
            || instrument.tone.sample.is_some()
            || instrument.tone.waveform.is_some()
        {
            found = true;
            egui::CollapsingHeader::new(format!("Voice {} sample / waveform", instrument.voice))
                .show(ui, |ui| {
                    draw_tone(ui, workspace, instrument.voice, "base", &instrument.tone)
                });
        }
        let keys = used_keys(candidate, instrument.voice);
        for region in instrument.regions.iter().filter(|region| {
            keys.iter()
                .any(|key| (region.key_start..=region.key_end).contains(key))
        }) {
            if let Some(tone) = &region.tone
                && (tone.sample_header.is_some()
                    || tone.sample.is_some()
                    || tone.waveform.is_some())
            {
                found = true;
                egui::CollapsingHeader::new(format!(
                    "Voice {} keys {}–{}",
                    instrument.voice, region.key_start, region.key_end
                ))
                .show(ui, |ui| {
                    draw_tone(
                        ui,
                        workspace,
                        instrument.voice,
                        &format!("keys {}–{}", region.key_start, region.key_end),
                        tone,
                    );
                });
            }
        }
    }
    if !found {
        ui.small("No sample or waveform spans were retained for this candidate.");
    }
}

fn draw_tone(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    voice: u8,
    label: &str,
    tone: &ToneInventory,
) {
    span_button(
        ui,
        workspace,
        tone.descriptor,
        &format!("Voice {voice} {label} descriptor"),
    );
    ui.small(format!(
        "Kind {:02X} · key {} · length {} · pan/sweep {:02X}{}",
        tone.kind,
        tone.key,
        tone.length,
        tone.pan_sweep,
        if tone.fixed_pitch {
            " · fixed pitch"
        } else {
            ""
        }
    ));
    if let Some(adsr) = tone.adsr {
        ui.small(format!(
            "ADSR {:02X} {:02X} {:02X} {:02X}",
            adsr[0], adsr[1], adsr[2], adsr[3]
        ));
    }
    if let Some(header) = tone.sample_header {
        span_button(
            ui,
            workspace,
            header,
            &format!("Voice {voice} {label} sample header"),
        );
    }
    if let Some(sample) = &tone.sample {
        draw_sample_spans(ui, workspace, voice, label, sample);
    }
    if let Some(waveform) = tone.waveform {
        span_button(
            ui,
            workspace,
            waveform,
            &format!("Voice {voice} {label} waveform"),
        );
    }
    if let Some(recipe) = &tone.synthesis {
        ui.small(format!("Camelot synthesis · {:?}", recipe.kind));
        span_button(
            ui,
            workspace,
            recipe.parameters,
            &format!("Voice {voice} {label} synthesis recipe"),
        );
        ui.small("SF2/WAV approximate the recipe; animated pulse width and filter timing are not preserved.");
    }
}

fn draw_sample_spans(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    voice: u8,
    label: &str,
    sample: &SampleInventory,
) {
    sample_span_button(
        ui,
        workspace,
        *sample,
        &format!("Voice {voice} {label} sample data"),
    );
    ui.small(format!(
        "{:?} · {:?} · {} encoded bytes · {} decoded points · {:.2} Hz",
        sample.encoding,
        sample.direction,
        sample.data.byte_len,
        sample.decoded_len,
        f64::from(sample.frequency) / 1024.0,
    ));
    let loop_info = match (sample.direction, sample.looped) {
        (crate::audio_discovery::SampleDirection::Forward, true) => {
            format!(
                "effective loop: decoded {}..{}",
                sample.loop_start, sample.decoded_len
            )
        }
        (crate::audio_discovery::SampleDirection::Forward, false) => {
            "effective loop: none (one-shot)".to_owned()
        }
        (crate::audio_discovery::SampleDirection::Reverse, true) => {
            "effective loop: none (reverse playback ignores the source loop flag)".to_owned()
        }
        (crate::audio_discovery::SampleDirection::Reverse, false) => {
            "effective loop: none (reverse one-shot)".to_owned()
        }
    };
    ui.small(loop_info);
}

fn used_keys(candidate: &SongCandidate, voice: u8) -> Vec<u8> {
    let mut keys = candidate
        .tracks
        .iter()
        .flat_map(|track| track.voice_keys.iter())
        .filter(|voice_keys| voice_keys.voice == voice)
        .flat_map(|voice_keys| voice_keys.keys.iter().copied())
        .collect::<Vec<_>>();
    keys.sort_unstable();
    keys.dedup();
    keys
}
