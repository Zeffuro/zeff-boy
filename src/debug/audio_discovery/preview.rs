use std::sync::Arc;

use crate::audio_discovery::{
    catalog::SongId,
    media::{ScanInput, ScanManifest},
    preview::{DEFAULT_VOLUME_PERCENT, MAX_PREVIEW_SECONDS, PreviewPlayer, PreviewRequest},
    render::{MAX_LOOP_PASSES, PlaybackGain, RenderOptions},
};

pub(super) struct PreviewState {
    pub(super) player: PreviewPlayer,
    selection: Option<SongId>,
    muted: u16,
    solo: u16,
    volume: u32,
    options: RenderOptions,
    native_limit: u16,
}

impl Default for PreviewState {
    fn default() -> Self {
        Self {
            player: PreviewPlayer::default(),
            selection: None,
            muted: 0,
            solo: 0,
            volume: DEFAULT_VOLUME_PERCENT,
            options: RenderOptions::default(),
            native_limit: crate::audio_discovery::natsume::preview::DEFAULT_DURATION_SECONDS,
        }
    }
}

impl PreviewState {
    pub(super) fn clear(&mut self) {
        self.player.stop();
        self.player.error = None;
        self.player.warnings.clear();
        self.selection = None;
        self.muted = 0;
        self.solo = 0;
        self.player.set_track_mask(u16::MAX);
    }

    fn select(&mut self, selection: Option<SongId>) {
        if self.selection != selection {
            self.clear();
            self.selection = selection;
        }
    }
}

pub(super) fn start_requested(
    state: &mut PreviewState,
    source: &Arc<ScanInput>,
    manifest: &ScanManifest,
    selection: SongId,
) {
    if !PreviewRequest::can_preview(manifest, selection) {
        state.player.error =
            Some("Preview is unavailable for this song's playback profile.".to_owned());
        return;
    }
    state.select(Some(selection));
    if state.player.snapshot().is_some() {
        state.player.seek(0);
        state.player.set_playing(true);
        return;
    }
    match PreviewRequest::prepare_song(source, manifest, selection, options_for(state, selection)) {
        Ok(request) => state.player.start(request),
        Err(error) => state.player.error = Some(format!("Cannot preview: {error:#}")),
    }
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    state: &mut PreviewState,
    source: &Arc<ScanInput>,
    manifest: &ScanManifest,
    selection: Option<SongId>,
) {
    state.select(selection);
    state.player.poll();
    let (selection, mp2k_tracks) = match selection {
        Some(selection @ SongId::Mp2k(index)) => {
            let Some(song) = manifest.scan.candidates.get(index) else {
                return;
            };
            (selection, Some(song.tracks.len()))
        }
        Some(selection) if manifest.scan.song(selection).is_some() => (selection, None),
        _ => {
            ui.small("Preview is unavailable for this song's current playback profile.");
            return;
        }
    };
    let can_preview = PreviewRequest::can_preview(manifest, selection);
    let is_cd = matches!(selection, SongId::Cdda(_));
    let requires_validation = manifest
        .scan
        .song(selection)
        .is_some_and(|song| song.requires_runtime_validation());
    let snapshot = state.player.snapshot();
    ui.horizontal_wrapped(|ui| {
        ui.strong(preview_label(
            manifest,
            selection,
            mp2k_tracks.is_some(),
            is_cd,
            snapshot.is_some(),
        ));
        let playing = snapshot.is_some_and(|snapshot| snapshot.playing);
        let button_width = ui
            .painter()
            .layout_no_wrap(
                if requires_validation {
                    "Verify & preview"
                } else {
                    "Pause preview"
                }
                .to_owned(),
                egui::TextStyle::Button.resolve(ui.style()),
                ui.visuals().text_color(),
            )
            .size()
            .x
            + ui.spacing().button_padding.x * 2.0;
        if ui
            .add_enabled(
                can_preview,
                egui::Button::new(if playing {
                    "Pause preview"
                } else if requires_validation && snapshot.is_none() {
                    "Verify & preview"
                } else {
                    "Play preview"
                })
                .min_size(egui::vec2(button_width, ui.spacing().interact_size.y)),
            )
            .clicked()
        {
            if snapshot.is_some() {
                state.player.set_playing(!playing);
            } else {
                match PreviewRequest::prepare_song(
                    source,
                    manifest,
                    selection,
                    options_for(state, selection),
                ) {
                    Ok(request) => state.player.start(request),
                    Err(error) => state.player.error = Some(format!("Cannot preview: {error:#}")),
                }
            }
        }
        if ui
            .add_enabled(snapshot.is_some(), egui::Button::new("Restart"))
            .clicked()
        {
            state.player.seek(0);
            state.player.set_playing(true);
        }
        if ui
            .add_enabled(state.player.is_pending(), egui::Button::new("Stop preview"))
            .clicked()
        {
            state.player.stop();
        }
        if ui
            .add(
                egui::Slider::new(&mut state.volume, 0..=100)
                    .text("Volume")
                    .suffix("%")
                    .max_decimals(0),
            )
            .changed()
        {
            state.player.set_volume(state.volume);
        }
    });
    draw_position(ui, state, selection, can_preview);
    if let Some(song_tracks) = mp2k_tracks {
        egui::CollapsingHeader::new("Tracks").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let tracks = snapshot.map_or(song_tracks, |state| state.tracks.max(song_tracks));
                for track in 0..tracks.min(16) {
                    let bit = 1u16 << track;
                    ui.group(|ui| {
                        ui.label((track + 1).to_string());
                        if ui
                            .selectable_label(state.muted & bit != 0, "M")
                            .on_hover_text("Mute this track")
                            .clicked()
                        {
                            state.muted ^= bit;
                        }
                        if ui
                            .selectable_label(state.solo & bit != 0, "S")
                            .on_hover_text("Solo this track")
                            .clicked()
                        {
                            state.solo ^= bit;
                        }
                    });
                }
                if ui.small_button("Reset tracks").clicked() {
                    state.muted = 0;
                    state.solo = 0;
                }
            });
        });
        state
            .player
            .set_track_mask(track_mask(state.muted, state.solo));
    } else {
        state.player.set_track_mask(u16::MAX);
    }
    egui::CollapsingHeader::new("Preview settings and limitations").show(ui, |ui| {
        ui.add_enabled_ui(!state.player.is_pending(), |ui| {
            ui.horizontal_wrapped(|ui| {
                if mp2k_tracks.is_some() {
                    ui.label("Total passes");
                    ui.add(egui::DragValue::new(&mut state.options.loops).range(1..=MAX_LOOP_PASSES));
                    ui.label("Limit (seconds)");
                    ui.add(egui::DragValue::new(&mut state.options.max_seconds).range(1..=MAX_PREVIEW_SECONDS));
                    egui::ComboBox::from_id_salt("audio-preview-gain")
                    .selected_text(state.options.playback_gain.label()).show_ui(ui, |ui| {
                        for gain in PlaybackGain::ALL { ui.selectable_value(&mut state.options.playback_gain, gain, gain.label()); }
                    });
                } else if !is_cd {
                    ui.label("Preview limit (seconds)");
                    ui.add(egui::DragValue::new(&mut state.native_limit).range(1..=MAX_PREVIEW_SECONDS));
                }
            });
        });
        ui.small(if mp2k_tracks.is_some() {
            "Uses the same approximate SoundFont synthesis as audio exports. MP2k reverb, modulation and dynamic Camelot effects are not reproduced. Seeking may take time to reconstruct active notes. Track controls can take one short audio buffer to apply."
        } else if is_cd {
            "Plays the CD track from index 1, without its pregap. Preview uses the output device's sample rate; WAV and FLAC exports preserve the original 44.1 kHz PCM."
        } else if matches!(selection, SongId::Vgm(_)) {
            "Plays the recorded PSG log once, stopping at its end or the preview limit. Seeking replays from the start. VGM timing and chip behavior can differ from the original capture."
        } else if matches!(selection, SongId::Huge(_)) {
            "Verifies original and isolated playback before returning audio. Stops at the verified capture endpoint or chosen maximum; seeking reads verified audio. Seamless loops and individual channel controls are unavailable."
        } else if matches!(selection, SongId::GbNative(_)) {
            "Plays the original Game Boy music. Playback stops at the qualified song end or first complete loop; seeking replays from the start."
        } else if matches!(selection, SongId::GbQuickThunder(_) | SongId::GbGhx(_) | SongId::GbSoundSystem(_) | SongId::GbCarillon(_)) {
            "Runs the original Game Boy sound driver under its reported hardware profile until the preview limit. Seeking replays from the start; automatic loop detection and individual channel controls are unavailable."
        } else if matches!(selection, SongId::GbTose(_)) {
            "Runs the original Game Boy sound driver with DMG timing until the preview limit. Seeking replays from the start; automatic loop detection and individual channel controls are unavailable."
        } else if matches!(selection, SongId::Gb(_) | SongId::GbMusyx(_)) {
            "Runs the original Game Boy Color sound driver at normal speed until the preview limit. Seeking replays from the start; automatic loop detection and individual channel controls are unavailable."
        } else if matches!(selection, SongId::NesNative(_) | SongId::Nes(_) | SongId::NesTose(_)) {
            "Runs the original NES driver with NTSC timing. Audio plays until the preview limit; seeking replays from the start. Automatic loop detection and individual channel controls are unavailable."
        } else if matches!(selection, SongId::SegaPsg(_)) {
            "Runs the original PSG driver with NTSC timing. Songs play until the preview limit; seeking replays from the start. Automatic loop detection and individual channel controls are unavailable."
        } else if matches!(selection, SongId::WsTose(_)) {
            "Runs the original WonderSwan driver under its reported hardware profile until the preview limit. Seeking replays from the start; automatic loop detection and individual channel controls are unavailable."
        } else {
            "Runs the original sound driver in a separate GBA emulator. Songs play until the preview limit; automatic loop detection and individual channel controls are unavailable. Seeking replays the driver from the start."
        });
        for warning in &state.player.warnings { ui.small(warning); }
    });
    if state.player.is_pending() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
    }
}

fn draw_position(
    ui: &mut egui::Ui,
    state: &mut PreviewState,
    selection: SongId,
    can_preview: bool,
) {
    let snapshot = state.player.snapshot();
    let rate = snapshot.map_or(1.0, |snapshot| f64::from(snapshot.sample_rate));
    let mut position = snapshot.map_or(0.0, |snapshot| snapshot.position as f64 / rate);
    let duration = snapshot.map_or(0.0, |snapshot| snapshot.duration as f64 / rate);
    let message = if state.player.error.is_some() {
        egui::RichText::new("Preview failed")
            .color(egui::Color32::LIGHT_RED)
            .into()
    } else if !can_preview {
        egui::WidgetText::from("Preview unavailable")
    } else {
        let end = if duration != 0.0 {
            time(duration)
        } else {
            "--:--".to_owned()
        };
        let suffix = if matches!(selection, SongId::Mp2k(_) | SongId::Cdda(_)) {
            ""
        } else {
            " preview limit"
        };
        egui::WidgetText::from(format!("{} / {end}{suffix}", time(position)))
    };
    // Keep the timeline in place while selection and asynchronous preview state change.
    ui.horizontal(|ui| {
        let label = ui.add(egui::Label::new(message).truncate());
        if let Some(error) = &state.player.error {
            label.on_hover_text(error);
        }
        if ui
            .add_enabled(
                duration != 0.0,
                egui::Slider::new(&mut position, 0.0..=duration.max(1.0))
                    .show_value(false)
                    .text("Position"),
            )
            .changed()
        {
            state.player.seek((position * rate) as usize);
        }
        let size = ui.spacing().interact_size.y;
        if snapshot.is_some_and(|snapshot| snapshot.preparing) {
            ui.add(egui::Spinner::new().size(size))
                .on_hover_text("Preparing preview position…");
        } else {
            ui.allocate_space(egui::vec2(size, size));
        }
    });
}

fn options_for(state: &PreviewState, selection: SongId) -> RenderOptions {
    if matches!(selection, SongId::Mp2k(_)) {
        state.options
    } else {
        RenderOptions {
            max_seconds: state.native_limit,
            ..RenderOptions::default()
        }
    }
}

fn preview_label(
    manifest: &ScanManifest,
    selection: SongId,
    is_mp2k: bool,
    is_cd: bool,
    ready: bool,
) -> String {
    if matches!(selection, SongId::Huge(_)) {
        if ready {
            "hUGEDriver preview · verified audio"
        } else {
            "hUGEDriver preview · verification required"
        }
        .to_owned()
    } else if is_mp2k {
        "MP2k preview · approximate".to_owned()
    } else if is_cd {
        "CD audio preview".to_owned()
    } else if matches!(selection, SongId::Vgm(_)) {
        "PSG register-log preview".to_owned()
    } else if matches!(selection, SongId::Natsume(_)) {
        "Natsume preview · original sound driver".to_owned()
    } else {
        let song = manifest
            .scan
            .song(selection)
            .expect("previewable song is retained");
        format!(
            "{} preview · {}",
            song.engine(),
            if crate::audio_discovery::pcm::song::PcmSong::is_native(song) {
                "original sound driver"
            } else {
                "approximate"
            }
        )
    }
}

fn track_mask(muted: u16, solo: u16) -> u16 {
    !muted & if solo == 0 { u16::MAX } else { solo }
}

fn time(seconds: f64) -> String {
    let seconds = seconds as usize;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests;
