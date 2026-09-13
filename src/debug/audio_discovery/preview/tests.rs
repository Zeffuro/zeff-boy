use super::*;

#[test]
fn solo_and_mute_combine_and_selection_resets_the_mix() {
    assert_eq!(track_mask(0, 0), u16::MAX);
    assert_eq!(track_mask(1, 0), u16::MAX - 1);
    assert_eq!(track_mask(1, 3), 2);
    assert_eq!(track_mask(0, 3), 3);
    let mut state = PreviewState::default();
    state.select(Some(SongId::Mp2k(0)));
    state.muted = 3;
    state.solo = 2;
    state.select(Some(SongId::Mp2k(0)));
    assert_eq!((state.muted, state.solo), (3, 2));
    state.select(Some(SongId::Mp2k(1)));
    assert_eq!((state.muted, state.solo), (0, 0));
}

#[test]
fn preview_controls_layout_without_opening_an_audio_device() {
    let bytes = crate::audio_discovery::test_support::gba_fixture();
    let source = Arc::new(ScanInput {
        cdda: None,
        system: Some(zeff_emu_common::system::System::Gba),
        standalone_audio: None,
        bytes: bytes.into(),
        provenance: None,
        analysis_profile: "preview-layout",
        display_name: None,
    });
    let manifest = source.analyze(
        crate::audio_discovery::ScanLimits::default(),
        &std::sync::atomic::AtomicBool::new(false),
    );
    let context = egui::Context::default();
    for width in [420.0, 1000.0] {
        let mut state = PreviewState::default();
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 700.0),
                )),
                ..Default::default()
            },
            |ui| draw(ui, &mut state, &source, &manifest, Some(SongId::Mp2k(0))),
        );
        let text = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(text.iter().any(|text| text.contains("Play preview")));
        assert!(text.iter().any(|text| text.contains("approximate")));
        assert!(!state.player.is_pending());
    }
}

#[test]
fn native_driver_preview_exposes_transport_without_inapplicable_track_controls() {
    use crate::audio_discovery::natsume::{NatsumeSong, NatsumeSongKind};
    let bytes = crate::audio_discovery::test_support::gba_fixture();
    let source = Arc::new(ScanInput {
        cdda: None,
        system: Some(zeff_emu_common::system::System::Gba),
        standalone_audio: None,
        bytes: bytes.into(),
        provenance: None,
        analysis_profile: "native-preview-layout",
        display_name: None,
    });
    let mut manifest = source.analyze(
        crate::audio_discovery::ScanLimits::default(),
        &std::sync::atomic::AtomicBool::new(false),
    );
    let header = manifest.scan.candidates[0].header;
    manifest.scan.natsume_songs.push(NatsumeSong {
        profile: "synthetic-layout-only",
        index: 2,
        title: "Driver song".into(),
        kind: NatsumeSongKind::Music,
        table_entry: header,
        header,
        channel_mask: 1,
        priority: 0,
        channels: Vec::new(),
        mapped_spans: Vec::new(),
        warnings: Vec::new(),
    });
    for width in [420.0, 1000.0] {
        let context = egui::Context::default();
        let mut state = PreviewState::default();
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 480.0),
                )),
                ..Default::default()
            },
            |ui| draw(ui, &mut state, &source, &manifest, Some(SongId::Natsume(0))),
        );
        let text = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(text.contains(&"Play preview"));
        assert!(
            text.iter()
                .any(|text| text.contains("original sound driver"))
        );
        assert!(!text.contains(&"Tracks"));
        assert!(!text.contains(&"Total passes"));
        assert!(!text.iter().any(|text| text.contains("unavailable")));
        assert!(!state.player.is_pending());
    }
    let request = PreviewRequest::prepare_song(
        &source,
        &manifest,
        SongId::Natsume(0),
        RenderOptions::default(),
    )
    .unwrap();
    let mut player = PreviewPlayer::default();
    let _capture = player.start_captured(request);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while player.is_pending() {
        assert!(std::time::Instant::now() < deadline);
        player.poll();
        std::thread::yield_now();
    }
    assert!(
        player.error.is_some(),
        "an unrecognized driver must fail before execution"
    );
}

#[test]
fn cd_audio_preview_exposes_track_transport_without_synthesis_settings() -> anyhow::Result<()> {
    let (source, manifest, _) = crate::audio_discovery::test_support::cdda_fixture()?;
    for width in [420.0, 1000.0] {
        let context = egui::Context::default();
        let mut state = PreviewState::default();
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 600.0),
                )),
                ..Default::default()
            },
            |ui| draw(ui, &mut state, &source, &manifest, Some(SongId::Cdda(0))),
        );
        let text = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(text.contains(&"CD audio preview"));
        assert!(text.contains(&"Play preview"));
        assert!(!text.contains(&"Tracks"));
        assert!(!text.contains(&"Total passes"));
        assert!(!text.contains(&"Preview limit (seconds)"));
        assert!(!state.player.is_pending());
    }
    Ok(())
}
