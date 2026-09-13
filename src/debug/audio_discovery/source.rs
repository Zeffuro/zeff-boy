use std::path::Path;
use std::sync::Arc;

use crate::audio_discovery::media::ScanInput;

use super::AudioDiscoveryState;

#[derive(Default)]
pub(super) struct SourceState {
    pub(super) game: Option<Arc<ScanInput>>,
    pub(super) standalone: Option<Arc<ScanInput>>,
    error: Option<String>,
}

impl SourceState {
    pub(super) fn selected(&self) -> Option<Arc<ScanInput>> {
        self.standalone.as_ref().or(self.game.as_ref()).cloned()
    }
}

impl AudioDiscoveryState {
    fn open_audio_file(&mut self, path: &Path) -> anyhow::Result<()> {
        let source = crate::audio_discovery::media::import::load_audio(path)?;
        self.source.standalone = Some(Arc::new(source));
        self.source.error = None;
        self.refresh_source();
        self.session.start();
        Ok(())
    }

    fn use_game_source(&mut self) {
        self.source.standalone = None;
        self.source.error = None;
        self.refresh_source();
    }
}

pub(super) fn draw(ui: &mut egui::Ui, state: &mut AudioDiscoveryState) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("Open audio file…").clicked()
            && let Some(path) = crate::platform::FileDialog::new()
                .set_title("Open Audio File")
                .add_filter("Tracker modules", &["xm", "mod", "s3m", "it"])
                .add_filter("VGM register logs", &["vgm", "vgz"])
                .add_filter("GBS and NSF music rips", &["gbs", "nsf"])
                .pick_file()
            && let Err(error) = state.open_audio_file(&path)
        {
            state.source.error = Some(format!("Cannot open audio file: {error:#}"));
        }
        if state.source.standalone.is_some() {
            let label = if state.source.game.is_some() {
                "Use loaded game"
            } else {
                "Close audio file"
            };
            if ui.button(label).clicked() {
                state.use_game_source();
            }
        }
        if ui.button("Supported audio").clicked() {
            state.show_support = true;
        }
    });
    if let Some(error) = &state.source.error {
        ui.colored_label(egui::Color32::LIGHT_RED, error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_discovery::test_support::tracker::{it_fixture, s3m_fixture};
    use crate::audio_discovery::{ScanStatus, test_support::gba_fixture};
    use zeff_emu_common::system::System;

    fn game() -> Arc<ScanInput> {
        Arc::new(ScanInput {
            cdda: None,
            system: Some(System::Gba),
            standalone_audio: None,
            bytes: gba_fixture().into(),
            provenance: None,
            analysis_profile: "audio-source-test",
            display_name: Some("Game".to_owned()),
        })
    }

    fn finish_scan(state: &mut AudioDiscoveryState) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while state.session.is_busy() {
            assert!(std::time::Instant::now() < deadline);
            state.refresh_source();
            std::thread::yield_now();
        }
    }

    #[test]
    fn opening_audio_keeps_the_game_source_and_switches_back_to_the_latest_game()
    -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("fixture.it");
        let bytes = it_fixture();
        std::fs::write(&path, &bytes)?;
        let mut state = AudioDiscoveryState::default();
        let original_game = game();
        state.bind_source(Some(Arc::clone(&original_game)));
        state.open_audio_file(&path)?;
        let opened = state.session.source.clone().unwrap();
        assert_eq!(&*opened.bytes, bytes);
        assert!(Arc::ptr_eq(
            state.source.game.as_ref().unwrap(),
            &original_game
        ));
        std::fs::write(&path, b"changed on disk after opening")?;
        let replacement_game = game();
        state.bind_source(Some(Arc::clone(&replacement_game)));
        assert!(Arc::ptr_eq(state.session.source.as_ref().unwrap(), &opened));
        finish_scan(&mut state);
        let manifest = state.session.manifest.as_ref().unwrap();
        assert_eq!(manifest.scan.status, ScanStatus::Complete);
        assert_eq!(manifest.scan.tracker_modules.len(), 1);
        assert_eq!(
            manifest.source.as_ref().unwrap().sha256,
            zeff_firmware::sha256_hex(&bytes)
        );
        state.use_game_source();
        assert!(state.session.manifest.is_none());
        assert!(Arc::ptr_eq(
            state.session.source.as_ref().unwrap(),
            &replacement_game
        ));
        assert_eq!(&*original_game.bytes, gba_fixture());
        Ok(())
    }

    #[test]
    fn audio_files_work_without_a_game_and_failed_imports_preserve_the_selection()
    -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("fixture.s3m");
        std::fs::write(&path, s3m_fixture())?;
        let mut state = AudioDiscoveryState::default();
        state.open_audio_file(&path)?;
        state.bind_source(None);
        let source = state.session.source.clone().unwrap();
        assert!(
            state
                .open_audio_file(&directory.path().join("missing.it"))
                .is_err()
        );
        assert!(
            state
                .open_audio_file(&directory.path().join("unknown.txt"))
                .is_err()
        );
        assert!(Arc::ptr_eq(state.session.source.as_ref().unwrap(), &source));
        finish_scan(&mut state);
        for width in [420.0, 1000.0] {
            let output = egui::Context::default().run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 720.0),
                    )),
                    ..Default::default()
                },
                |ui| super::super::draw_audio_explorer(ui, &mut state),
            );
            assert!(!output.shapes.is_empty());
        }
        state.use_game_source();
        assert!(state.session.source.is_none());
        assert!(state.session.manifest.is_none());
        Ok(())
    }

    #[test]
    fn vgz_source_preserves_game_and_layout_uses_original_compressed_bytes() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("fixture.vgz");
        let bytes = crate::audio_discovery::vgm_export::tests::fixture(true);
        std::fs::write(&path, &bytes)?;
        let mut state = AudioDiscoveryState::default();
        let game = game();
        state.bind_source(Some(Arc::clone(&game)));
        state.open_audio_file(&path)?;
        finish_scan(&mut state);
        let manifest = state.session.manifest.as_ref().unwrap();
        assert_eq!(manifest.scan.status, ScanStatus::Complete);
        assert_eq!(manifest.scan.vgm_logs.len(), 1);
        assert_eq!(state.session.source.as_ref().unwrap().bytes.as_ref(), bytes);
        for width in [420.0, 1000.0] {
            let output = egui::Context::default().run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 720.0),
                    )),
                    ..Default::default()
                },
                |ui| super::super::draw_audio_explorer(ui, &mut state),
            );
            assert!(!output.shapes.is_empty());
        }
        assert_eq!(
            state.workspace.selected_candidate,
            Some(crate::audio_discovery::catalog::SongId::Vgm(0))
        );
        state.use_game_source();
        assert!(Arc::ptr_eq(state.session.source.as_ref().unwrap(), &game));
        assert!(state.session.manifest.is_none());
        Ok(())
    }

    #[test]
    fn imported_rips_use_one_catalog_entry_and_physical_source_ranges() -> anyhow::Result<()> {
        use crate::audio_discovery::{
            catalog::SongId, rips::RipFormat, test_support::rips::fixture,
        };
        let directory = tempfile::tempdir()?;
        for format in [RipFormat::Gbs, RipFormat::Nsf] {
            let bytes = fixture(format);
            let path = directory
                .path()
                .join(format!("fixture.{}", format.extension()));
            std::fs::write(&path, &bytes)?;
            let mut state = AudioDiscoveryState::default();
            state.open_audio_file(&path)?;
            finish_scan(&mut state);
            let report = &state.session.manifest.as_ref().unwrap().scan;
            assert_eq!(report.song_count(), 1);
            assert_eq!(report.music_rips[0].song_count, 3);
            for width in [420.0, 1000.0] {
                let output = egui::Context::default().run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 720.0),
                        )),
                        ..Default::default()
                    },
                    |ui| super::super::draw_audio_explorer(ui, &mut state),
                );
                assert!(!output.shapes.is_empty());
            }
            assert_eq!(state.workspace.selected_candidate, Some(SongId::Rip(0)));
            let selected = state.workspace.selected_span.as_ref().unwrap();
            assert_eq!(selected.span.canonical_cpu_address, None);
            assert_eq!(selected.span.byte_len as usize, bytes.len());
        }
        Ok(())
    }
}
