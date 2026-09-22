use std::collections::HashMap;

use crate::audio_discovery::catalog::{SongId, SongRef};
use crate::audio_discovery::{SampleInventory, ScanReport, SongCandidate, SourceSpan};

mod engines;
mod fingerprints;
mod song_list;
use song_list::draw_song_list;
#[cfg(test)]
use song_list::select_song;
mod mp2k;
mod native;
mod natsume;
mod relations;
mod rips;
use engines::{
    cdda_duration, draw_cdda_details, draw_gax_details, draw_gb_details, draw_module_details,
    draw_nes_details, draw_vgm_details,
};
use mp2k::draw_mp2k_details;

const WIDE_LAYOUT_MIN_WIDTH: f32 = 720.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SelectedSpan {
    pub(super) span: SourceSpan,
    pub(super) label: String,
    pub(super) sample: Option<SampleInventory>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum DetailTab {
    #[default]
    Structure,
    Instruments,
    Samples,
    Hex,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum CompactTab {
    #[default]
    Songs,
    Details,
    SourceData,
}

#[derive(Default)]
pub(super) struct AudioWorkspace {
    pub(super) selected_span: Option<SelectedSpan>,
    pub(super) selected_candidate: Option<SongId>,
    filter: String,
    role_filter: Option<zeff_audio_discovery::classification::AudioRole>,
    role_request: Option<(
        SongId,
        Option<zeff_audio_discovery::classification::AudioRole>,
    )>,
    preview_request: Option<SongId>,
    export_request: Option<SongId>,
    detail_tab: DetailTab,
    compact_tab: CompactTab,
    show_details: bool,
    tbl_map: HashMap<u8, String>,
    hex_reset: bool,
    relationships: Option<relations::CachedGraph>,
}

impl AudioWorkspace {
    pub(super) fn ensure_selection(&mut self, report: &ScanReport) {
        let selected_before = self.selected_candidate;
        if self
            .selected_candidate
            .is_some_and(|id| report.song(id).is_none())
        {
            self.selected_candidate = None;
        }
        if self
            .selected_span
            .as_ref()
            .is_some_and(|selected| !span_is_mapped(selected.span, report.media.byte_len))
        {
            self.selected_span = None;
        }
        if self.selected_candidate.is_none() {
            self.selected_candidate = report
                .song_ids()
                .find(|&id| song_is_music(report.song(id).expect("song id comes from this report")))
                .or_else(|| report.song_ids().next());
        }
        if selected_before != self.selected_candidate {
            self.relationships = None;
        }
        let suppress_hex_default = self.selected_candidate.is_some_and(|song| {
            self.relationships
                .as_ref()
                .is_some_and(|relationships| relationships.suppresses_hex_default_for(song))
        });
        if !suppress_hex_default
            && self.selected_span.is_none()
            && let Some(song) = self.selected_candidate.and_then(|id| report.song(id))
            && let Some(span) = song.span()
        {
            select_span(self, span, format!("{} header", song.title()));
            self.hex_reset = true;
        }
    }

    pub(super) fn take_preview_request(&mut self) -> Option<SongId> {
        self.preview_request.take()
    }

    pub(super) fn take_export_request(&mut self) -> Option<SongId> {
        self.export_request.take()
    }

    pub(super) fn take_role_request(
        &mut self,
    ) -> Option<(
        SongId,
        Option<zeff_audio_discovery::classification::AudioRole>,
    )> {
        self.role_request.take()
    }
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    report: &ScanReport,
    bytes: &[u8],
    classifications: &crate::audio_discovery::roles::Classifications,
    can_preview: impl Fn(SongId) -> bool,
) {
    workspace.ensure_selection(report);
    fingerprints::draw(ui, workspace, report);
    let height = workspace_height(ui.available_height());
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), height),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            if use_wide_layout(ui.available_width()) {
                ui.columns(2, |columns| {
                    draw_song_list(
                        &mut columns[0],
                        workspace,
                        report,
                        classifications,
                        &can_preview,
                    );
                    draw_selected_song(&mut columns[1], workspace, report, bytes);
                });
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut workspace.compact_tab, CompactTab::Songs, "Songs");
                    ui.selectable_value(&mut workspace.compact_tab, CompactTab::Details, "Details");
                    ui.selectable_value(
                        &mut workspace.compact_tab,
                        CompactTab::SourceData,
                        "Source data",
                    );
                });
                ui.separator();
                match workspace.compact_tab {
                    CompactTab::Songs => {
                        draw_song_list(ui, workspace, report, classifications, &can_preview)
                    }
                    CompactTab::Details => draw_details_region(ui, workspace, report),
                    CompactTab::SourceData => draw_hex(ui, workspace, bytes),
                }
            }
        },
    );
}

fn draw_selected_song(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    report: &ScanReport,
    bytes: &[u8],
) {
    ui.heading("Selected song");
    let Some(id) = workspace.selected_candidate else {
        ui.small("Choose a song to inspect it.");
        if workspace.selected_span.is_some() {
            draw_hex(ui, workspace, bytes);
        }
        return;
    };
    let Some(song) = report.song(id) else {
        ui.small("The selected song is no longer in this scan.");
        return;
    };
    ui.strong(song.title());
    ui.small(song.engine());
    if !song_is_music(song) {
        ui.small("This is a silence entry, not music. Its mapped data remains inspectable and exportable.");
    }
    let details_label = if workspace.show_details {
        "Back to song summary"
    } else {
        "View song details"
    };
    let details_button = egui::Button::new(egui::RichText::new(details_label).strong())
        .min_size(egui::vec2(168.0, 30.0));
    let details_button = if workspace.show_details {
        details_button
    } else {
        details_button.fill(ui.visuals().selection.bg_fill)
    };
    if ui.add(details_button).clicked() {
        workspace.show_details = !workspace.show_details;
    }
    if workspace.show_details {
        ui.separator();
        draw_details_region(ui, workspace, report);
    }
    egui::CollapsingHeader::new("Source data").show(ui, |ui| {
        draw_hex(ui, workspace, bytes);
    });
}

fn draw_details_region(ui: &mut egui::Ui, workspace: &mut AudioWorkspace, report: &ScanReport) {
    egui::ScrollArea::vertical()
        .id_salt("audio-discovery-selected-details")
        .max_height(ui.available_height())
        .show(ui, |ui| draw_details(ui, workspace, report));
}

fn draw_details(ui: &mut egui::Ui, workspace: &mut AudioWorkspace, report: &ScanReport) {
    ui.heading("Song details");
    relations::draw(ui, workspace, report);
    let Some(id) = workspace.selected_candidate else {
        ui.label("Select a song to inspect its structures.");
        return;
    };
    let Some(song) = report.song(id) else {
        ui.label("The selected song is no longer in this scan.");
        return;
    };
    match song {
        SongRef::Huge(song) => {
            ui.label(format!(
                "hUGEDriver · descriptor {:04X}",
                song.bound.song.descriptor.offset
            ));
            ui.label("Playback and export require runtime verification. Audio stops at the verified capture endpoint.");
        }
        SongRef::Mp2k(candidate) => draw_mp2k_details(ui, workspace, candidate),
        SongRef::Gax(song) => draw_gax_details(ui, workspace, song),
        SongRef::Gb(song) => draw_gb_details(ui, workspace, song),
        SongRef::Nes(song) => draw_nes_details(ui, workspace, song),
        SongRef::Natsume(song) => natsume::draw(ui, workspace, song),
        SongRef::Aas(song) => engines::draw_aas_details(ui, song),
        SongRef::DescriptorMidi(song) => engines::draw_descriptor_midi_details(ui, song),
        SongRef::Nsq(song) => engines::draw_nsq_details(ui, song),
        SongRef::Radriver(song) => engines::draw_radriver_details(ui, song),
        SongRef::Gbass(song) => engines::draw_gbass_details(ui, song),
        SongRef::AasStream(song) => engines::draw_aas_stream_details(ui, song),
        SongRef::AasPcm(song) => engines::draw_aas_pcm_details(ui, song),
        SongRef::NesNative(song) => engines::draw_nes_native_details(ui, song),
        SongRef::GbNative(song) => engines::draw_gb_native_details(ui, song),
        SongRef::GbTose(_)
        | SongRef::GbQuickThunder(_)
        | SongRef::GbGhx(_)
        | SongRef::GbSoundSystem(_)
        | SongRef::GbCarillon(_)
        | SongRef::WsTose(_)
        | SongRef::NesTose(_)
        | SongRef::GbMusyx(_) => native::draw(ui, song),
        SongRef::SegaPsg(song) => engines::draw_sega_psg_details(ui, workspace, song),
        SongRef::Musyx(song) => engines::draw_musyx_details(ui, song),
        SongRef::Krawall(song) => engines::draw_krawall_details(ui, song),
        SongRef::GaxNative(song) => engines::draw_gax_native_details(ui, song),
        SongRef::EngineSoftware(song) => engines::draw_engine_software_details(ui, song),
        SongRef::Vgm(log) => draw_vgm_details(ui, workspace, log),
        SongRef::Rip(rip) => rips::draw(ui, workspace, rip),
        SongRef::Module(module) => draw_module_details(ui, workspace, module),
        #[cfg(not(target_arch = "wasm32"))]
        SongRef::Cdda(track) => draw_cdda_details(ui, track),
    }
}

pub(super) fn song_title(candidate: &SongCandidate) -> String {
    SongRef::Mp2k(candidate).title()
}

fn song_label(song: SongRef<'_>) -> String {
    let detail = match song {
        SongRef::Huge(_) => "4 channels · verification required".to_owned(),
        SongRef::Mp2k(candidate) => format!("{} tracks", candidate.tracks.len()),
        SongRef::Gax(song) => format!("{} channels", song.channels.len()),
        SongRef::EngineSoftware(song) => format!("{} channels", song.channels),
        SongRef::Krawall(song) => {
            format!("{} channels · order {}", song.channels, song.start_order)
        }
        SongRef::GaxNative(song) => format!("{} channels", song.channels),
        SongRef::Musyx(song) => format!("{} channels", song.channels),
        SongRef::Aas(song) => format!("{} channels", song.channels),
        SongRef::DescriptorMidi(song) => format!("{} channels", song.channels),
        SongRef::Nsq(song) => format!("{} instruments", song.instruments),
        SongRef::Radriver(song) => format!("{} samples", song.samples.len()),
        SongRef::Gbass(song) => format!("{} channels", song.channels),
        SongRef::AasStream(song) => format!("{} encoded bytes", song.encoded_data.byte_len),
        SongRef::AasPcm(song) => format!(
            "{} Hz · {} sample bytes",
            song.sample_rate, song.sample_data.byte_len
        ),
        SongRef::NesNative(song) => format!("{} channels", song.channels.len()),
        SongRef::GbNative(song) => format!("{} channels", song.channels.len()),
        SongRef::GbMusyx(song) => format!("{} tracks", song.tracks.len()),
        SongRef::GbTose(song) => format!("{} tracks · bank {:02X}", song.tracks.len(), song.bank),
        SongRef::GbQuickThunder(song) => {
            format!("{} tracks · bank {:02X}", song.tracks.len(), song.bank)
        }
        SongRef::GbGhx(song) => format!(
            "{} tracks · module {} / subsong {}",
            song.tracks.len(),
            song.module,
            song.subsong
        ),
        SongRef::GbSoundSystem(song) => format!("{} tracks", song.tracks.len()),
        SongRef::GbCarillon(song) => format!("{} tracks", song.tracks.len()),
        SongRef::WsTose(song) => format!("{} tracks · native WS", song.tracks.len()),
        SongRef::NesTose(song) => format!("{} tracks · NTSC", song.tracks.len()),
        SongRef::SegaPsg(song) => format!("{} channels · NTSC", song.channels.len()),
        SongRef::Gb(song) if song.index == 0 => "silence entry · not music".to_owned(),
        SongRef::Gb(song) => format!("{} channels", song.channels.len()),
        SongRef::Nes(song) => format!("{} channels", song.channels.len()),
        SongRef::Natsume(song) => match song.kind {
            crate::audio_discovery::natsume::NatsumeSongKind::Music => {
                format!("{} channels · playable", song.channels.len())
            }
            crate::audio_discovery::natsume::NatsumeSongKind::Setup => {
                "setup data · not music".to_owned()
            }
            crate::audio_discovery::natsume::NatsumeSongKind::Silence => {
                "silence entry · not music".to_owned()
            }
        },
        SongRef::Rip(rip) => format!("first song {} · preserved source", rip.first_song),
        SongRef::Vgm(log) => format!(
            "{} commands · {:.2} s",
            log.command_count,
            log.samples as f64 / f64::from(crate::audio_discovery::vgm::TICKS_PER_SECOND)
        ),
        SongRef::Module(module) => format!("{} patterns", module.patterns),
        #[cfg(not(target_arch = "wasm32"))]
        SongRef::Cdda(track) => cdda_duration(track.pcm_frames),
    };
    format!("{} · {} · {}", song.title(), song.engine(), detail)
}

fn draw_selected_summary(ui: &mut egui::Ui, workspace: &AudioWorkspace) {
    let Some(selected) = &workspace.selected_span else {
        ui.small("Select a mapped structure to show it in Hex.");
        return;
    };
    ui.monospace(span_location(selected.span));
    ui.label(&selected.label);
}

fn span_location(span: SourceSpan) -> String {
    match span.canonical_cpu_address {
        Some(address) => format!(
            "CPU ROM 0x{address:08X} (effective +0x{:06X})",
            span.effective_offset
        ),
        None => format!("File +0x{:06X}", span.effective_offset),
    }
}

fn draw_hex(ui: &mut egui::Ui, workspace: &mut AudioWorkspace, bytes: &[u8]) {
    ui.heading("Hex");
    let Some(selected) = &workspace.selected_span else {
        ui.small("Select a header, track, descriptor, sample, or waveform.");
        return;
    };
    let Some(selected_bytes) = selected_bytes(bytes, selected.span) else {
        ui.colored_label(
            egui::Color32::LIGHT_RED,
            "The selected range is outside loaded media.",
        );
        return;
    };
    ui.monospace(format!(
        "{} · {} bytes",
        span_location(selected.span),
        selected_bytes.len()
    ));
    ui.small(&selected.label);

    let layout = super::super::hex_viewer::hex_layout(
        ui.available_width(),
        6,
        super::super::common::debug_mono_font(ui).size,
    );
    let formats = super::super::hex_viewer::hex_text_formats(ui, layout);
    super::super::hex_viewer::draw_hex_header(ui, "Offset   ", &formats);
    let row_count = selected_bytes.len().div_ceil(layout.bytes_per_row);
    let row_height = ui.fonts_mut(|fonts| fonts.row_height(&formats.normal.font_id));
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt("audio-discovery-hex")
        .max_height(ui.available_height());
    if std::mem::take(&mut workspace.hex_reset) {
        scroll = scroll.vertical_scroll_offset(0.0);
    }
    scroll.show_rows(ui, row_height, row_count, |ui, rows| {
        let page = visible_hex_rows(selected.span, selected_bytes, layout.bytes_per_row, rows);
        super::super::hex_viewer::draw_hex_grid(
            ui,
            &page,
            6,
            &formats,
            super::super::hex_viewer::HexGridOptions {
                flash_ticks: None,
                tbl_map: &workspace.tbl_map,
                watched_ranges: &[],
            },
        );
    });
}

fn span_button(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    span: impl Into<SourceSpan>,
    label: &str,
) {
    let span = span.into();
    let selected = workspace
        .selected_span
        .as_ref()
        .is_some_and(|value| value.span == span);
    if ui
        .selectable_label(
            selected,
            format!(
                "{label}: +{:06X} · {} bytes",
                span.effective_offset, span.byte_len
            ),
        )
        .clicked()
    {
        select_span(workspace, span, label.to_owned());
    }
}

fn sample_span_button(
    ui: &mut egui::Ui,
    workspace: &mut AudioWorkspace,
    sample: SampleInventory,
    label: &str,
) {
    let selected = workspace
        .selected_span
        .as_ref()
        .is_some_and(|value| value.sample == Some(sample));
    if ui
        .selectable_label(
            selected,
            format!(
                "{label}: +{:06X} · {} encoded bytes",
                sample.data.effective_offset, sample.data.byte_len
            ),
        )
        .clicked()
    {
        select_sample_span(workspace, sample, label.to_owned());
    }
}

fn select_span(workspace: &mut AudioWorkspace, span: impl Into<SourceSpan>, label: String) {
    let span = span.into();
    workspace.hex_reset = workspace
        .selected_span
        .as_ref()
        .is_none_or(|selected| selected.span != span);
    workspace.selected_span = Some(SelectedSpan {
        span,
        label,
        sample: None,
    });
}

fn select_sample_span(workspace: &mut AudioWorkspace, sample: SampleInventory, label: String) {
    let span = SourceSpan::from(sample.data);
    workspace.hex_reset = workspace
        .selected_span
        .as_ref()
        .is_none_or(|selected| selected.span != span);
    workspace.selected_span = Some(SelectedSpan {
        span,
        label,
        sample: Some(sample),
    });
}

pub(super) fn use_wide_layout(width: f32) -> bool {
    width >= WIDE_LAYOUT_MIN_WIDTH
}

fn song_is_music(song: SongRef<'_>) -> bool {
    match song {
        SongRef::Natsume(song) => {
            matches!(
                song.kind,
                crate::audio_discovery::natsume::NatsumeSongKind::Music
            )
        }
        SongRef::Gb(song) => song.index != 0,
        _ => true,
    }
}

pub(super) fn workspace_height(available_height: f32) -> f32 {
    available_height.max(0.0)
}

pub(super) fn filtered_song_ids(report: &ScanReport, filter: &str) -> Vec<SongId> {
    let needle = filter.trim().to_ascii_lowercase();
    report
        .song_ids()
        .filter(|&id| {
            let song = report.song(id).expect("song id comes from this report");
            let mut searchable = format!("{} {}", song.title(), song.engine());
            if let Some(span) = song.span() {
                searchable.push_str(&format!(" {:06x}", span.effective_offset));
                if let Some(address) = span.canonical_cpu_address {
                    searchable.push_str(&format!(" {address:08x}"));
                }
            }
            if let SongRef::Mp2k(candidate) = song {
                searchable.push_str(&format!(" voicegroup {:06x}", candidate.voicegroup_offset));
                for entry in &candidate.table_entries {
                    searchable.push_str(&format!(
                        " song {} table {:06x} player {}",
                        entry.index, entry.table_offset, entry.player
                    ));
                }
            }
            searchable.to_ascii_lowercase().contains(&needle)
        })
        .collect()
}

pub(super) fn selected_bytes(bytes: &[u8], span: impl Into<SourceSpan>) -> Option<&[u8]> {
    let span = span.into();
    let start = usize::try_from(span.effective_offset).ok()?;
    let len = usize::try_from(span.byte_len).ok()?;
    let end = start.checked_add(len)?;
    bytes.get(start..end)
}

pub(super) fn visible_hex_rows(
    span: impl Into<SourceSpan>,
    bytes: &[u8],
    bytes_per_row: usize,
    rows: std::ops::Range<usize>,
) -> Vec<(u32, u8)> {
    let span = span.into();
    if bytes_per_row == 0 {
        return Vec::new();
    }
    let start = rows.start.saturating_mul(bytes_per_row).min(bytes.len());
    let end = rows.end.saturating_mul(bytes_per_row).min(bytes.len());
    bytes[start..end]
        .iter()
        .enumerate()
        .filter_map(|(index, byte)| {
            u32::try_from(start + index)
                .ok()
                .and_then(|offset| span.effective_offset.checked_add(offset))
                .map(|address| (address, *byte))
        })
        .collect()
}

fn span_is_mapped(span: SourceSpan, media_len: u64) -> bool {
    let start = u64::from(span.effective_offset);
    let end = start.saturating_add(u64::from(span.byte_len));
    span.byte_len > 0 && end <= media_len
}
