use crate::audio_discovery::formats::FormatAvailability;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc::{self, Receiver, TryRecvError},
};

use crate::audio_discovery::SampleInventory;
use crate::audio_discovery::assets::{AudioExportRequest, samples};
use crate::audio_discovery::catalog::{SongId, SongRef};
use crate::audio_discovery::export::SongExportRequest;
use crate::audio_discovery::extract::ExtractionRequest;
use crate::audio_discovery::formats::{
    AudioFormat, BankSelect, ExportKind, SONG_FORMATS, SongFormat,
};
use crate::audio_discovery::media::{ScanInput, ScanManifest};
use crate::audio_discovery::naming;
use crate::audio_discovery::render::{
    MAX_DURATION_SECONDS, MAX_FADE_SECONDS, MAX_LOOP_PASSES, PlaybackGain, RenderOptions,
    SAMPLE_RATES,
};

use super::workspace::SelectedSpan;

mod options;
use options::{draw_native_options, draw_song_options};

struct PendingExport {
    source: Arc<ScanInput>,
    receiver: Receiver<anyhow::Result<Option<String>>>,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
    label: String,
}

pub(super) struct ExportState {
    status: Option<String>,
    pending: Option<PendingExport>,
    options: RenderOptions,
    native_options: RenderOptions,
    song_format: SongFormat,
    sample_format: AudioFormat,
    gsf_support: Option<(SongId, bool)>,
}

impl Default for ExportState {
    fn default() -> Self {
        Self {
            status: None,
            pending: None,
            options: RenderOptions::default(),
            native_options: RenderOptions {
                max_seconds: crate::audio_discovery::natsume::preview::DEFAULT_DURATION_SECONDS,
                ..RenderOptions::default()
            },
            song_format: SongFormat::default(),
            sample_format: AudioFormat::default(),
            gsf_support: None,
        }
    }
}

impl Drop for ExportState {
    fn drop(&mut self) {
        if let Some(pending) = &self.pending {
            pending.cancel.store(true, Ordering::Relaxed);
        }
    }
}

impl ExportState {
    pub(super) fn clear_status(&mut self) {
        self.status = None;
        self.gsf_support = None;
        if let Some(pending) = &self.pending {
            pending.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub(super) fn poll(&mut self, source: Option<&Arc<ScanInput>>) {
        let Some(pending) = &self.pending else {
            return;
        };
        let result = match pending.receiver.try_recv() {
            Err(TryRecvError::Empty) => return,
            Ok(result) => result,
            Err(TryRecvError::Disconnected) => {
                Err(anyhow::anyhow!("audio export worker stopped unexpectedly"))
            }
        };
        let current_source = source.is_some_and(|current| Arc::ptr_eq(current, &pending.source));
        let label = pending.label.clone();
        self.pending = None;
        if !current_source {
            self.status = None;
            return;
        }
        self.status = Some(match result {
            Ok(message) => message.unwrap_or_else(|| format!("{label} exported")),
            Err(error) => format!("{label} export failed: {error:#}"),
        });
    }

    fn is_busy(&self) -> bool {
        self.pending.is_some()
    }
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    state: &mut ExportState,
    input: &Arc<ScanInput>,
    manifest: &ScanManifest,
    candidate_index: Option<SongId>,
    selected: Option<&SelectedSpan>,
) {
    state.poll(Some(input));
    ui.horizontal_wrapped(|ui| {
        if ui.button("Export report JSON…").clicked()
            && let Some(path) = crate::platform::FileDialog::new()
                .set_title("Export Audio Discovery Report")
                .add_filter("JSON", &["json"])
                .set_file_name(&naming::report(input))
                .save_file()
        {
            state.status = Some(match manifest.write_new(&path) {
                Ok(()) => format!("Report exported to {}", path.display()),
                Err(error) => format!("Report export failed: {error:#}"),
            });
        }

        let can_export_selection = selected.is_some() && !state.is_busy();
        if ui
            .add_enabled(
                can_export_selection,
                egui::Button::new("Extract selected bytes…"),
            )
            .clicked()
            && let Some(path) = crate::platform::FileDialog::new()
                .set_title("Extract Raw Audio Selection")
                .add_filter("ZIP archive", &["zip"])
                .set_file_name(&naming::selection(input, selected.unwrap().span))
                .save_file()
            && let Some(selected) = selected
        {
            start_selection_export(state, input, manifest, selected, path);
        }
    });
    let song = candidate_index.and_then(|id| manifest.scan.song(id));
    let mp2k_index = match candidate_index {
        Some(SongId::Mp2k(index)) => Some(index),
        _ => None,
    };
    if let Some(id) = candidate_index
        && state.gsf_support.map(|cached| cached.0) != Some(id)
    {
        let supported = if let Some(index) = mp2k_index {
            crate::audio_discovery::gsf::available(input, manifest, index)
        } else {
            song.is_some_and(|song| song.supports(SongFormat::Gsf))
        };
        state.gsf_support = Some((id, supported));
    }
    let gsf_supported = state.gsf_support.is_some_and(|(_, supported)| supported);
    let supports = |format: SongFormat| {
        format.available()
            && song.is_some_and(|song| song.supports(format))
            && (!format.is_gsf() || gsf_supported)
    };
    if let Some(song) = song
        && (!song.supports(state.song_format) || !supports(state.song_format))
        && let Some(info) = SONG_FORMATS.iter().find(|info| supports(info.format))
    {
        state.song_format = info.format;
    }
    let selected_sample = song.and_then(|song| match song {
        SongRef::Mp2k(song) => selected.and_then(|selected| selected_sample(song, selected)),
        _ => None,
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Song format");
        egui::ComboBox::from_id_salt("audio-song-format")
            .selected_text(state.song_format.info().label)
            .show_ui(ui, |ui| {
                for info in SONG_FORMATS.iter().filter(|info| supports(info.format)) {
                    ui.selectable_value(&mut state.song_format, info.format, info.label);
                }
            });
        let info = state.song_format.info();
        if ui
            .add_enabled(
                song.is_some() && !state.is_busy(),
                egui::Button::new("Export song…"),
            )
            .clicked()
            && let Some(path) = crate::platform::FileDialog::new()
                .set_title(&format!("Export {}", info.label))
                .add_filter(info.extension, &[info.extension])
                .set_file_name(&naming::song(
                    input,
                    &manifest.scan,
                    candidate_index.unwrap(),
                    state.song_format,
                ))
                .save_file()
            && let Some(index) = candidate_index
        {
            let options = export_options(state, manifest, index);
            match SongExportRequest::prepare(input, manifest, index, state.song_format, options) {
                Ok(request) => start_worker(state, input, info.label, move |cancel, progress| {
                    request.write_new(&path, cancel, progress)
                }),
                Err(error) => state.status = Some(format!("Cannot export: {error:#}")),
            }
        }
        let can_export_all = !state.is_busy() && state.song_format.available()
            && manifest.scan.song_ids().any(|id| manifest.scan.song(id).is_some_and(|song| song.supports(state.song_format)));
        if ui.add_enabled(can_export_all, egui::Button::new("Export all songs…"))
            .on_hover_text("Export every supported entry in this format to one ZIP, including entries hidden by the current filter.").clicked()
            && let Some(path) = crate::platform::FileDialog::new()
                .set_title("Export All Songs")
                .add_filter("ZIP archive", &["zip"])
                .set_file_name(&naming::all_songs(input, state.song_format))
                .save_file()
        {
            let request = crate::audio_discovery::batch::BatchExportRequest::prepare(
                input, manifest, state.song_format, |id| Ok(export_options(state, manifest, id)));
            match request {
                Ok(request) => start_worker_with_result(state, input, "All songs", move |cancel, progress| {
                    request.write_new(&path, cancel, progress).map(|summary| Some(summary.message()))
                }),
                Err(error) => state.status = Some(format!("Cannot export all songs: {error:#}")),
            }
        }
    });
    if state.song_format.is_gsf() && mp2k_index.is_none() {
        ui.small("Runs the original GBA sound driver. Your player applies the requested duration and fade tags.");
        ui.add_enabled_ui(!state.is_busy(), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Duration (seconds)");
                ui.add(
                    egui::DragValue::new(&mut state.native_options.max_seconds)
                        .range(1..=MAX_DURATION_SECONDS),
                );
                ui.label("Fade seconds");
                let maximum = state
                    .native_options
                    .max_seconds
                    .min(u16::from(MAX_FADE_SECONDS)) as u8;
                state.native_options.fade_seconds = state.native_options.fade_seconds.min(maximum);
                ui.add(
                    egui::DragValue::new(&mut state.native_options.fade_seconds).range(0..=maximum),
                );
            });
        });
    } else if matches!(song, Some(SongRef::Cdda(_))) {
        ui.small("CD audio starts at index 1; pregaps are omitted. WAV and FLAC preserve the original 44.1 kHz stereo PCM. Ogg Vorbis is lossy.");
    } else if matches!(song, Some(SongRef::Natsume(_)))
        && matches!(state.song_format, SongFormat::Audio(_))
    {
        ui.small("Records the original sound driver for the chosen duration. WAV and FLAC preserve the rendered PCM; Ogg Vorbis is lossy.");
        draw_native_options(ui, state);
    } else if song.is_some_and(crate::audio_discovery::pcm::song::PcmSong::can_play)
        && matches!(state.song_format, SongFormat::Audio(_))
    {
        ui.small("Records the same player used by preview for the chosen duration. WAV and FLAC preserve its PCM; Ogg Vorbis is lossy.");
        draw_native_options(ui, state);
    } else if matches!(song, Some(SongRef::Gb(_) | SongRef::Nes(_)))
        && state.song_format == SongFormat::Midi
    {
        ui.small("Approximate note sequence with General MIDI instruments. Hardware waveforms, envelopes and modulation are not reproduced.");
        ui.horizontal_wrapped(|ui| {
            ui.label("Total passes");
            ui.add(egui::DragValue::new(&mut state.options.loops).range(1..=MAX_LOOP_PASSES));
            ui.label("Maximum seconds");
            ui.add(
                egui::DragValue::new(&mut state.options.max_seconds)
                    .range(1..=MAX_DURATION_SECONDS),
            );
        });
    } else {
        ui.small(state.song_format.info().description);
    }
    if mp2k_index.is_some() {
        if !gsf_supported {
            ui.small("GSF export is unavailable for this driver.");
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Sample format");
            egui::ComboBox::from_id_salt("audio-sample-format")
                .selected_text(state.sample_format.label())
                .show_ui(ui, |ui| {
                    for format in AudioFormat::ALL
                        .into_iter()
                        .filter(|format| format.available())
                    {
                        ui.selectable_value(&mut state.sample_format, format, format.label());
                    }
                });
            if ui
                .add_enabled(
                    selected_sample.is_some() && !state.is_busy(),
                    egui::Button::new("Export sample…"),
                )
                .clicked()
                && let Some(path) = crate::platform::FileDialog::new()
                    .set_title(&format!("Export {} sample", state.sample_format.label()))
                    .add_filter(
                        state.sample_format.extension(),
                        &[state.sample_format.extension()],
                    )
                    .set_file_name(&naming::sample(
                        input,
                        &selected_sample.unwrap(),
                        state.sample_format,
                    ))
                    .save_file()
            {
                let sample = selected_sample.expect("button is disabled without a sample");
                match AudioExportRequest::prepare(
                    input,
                    manifest,
                    mp2k_index.expect("button is disabled without an MP2k song"),
                    ExportKind::Sample {
                        format: state.sample_format,
                        sample,
                    },
                ) {
                    Ok(request) => start_worker(state, input, "Sample", move |cancel, progress| {
                        request.write_new(&path, cancel, progress)
                    }),
                    Err(error) => state.status = Some(format!("Cannot export: {error:#}")),
                }
            }
        });
        draw_song_options(ui, state);
        if state.song_format.is_gsf() {
            ui.small("GSF uses the cartridge's driver and sounds. Length and fade are player tags estimated from the sequence. Playback starts with fresh driver state.");
        } else {
            ui.small(format!("Rendered audio is an approximate stereo mix at {} Hz: {} loops, up to {} seconds, {}-second fade. Unsupported custom sounds cannot be rendered.", state.options.sample_rate, state.options.loops, state.options.max_seconds, state.options.fade_seconds));
        }
    }
    if let Some(pending) = &state.pending {
        ui.horizontal_wrapped(|ui| {
            ui.spinner();
            ui.label(format!(
                "{} · {}%",
                pending.label,
                pending.progress.load(Ordering::Relaxed)
            ));
            if ui.button("Cancel export").clicked() {
                pending.cancel.store(true, Ordering::Relaxed);
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        });
    }
    if selected.is_none() {
        ui.small("Select a mapped structure before extracting raw bytes.");
    }
    if let Some(status) = &state.status {
        ui.small(status);
    }
}

fn export_options(
    state: &ExportState,
    manifest: &ScanManifest,
    selection: SongId,
) -> RenderOptions {
    let uses_native_player = matches!(selection, SongId::Natsume(_))
        || manifest
            .scan
            .song(selection)
            .is_some_and(crate::audio_discovery::pcm::song::PcmSong::can_play);
    choose_export_options(state, uses_native_player)
}

fn choose_export_options(state: &ExportState, uses_native_player: bool) -> RenderOptions {
    if (matches!(state.song_format, SongFormat::Audio(_)) || state.song_format.is_gsf())
        && uses_native_player
    {
        state.native_options
    } else {
        state.options
    }
}

fn selected_sample(
    song: &crate::audio_discovery::SongCandidate,
    selected: &SelectedSpan,
) -> Option<SampleInventory> {
    resolve_selected_sample(selected, samples(song))
}

fn resolve_selected_sample<'a>(
    selected: &SelectedSpan,
    mut candidates: impl Iterator<Item = &'a SampleInventory>,
) -> Option<SampleInventory> {
    if let Some(sample) = selected.sample {
        return candidates.find(|candidate| **candidate == sample).copied();
    }
    let mut matching = candidates.filter(|candidate| {
        crate::audio_discovery::SourceSpan::from(candidate.data) == selected.span
    });
    let sample = *matching.next()?;
    matching.next().is_none().then_some(sample)
}

fn start_selection_export(
    state: &mut ExportState,
    input: &Arc<ScanInput>,
    manifest: &ScanManifest,
    selected: &SelectedSpan,
    path: std::path::PathBuf,
) {
    if state.is_busy() {
        return;
    }
    let request = match ExtractionRequest::prepare(input, manifest, selected.span, &selected.label)
    {
        Ok(request) => request,
        Err(error) => {
            state.status = Some(format!("Raw selection cannot be exported: {error:#}"));
            return;
        }
    };
    start_worker(state, input, "Raw selection", move |cancel, _| {
        request.write_new_cancellable(&path, cancel)
    });
}

fn start_worker(
    state: &mut ExportState,
    input: &Arc<ScanInput>,
    label: &str,
    work: impl FnOnce(&AtomicBool, &AtomicU32) -> anyhow::Result<()> + Send + 'static,
) {
    start_worker_with_result(state, input, label, move |cancel, progress| {
        work(cancel, progress).map(|()| None)
    });
}

fn start_worker_with_result(
    state: &mut ExportState,
    input: &Arc<ScanInput>,
    label: &str,
    work: impl FnOnce(&AtomicBool, &AtomicU32) -> anyhow::Result<Option<String>> + Send + 'static,
) {
    if state.is_busy() {
        return;
    }
    let source = Arc::clone(input);
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(AtomicU32::new(0));
    let worker_cancel = Arc::clone(&cancel);
    let worker_progress = Arc::clone(&progress);
    let (sender, receiver) = mpsc::sync_channel(1);
    match std::thread::Builder::new()
        .name("zeff-audio-export".to_owned())
        .spawn(move || {
            let _ = sender.send(work(&worker_cancel, &worker_progress));
        }) {
        Ok(_) => {
            state.pending = Some(PendingExport {
                source,
                receiver,
                cancel,
                progress,
                label: label.to_owned(),
            });
            state.status = None;
        }
        Err(error) => state.status = Some(format!("Couldn't start audio export: {error}")),
    }
}

#[cfg(test)]
impl ExportState {
    pub(super) fn is_busy_for_test(&self) -> bool {
        self.is_busy()
    }

    pub(super) fn status_for_test(&self) -> Option<&str> {
        self.status.as_deref()
    }
}

#[cfg(test)]
mod tests;
