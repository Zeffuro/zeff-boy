use super::*;
use zeff_emu_common::system::System;

fn input() -> Arc<ScanInput> {
    Arc::new(ScanInput {
        #[cfg(not(target_arch = "wasm32"))]
        cdda: None,
        system: Some(System::Gba),
        standalone_audio: None,
        bytes: vec![0; 0xC0].into(),
        provenance: None,
        analysis_profile: "debug-audio-export-test-v1",
        display_name: None,
    })
}

#[test]
fn native_audio_export_has_separate_duration_and_only_applicable_controls() {
    use crate::audio_discovery::natsume::{NatsumeSong, NatsumeSongKind};
    let input = input();
    let mut manifest = input.analyze(Default::default(), &AtomicBool::new(false));
    let span = crate::audio_discovery::RomSpan {
        effective_offset: 0x80,
        byte_len: 4,
        canonical_cpu_address: 0x0800_0080,
    };
    manifest.scan.natsume_songs.push(NatsumeSong {
        profile: "synthetic-layout-only",
        index: 2,
        title: "Driver song".into(),
        kind: NatsumeSongKind::Music,
        table_entry: span,
        header: span,
        channel_mask: 1,
        priority: 0,
        channels: Vec::new(),
        mapped_spans: vec![span],
        warnings: Vec::new(),
    });
    for width in [420.0, 1000.0] {
        let context = egui::Context::default();
        let mut state = ExportState::default();
        state.options.max_seconds = 42;
        state.options.loops = 8;
        state.options.playback_gain = PlaybackGain::Mp2kAmplitude;
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 700.0),
                )),
                ..Default::default()
            },
            |ui| {
                draw(
                    ui,
                    &mut state,
                    &input,
                    &manifest,
                    Some(SongId::Natsume(0)),
                    None,
                )
            },
        );
        let text = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(text.contains(&"Duration (seconds)"));
        assert!(text.contains(&"End fade (seconds)"));
        assert!(!text.contains(&"Loop passes"));
        assert!(!text.contains(&"Playback gain"));
        assert_eq!(state.native_options.max_seconds, 180);
        assert_eq!(state.native_options.loops, 1);
        assert_eq!(state.native_options.playback_gain, PlaybackGain::Raw);
        assert_eq!(state.options.max_seconds, 42);
        assert!(!state.is_busy());
    }
}

#[test]
fn native_song_midi_export_keeps_the_midi_render_settings() {
    let mut state = ExportState::default();
    state.options.max_seconds = 42;
    state.options.loops = 8;
    state.native_options.max_seconds = 180;
    state.song_format = SongFormat::Midi;

    let selected = choose_export_options(&state, true);
    assert_eq!(selected.max_seconds, state.options.max_seconds);
    assert_eq!(selected.loops, state.options.loops);
}

#[test]
fn native_gsf_uses_native_duration_without_changing_mp2k_timing() {
    let mut state = ExportState::default();
    state.options.max_seconds = 42;
    state.native_options.max_seconds = 180;
    for format in [SongFormat::Gsf, SongFormat::MiniGsfPack] {
        state.song_format = format;
        assert_eq!(choose_export_options(&state, true).max_seconds, 180);
        assert_eq!(choose_export_options(&state, false).max_seconds, 42);
    }
}

#[test]
fn completed_export_from_a_replaced_rom_cannot_publish_status() {
    let old_source = input();
    let new_source = input();
    let (sender, receiver) = mpsc::sync_channel(1);
    sender
        .send(Ok::<Option<String>, anyhow::Error>(None))
        .unwrap();
    let mut state = ExportState {
        status: Some("old status".to_owned()),
        pending: Some(PendingExport {
            source: old_source,
            receiver,
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(AtomicU32::new(0)),
            label: "Raw selection".to_owned(),
        }),
        ..Default::default()
    };

    state.clear_status();
    assert!(state.is_busy_for_test());
    state.poll(Some(&new_source));
    assert!(!state.is_busy_for_test());
    assert_eq!(state.status_for_test(), None);
}

#[test]
fn export_controls_wrap_in_a_narrow_dock_without_starting_a_worker() {
    let input = Arc::new(ScanInput {
        #[cfg(not(target_arch = "wasm32"))]
        cdda: None,
        system: Some(System::Gba),
        standalone_audio: None,
        bytes: crate::audio_discovery::test_support::gba_fixture().into(),
        provenance: None,
        analysis_profile: "export-layout-test",
        display_name: None,
    });
    let manifest = input.analyze(Default::default(), &AtomicBool::new(false));
    let selected = SelectedSpan {
        span: samples(&manifest.scan.candidates[0])
            .next()
            .unwrap()
            .data
            .into(),
        label: "PCM sample".to_owned(),
        sample: None,
    };
    for width in [420.0, 1_000.0] {
        let context = egui::Context::default();
        let mut state = ExportState::default();
        let mut output = None;
        for _ in 0..2 {
            output = Some(context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 600.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    draw(
                        ui,
                        &mut state,
                        &input,
                        &manifest,
                        Some(SongId::Mp2k(0)),
                        Some(&selected),
                    )
                },
            ));
        }
        let output = output.unwrap();
        let texts = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    Some(text)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for expected in [
            "Song format",
            "Export song…",
            "Sample format",
            "Export sample…",
        ] {
            let label = texts
                .iter()
                .find(|text| text.galley.job.text == expected)
                .expect("export control must be visible");
            assert!(
                label.pos.x + label.galley.size().x <= width + 1.0,
                "{expected} exceeds the dock"
            );
        }
        assert!(!state.is_busy());
    }
}

#[test]
fn sample_selection_retains_the_exact_decoder_when_directions_share_a_span() {
    let input = Arc::new(ScanInput {
        #[cfg(not(target_arch = "wasm32"))]
        cdda: None,
        system: Some(System::Gba),
        standalone_audio: None,
        bytes: crate::audio_discovery::test_support::gba_fixture().into(),
        provenance: None,
        analysis_profile: "shared-sample-selection-test",
        display_name: None,
    });
    let manifest = input.analyze(Default::default(), &AtomicBool::new(false));
    let forward = *samples(&manifest.scan.candidates[0]).next().unwrap();
    let mut reverse = forward;
    reverse.direction = crate::audio_discovery::SampleDirection::Reverse;
    let candidates = [forward, reverse];
    let selected = SelectedSpan {
        span: forward.data.into(),
        label: "forward PCM".to_owned(),
        sample: Some(forward),
    };
    assert_eq!(
        resolve_selected_sample(&selected, candidates.iter()),
        Some(forward)
    );
    let selected = SelectedSpan {
        span: reverse.data.into(),
        label: "reverse PCM".to_owned(),
        sample: Some(reverse),
    };
    assert_eq!(
        resolve_selected_sample(&selected, candidates.iter()),
        Some(reverse)
    );
    let ambiguous = SelectedSpan {
        span: forward.data.into(),
        label: "raw span only".to_owned(),
        sample: None,
    };
    assert_eq!(resolve_selected_sample(&ambiguous, candidates.iter()), None);
}
