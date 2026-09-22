use crate::audio_discovery::{DetectorState, MalformedInput, ScanStatus, ScanStop, Warning};

#[cfg(not(target_arch = "wasm32"))]
mod export;
#[cfg(not(target_arch = "wasm32"))]
mod preview;
#[cfg(not(target_arch = "wasm32"))]
mod source;
#[cfg(not(target_arch = "wasm32"))]
mod support;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
#[cfg(not(target_arch = "wasm32"))]
mod workspace;

#[derive(Default)]
pub(crate) struct AudioDiscoveryState {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) session: crate::audio_discovery::session::DiscoverySession,
    #[cfg(not(target_arch = "wasm32"))]
    workspace: workspace::AudioWorkspace,
    #[cfg(not(target_arch = "wasm32"))]
    export: export::ExportState,
    #[cfg(not(target_arch = "wasm32"))]
    preview: preview::PreviewState,
    #[cfg(not(target_arch = "wasm32"))]
    source: source::SourceState,
    #[cfg(not(target_arch = "wasm32"))]
    show_support: bool,
    #[cfg(not(target_arch = "wasm32"))]
    show_export: bool,
    #[cfg(not(target_arch = "wasm32"))]
    show_provenance: bool,
    #[cfg(not(target_arch = "wasm32"))]
    role_error: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
impl AudioDiscoveryState {
    pub(crate) fn bind_source(
        &mut self,
        source: Option<std::sync::Arc<crate::audio_discovery::media::ScanInput>>,
    ) {
        self.source.game = source;
        self.refresh_source();
    }

    fn refresh_source(&mut self) {
        let source = self.source.selected();
        if self.session.bind_source(source) {
            self.workspace = Default::default();
            self.export.clear_status();
            self.preview.clear();
            self.role_error = None;
        }
        self.session.poll();
        self.export.poll(self.session.source.as_ref());
        self.preview.player.poll();
    }

    pub(crate) fn stop_preview(&mut self) {
        self.preview.clear();
    }

    #[cfg(test)]
    pub(crate) fn preview_player(&mut self) -> &mut crate::audio_discovery::preview::PreviewPlayer {
        &mut self.preview.player
    }
}

pub(crate) fn draw_audio_explorer(ui: &mut egui::Ui, state: &mut AudioDiscoveryState) {
    ui.heading("Audio Explorer");
    #[cfg(target_arch = "wasm32")]
    {
        let _ = state;
        ui.label("ROM audio discovery is available in the native app.");
    }
    #[cfg(not(target_arch = "wasm32"))]
    draw_native(ui, state);
}

#[cfg(not(target_arch = "wasm32"))]
fn draw_native(ui: &mut egui::Ui, state: &mut AudioDiscoveryState) {
    source::draw(ui, state);
    support::draw(ui.ctx(), &mut state.show_support);
    state.session.poll();
    state.export.poll(state.session.source.as_ref());
    if state.session.source.is_none() {
        ui.label("Load a game or open an audio file to inspect its music.");
        return;
    }
    ui.horizontal_wrapped(|ui| {
        let name = state
            .session
            .source
            .as_ref()
            .and_then(|input| input.display_name.as_deref())
            .unwrap_or("Loaded audio source");
        ui.small(name);
        if ui
            .add_enabled(state.session.can_start(), egui::Button::new("Scan audio"))
            .clicked()
        {
            state.session.start();
            state.workspace = Default::default();
            state.export.clear_status();
            state.preview.clear();
        }
        if state.session.is_busy() {
            if ui.button("Cancel").clicked() {
                state.session.cancel();
            }
            ui.spinner();
            ui.label(if state.session.is_queued() {
                "New scan queued"
            } else if state.session.cancelled {
                "Cancelling…"
            } else {
                "Scanning…"
            });
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        } else if state.session.cancelled {
            ui.label("Scan cancelled");
        }
        draw_options(ui, state);
        if state.session.manifest.is_some() {
            ui.menu_button("More", |ui| {
                if ui.button("Export audio…").clicked() {
                    state.show_export = true;
                    ui.close();
                }
                if ui.button("Source and provenance…").clicked() {
                    state.show_provenance = true;
                    ui.close();
                }
            });
        }
    });

    if let Some(error) = &state.session.error {
        ui.colored_label(egui::Color32::LIGHT_RED, error);
    }
    if let Some(manifest) = &state.session.manifest {
        ui.horizontal_wrapped(|ui| {
            ui.strong(status_text(manifest.scan.status));
            ui.label(format!("{} audio entries", manifest.scan.song_count()));
        });
        let retry_limit = match manifest.scan.status {
            ScanStatus::Incomplete(ScanStop::CandidateLimit)
                if manifest.scan.limits.max_candidates < crate::audio_discovery::MAX_CANDIDATES =>
            {
                Some(
                    manifest
                        .scan
                        .limits
                        .max_candidates
                        .saturating_mul(2)
                        .clamp(1024, crate::audio_discovery::MAX_CANDIDATES),
                )
            }
            _ => None,
        };
        if let Some(limit) = retry_limit {
            ui.label("More candidates may exist beyond this partial result.");
            if ui
                .button(format!("Rescan with a {limit}-candidate limit"))
                .clicked()
            {
                state.session.limits.max_candidates = limit;
                state.session.start();
                state.workspace = Default::default();
                state.export.clear_status();
                state.preview.clear();
            }
        }
    }
    let Some(manifest) = &mut state.session.manifest else {
        return;
    };
    if !manifest.classifications_loaded {
        state.role_error = manifest
            .load_roles()
            .err()
            .map(|error| format!("Audio roles: {error:#}"));
    }
    if let Some(error) = &state.role_error {
        ui.colored_label(egui::Color32::LIGHT_RED, error);
    }
    let source = state.session.source.as_ref().expect("loaded source");
    state.workspace.ensure_selection(&manifest.scan);

    preview::draw(
        ui,
        &mut state.preview,
        source,
        manifest,
        state.workspace.selected_candidate,
    );
    ui.separator();
    if manifest.scan.song_count() == 0 {
        ui.label(if !manifest.scan.song_tables.is_empty() {
            "Recognized an MP2k song table. No sequence candidates were retained; inspect its entries below."
        } else if !manifest.scan.driver_candidates.is_empty() {
            "Found possible sound drivers. No playable songs were identified; inspect the matches below."
        } else if manifest.scan.status == ScanStatus::Complete {
            "No supported audio format found. The game may use another sound engine."
        } else if manifest.scan.status == ScanStatus::Unsupported {
            "This source does not match a supported audio format profile."
        } else if matches!(manifest.scan.status, ScanStatus::Malformed(_)) {
            "The audio file has an invalid header or program range. No audio entries were retained."
        } else {
            "No candidates retained in this partial scan."
        });
    }
    if manifest.scan.song_count() != 0
        || !manifest.scan.song_tables.is_empty()
        || !manifest.scan.driver_candidates.is_empty()
    {
        workspace::draw(
            ui,
            &mut state.workspace,
            &manifest.scan,
            &source.bytes,
            &manifest.classifications,
            |selection| {
                crate::audio_discovery::preview::PreviewRequest::can_preview(manifest, selection)
            },
        );
    }
    if let Some((selection, role)) = state.workspace.take_role_request() {
        state.role_error = manifest
            .save_role(selection, role)
            .err()
            .map(|error| format!("Could not save audio role: {error:#}"));
    }
    if let Some(selection) = state.workspace.take_preview_request() {
        preview::start_requested(&mut state.preview, source, manifest, selection);
    }
    if state.workspace.take_export_request().is_some() {
        state.show_export = true;
    }
    draw_export_window(
        ui.ctx(),
        &mut state.workspace,
        &mut state.export,
        &mut state.show_export,
        source,
        manifest,
    );
    egui::Window::new("Source and provenance")
        .open(&mut state.show_provenance)
        .resizable(true)
        .default_width(520.0)
        .show(ui.ctx(), |ui| draw_provenance(ui, manifest));
}

#[cfg(not(target_arch = "wasm32"))]
fn draw_export_window(
    ctx: &egui::Context,
    workspace: &mut workspace::AudioWorkspace,
    export_state: &mut export::ExportState,
    show_export: &mut bool,
    source: &std::sync::Arc<crate::audio_discovery::media::ScanInput>,
    manifest: &crate::audio_discovery::media::ScanManifest,
) {
    egui::Window::new("Audio export")
        .open(show_export)
        .resizable(true)
        .default_width(480.0)
        .show(ctx, |ui| {
            export::draw(
                ui,
                export_state,
                source,
                manifest,
                workspace.selected_candidate,
                workspace.selected_span.as_ref(),
            );
        });
}

#[cfg(not(target_arch = "wasm32"))]
fn draw_options(ui: &mut egui::Ui, state: &mut AudioDiscoveryState) {
    ui.menu_button("Options", |ui| {
        ui.label("Scan settings");
        ui.add_enabled_ui(!state.session.is_busy(), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Candidates");
                ui.add(
                    egui::DragValue::new(&mut state.session.limits.max_candidates)
                        .range(1..=crate::audio_discovery::MAX_CANDIDATES),
                );
                ui.label("Work budget");
                ui.add(
                    egui::DragValue::new(&mut state.session.limits.max_work)
                        .range(1..=crate::audio_discovery::MAX_SCAN_WORK)
                        .speed(100_000),
                );
            });
        });
        ui.small(
            "Limits bound analysis and memory use. Reaching a limit keeps a partial inventory.",
        );
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn draw_provenance(ui: &mut egui::Ui, manifest: &crate::audio_discovery::media::ScanManifest) {
    ui.label(format!(
        "Loaded media: {}",
        byte_count(manifest.scan.media.byte_len)
    ));
    if let Some(identity) = &manifest.source {
        if let Some(member) = &identity.selected_member {
            ui.label(format!("Archive member: {}", member.name));
        } else {
            ui.label(source_description(identity.kind));
        }
    } else if let Some(disc) = &manifest.disc {
        ui.label(format!(
            "{} · {}",
            disc_source_description(disc.provenance.source_kind),
            byte_count(disc.provenance.source_media_len as u64)
        ));
        if disc.provenance.transforms_applied {
            ui.label("Disc mods were applied at load time.");
        }
    } else {
        ui.label("Loaded directly from an in-memory source.");
    }
    if let Some(transforms) = &manifest.transforms
        && !transforms.is_empty()
    {
        ui.label(format!(
            "{} load-time mod attempts recorded",
            transforms.len()
        ));
    }
    ui.small("Full source identity is preserved in reports and exports.");
    egui::CollapsingHeader::new("Technical diagnostics").show(ui, |ui| {
        if let Some(hash) = &manifest.scan.media.sha256 {
            hash_row(ui, "Scanned SHA-256", hash);
        }
        if let Some(identity) = &manifest.source {
            ui.small(format!("Source kind: {}", identity.kind));
            hash_row(ui, "Original SHA-256", &identity.sha256);
            if let Some(container) = &identity.container {
                hash_row(ui, "Archive SHA-256", &container.sha256);
            }
        } else if let Some(disc) = &manifest.disc {
            ui.small(format!("Source kind: {}", disc.provenance.source_kind));
            hash_row(ui, "Original disc SHA-256", &disc.original_disc_sha256);
            hash_row(
                ui,
                "Source media SHA-256",
                &disc.provenance.source_media_sha256,
            );
        }
        if let Some(transforms) = &manifest.transforms {
            for step in transforms {
                ui.label(format!("{}: {:?}", step.filename, step.outcome));
            }
        }
        ui.small(format!(
            "Analysis profile {} · detector {} v{} · {} / {} work units",
            manifest.analysis_profile,
            manifest.scan.detector,
            manifest.scan.detector_version,
            manifest.scan.work_used,
            manifest.scan.limits.max_work
        ));
        for detector in manifest.scan.applicable_detectors {
            ui.small(format!(
                "{} v{}: {}",
                detector.id, detector.semantic_version, detector.scope
            ));
        }
        for outcome in &manifest.scan.detector_outcomes {
            ui.small(format!(
                "{}: {} · {} retained · {} work units",
                outcome.descriptor.id,
                detector_state_text(outcome.state),
                outcome.retained_matches,
                outcome.work_used
            ));
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn source_description(kind: &str) -> &'static str {
    match kind {
        "direct_gba_file" => "Opened from a Game Boy Advance cartridge file.",
        "direct_cartridge_file" => "Opened from a cartridge file.",
        "preloaded_gba_bytes" => "Loaded from the running Game Boy Advance snapshot.",
        "preloaded_cartridge_bytes" => "Loaded from the running cartridge snapshot.",
        "direct_xm_file" => "Opened from a standalone XM module.",
        "direct_mod_file" => "Opened from a standalone MOD module.",
        "direct_s3m_file" => "Opened from a standalone S3M module.",
        "direct_it_file" => "Opened from a standalone IT module.",
        "direct_vgm_file" => "Opened from a standalone VGM or VGZ register log.",
        "direct_gbs_file" => "Opened from a standalone GBS music rip.",
        "direct_nsf_file" => "Opened from a standalone NSF music rip.",
        _ => "Loaded from an identified media source.",
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn disc_source_description(kind: &str) -> &'static str {
    match kind {
        "loaded_disc" => "Loaded from the running disc snapshot",
        "cue" => "Opened from a CUE disc set",
        "zip_cue" => "Opened from a CUE disc set in a ZIP archive",
        _ => "Loaded from an identified disc source",
    }
}

fn byte_count(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} bytes")
    }
}

fn hash_row(ui: &mut egui::Ui, label: &str, hash: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(label);
        ui.monospace(format!("{}…", &hash[..hash.len().min(16)]));
        if ui.small_button("Copy").on_hover_text(hash).clicked() {
            ui.ctx().copy_text(hash.to_owned());
        }
    });
}

fn status_text(status: ScanStatus) -> String {
    match status {
        ScanStatus::Complete => "Scan complete".into(),
        ScanStatus::Unsupported => "Source format is unsupported".into(),
        ScanStatus::Malformed(reason) => format!("Invalid audio file: {}", malformed_text(reason)),
        ScanStatus::Incomplete(reason) => format!("Scan incomplete: {}", stop_text(reason)),
    }
}

fn stop_text(reason: ScanStop) -> &'static str {
    match reason {
        ScanStop::Cancelled => "cancelled",
        ScanStop::WorkLimit => "work limit reached",
        ScanStop::CandidateLimit => "candidate limit reached",
        ScanStop::MediaLimit => "source or decoded data exceeds the size limit",
        ScanStop::InvalidLimits => "invalid source selection or scan limits",
        ScanStop::InventoryLimit => "inventory limit reached",
        ScanStop::ValidationLimit => "validation limit reached",
    }
}

fn malformed_text(reason: MalformedInput) -> &'static str {
    match reason {
        MalformedInput::TruncatedChunk => "chunk extends beyond the file",
        MalformedInput::InvalidChunkSize => "invalid chunk size",
        MalformedInput::InvalidChunkOrder => "invalid chunk order",
        MalformedInput::MissingRequiredChunk => "required chunk is missing",
        MalformedInput::DuplicateChunk => "duplicate required chunk",
        MalformedInput::TruncatedHeader => "truncated header",
        MalformedInput::EmptyProgram => "missing program data",
        MalformedInput::InvalidSongCount => "invalid song count",
        MalformedInput::InvalidFirstSong => "starting song is outside the declared song count",
        MalformedInput::InvalidAddress => "address is outside the supported range",
        MalformedInput::ProgramLengthExceedsSource => "declared program extends beyond the file",
    }
}

fn detector_state_text(state: DetectorState) -> String {
    match state {
        DetectorState::Complete => "Complete".into(),
        DetectorState::Unsupported => "Unsupported format".into(),
        DetectorState::Malformed(reason) => format!("Invalid file: {}", malformed_text(reason)),
        DetectorState::Incomplete(reason) => format!("Incomplete: {}", stop_text(reason)),
        DetectorState::NotRun(reason) => format!("Not run: {}", stop_text(reason)),
    }
}

fn warning_text(warning: &Warning) -> String {
    match warning {
        Warning::UnsupportedCommand { offset, opcode } => {
            format!("Unsupported command {opcode:02X} at ROM {offset:08X}")
        }
        Warning::InvalidTrack { offset } => format!("Invalid track data at ROM {offset:08X}"),
        Warning::TrackLimit { offset } => format!("Track validation stopped at ROM {offset:08X}"),
        Warning::UnsupportedInstrument { voice, kind } => {
            format!("Voice {voice}: unsupported instrument type {kind:02X}")
        }
        Warning::InvalidInstrument { voice } => {
            format!("Voice {voice}: invalid or missing instrument/sample data")
        }
        Warning::EmptySample { voice, offset } => format!(
            "Voice {voice}: sample header at ROM {offset:08X} declares no PCM bytes; custom synthesis is not decoded"
        ),
        Warning::NoVoiceSelection => "Track has no explicit voice selection".into(),
        Warning::NoExplicitNoteData => "Explicit note key/velocity data was not established".into(),
        Warning::UnsupportedTrackCount { count } => format!("Unresolved track count: {count}"),
        Warning::UnresolvedInstrumentKeys { voice } => {
            format!("Voice {voice}: no note keys established for this instrument")
        }
        Warning::InvalidInstrumentRegion { voice, key, offset } => format!(
            "Voice {voice}, key {key}: invalid or missing region/sample at ROM {offset:08X}"
        ),
        Warning::UnsupportedInstrumentRegion {
            voice,
            key,
            kind,
            offset,
        } => format!(
            "Voice {voice}, key {key}: unsupported region type {kind:02X} at ROM {offset:08X}"
        ),
    }
}
