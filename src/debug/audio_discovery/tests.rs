use std::sync::{Arc, atomic::AtomicBool};

use super::workspace::{
    AudioWorkspace, filtered_song_ids, selected_bytes, use_wide_layout, visible_hex_rows,
};
use crate::audio_discovery::catalog::SongId;
use crate::audio_discovery::media::ScanInput;
use crate::audio_discovery::{RomSpan, ScanLimits, SourceSpan};
use zeff_emu_common::system::System;

mod fingerprints;
mod interactions;
mod transport;

fn report_and_bytes() -> (crate::audio_discovery::ScanReport, Vec<u8>) {
    let bytes = crate::audio_discovery::test_support::gba_fixture();
    let input = ScanInput {
        cdda: None,
        system: Some(System::Gba),
        standalone_audio: None,
        bytes: bytes.clone().into(),
        provenance: None,
        analysis_profile: "debug-audio-workspace-test-v1",
        display_name: None,
    };
    (
        input
            .analyze(ScanLimits::default(), &AtomicBool::new(false))
            .scan,
        bytes,
    )
}

fn span(offset: usize, len: usize) -> RomSpan {
    RomSpan {
        effective_offset: offset as u32,
        byte_len: len as u32,
        canonical_cpu_address: 0x0800_0000 + offset as u32,
    }
}

#[test]
fn workspace_uses_columns_only_when_the_dock_is_wide() {
    assert!(use_wide_layout(1_000.0));
    assert!(!use_wide_layout(600.0));

    let (report, bytes) = report_and_bytes();
    let context = egui::Context::default();
    for width in [1_000.0, 420.0] {
        let mut workspace = AudioWorkspace::default();
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 640.0),
                )),
                ..Default::default()
            },
            |ui| {
                super::workspace::draw(
                    ui,
                    &mut workspace,
                    &report,
                    &bytes,
                    &crate::audio_discovery::roles::classify(&report),
                    |_| true,
                );
            },
        );
        assert!(!output.shapes.is_empty());
    }
}

#[test]
fn provenance_summary_hides_hashes_and_detector_metrics_by_default() {
    let bytes = crate::audio_discovery::test_support::gba_fixture();
    let input = ScanInput {
        cdda: None,
        system: Some(System::Gba),
        standalone_audio: None,
        bytes: bytes.into(),
        provenance: None,
        analysis_profile: "compact-provenance-test-v1",
        display_name: None,
    };
    let manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
    let context = egui::Context::default();
    let output = context.run_ui(Default::default(), |ui| {
        super::draw_provenance(ui, &manifest);
    });
    let text = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(text.iter().any(|value| value.starts_with("Loaded media: ")));
    assert!(text.contains(&"Loaded directly from an in-memory source."));
    assert!(text.contains(&"Technical diagnostics"));
    assert!(!text.iter().any(|value| value.contains("SHA-256")));
    assert!(!text.iter().any(|value| value.contains("work units")));
    assert!(
        !text
            .iter()
            .any(|value| value.contains("compact-provenance-test-v1"))
    );
}

#[test]
fn provenance_source_kinds_have_human_compact_labels() {
    let sources = [
        (
            "direct_gba_file",
            "Opened from a Game Boy Advance cartridge file.",
        ),
        ("direct_cartridge_file", "Opened from a cartridge file."),
        (
            "preloaded_gba_bytes",
            "Loaded from the running Game Boy Advance snapshot.",
        ),
        (
            "preloaded_cartridge_bytes",
            "Loaded from the running cartridge snapshot.",
        ),
        ("direct_xm_file", "Opened from a standalone XM module."),
        (
            "direct_vgm_file",
            "Opened from a standalone VGM or VGZ register log.",
        ),
        ("direct_gbs_file", "Opened from a standalone GBS music rip."),
    ];
    for (kind, expected) in sources {
        assert_eq!(super::source_description(kind), expected);
    }
    assert_eq!(
        super::disc_source_description("loaded_disc"),
        "Loaded from the running disc snapshot"
    );
    assert_eq!(
        super::disc_source_description("zip_cue"),
        "Opened from a CUE disc set in a ZIP archive"
    );
}

#[test]
fn natsume_summary_uses_a_prominent_details_action_and_keeps_metrics_advanced() {
    use crate::audio_discovery::natsume::{
        NatsumeChannel, NatsumeSong, NatsumeSongKind, NatsumeTermination,
    };

    let (mut report, bytes) = report_and_bytes();
    report.candidates.clear();
    report.gax_songs.clear();
    report.tracker_modules.clear();
    report.natsume_songs.push(NatsumeSong {
        profile: "fixture-natsume-profile",
        index: 2,
        title: "Fixture Natsume song".to_owned(),
        kind: NatsumeSongKind::Music,
        table_entry: span(0x80, 4),
        header: span(0x100, 8),
        channel_mask: 0x800,
        priority: 192,
        channels: vec![NatsumeChannel {
            number: 0,
            hardware_kind: 0,
            entry: span(0x300, 1),
            event_count: 12,
            note_count: 4,
            wait_units: 96,
            termination: NatsumeTermination::Loop,
            loop_start_wait_units: Some(24),
        }],
        mapped_spans: vec![span(0x80, 4), span(0x100, 8), span(0x300, 16)],
        warnings: vec!["Fixture structural warning".to_owned()],
    });
    let context = egui::Context::default();
    let mut workspace = AudioWorkspace::default();
    let render = |workspace: &mut AudioWorkspace, events| {
        context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1_000.0, 640.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                super::workspace::draw(
                    ui,
                    workspace,
                    &report,
                    &bytes,
                    &crate::audio_discovery::roles::classify(&report),
                    |_| true,
                )
            },
        )
    };
    let _ = render(&mut workspace, Vec::new());
    let output = render(&mut workspace, Vec::new());
    let details = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "View song details" => Some(text),
            _ => None,
        })
        .expect("the selected song exposes a clear details action");
    let position = details.pos + details.galley.size() / 2.0;
    for pressed in [true, false] {
        let _ = render(
            &mut workspace,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
        );
    }
    let output = render(&mut workspace, Vec::new());
    let text = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(text.contains(&"Back to song summary"));
    assert!(text.contains(&"Fixture Natsume song"));
    assert!(text.contains(&"1 channels"));
    assert!(text.contains(&"Advanced sequence data"));
    assert!(
        text.iter()
            .any(|value| value.contains("Original-driver preview"))
    );
    assert!(!text.iter().any(|value| value.contains("wait units")));
    assert!(!text.iter().any(|value| value.contains("Priority 192")));
    assert!(
        !text
            .iter()
            .any(|value| value.contains("fixture-natsume-profile"))
    );
    assert!(
        !text
            .iter()
            .any(|value| value.contains("Fixture structural warning"))
    );
}

#[test]
fn filter_selection_and_source_reset_are_scoped_to_the_workspace() {
    let (report, _) = report_and_bytes();
    assert!(!report.candidates.is_empty());
    assert_eq!(
        filtered_song_ids(&report, "mp2k +000100"),
        vec![SongId::Mp2k(0)]
    );
    assert!(filtered_song_ids(&report, "no matching candidate").is_empty());

    let mut workspace = AudioWorkspace::default();
    workspace.ensure_selection(&report);
    assert!(workspace.selected_span.is_some());
    workspace = AudioWorkspace::default();
    assert!(workspace.selected_span.is_none());
}

#[test]
fn song_filter_and_title_use_table_indices_and_source_binding_clears_selection() {
    use std::sync::Arc;
    let (mut report, bytes) = report_and_bytes();
    report.candidates[0]
        .table_entries
        .push(crate::audio_discovery::SongTableReference {
            table_offset: 0x800,
            index: 42,
            entry: span(0x950, 8),
            player: 3,
        });
    assert_eq!(
        super::workspace::song_title(&report.candidates[0]),
        "Song 42"
    );
    assert_eq!(filtered_song_ids(&report, "song 42"), vec![SongId::Mp2k(0)]);
    assert!(filtered_song_ids(&report, "song 0").is_empty());
    assert_eq!(
        filtered_song_ids(&report, "table 000800"),
        vec![SongId::Mp2k(0)]
    );
    let input = || {
        Arc::new(ScanInput {
            cdda: None,
            system: Some(System::Gba),
            standalone_audio: None,
            bytes: bytes.clone().into(),
            provenance: None,
            analysis_profile: "test-source-reset",
            display_name: None,
        })
    };
    let first = input();
    let mut state = super::AudioDiscoveryState::default();
    state.bind_source(Some(Arc::clone(&first)));
    state.workspace.ensure_selection(&report);
    state.bind_source(Some(first));
    assert!(state.workspace.selected_span.is_some());
    state.bind_source(Some(input()));
    assert!(state.workspace.selected_span.is_none());
    state.workspace.ensure_selection(&report);
    state.bind_source(None);
    assert!(state.workspace.selected_span.is_none());
}

#[test]
fn hex_rows_follow_only_the_selected_exact_range() {
    let bytes = vec![0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17];
    let selected = span(2, 5);
    assert_eq!(
        selected_bytes(&bytes, selected),
        Some(&[0x12, 0x13, 0x14, 0x15, 0x16][..])
    );
    assert_eq!(
        visible_hex_rows(selected, selected_bytes(&bytes, selected).unwrap(), 4, 0..2),
        vec![(2, 0x12), (3, 0x13), (4, 0x14), (5, 0x15), (6, 0x16)]
    );
    assert!(selected_bytes(&bytes, span(7, 2)).is_none());
}

#[test]
fn generic_file_spans_keep_their_native_addresses() {
    let bytes = vec![0x10, 0x11, 0x12, 0x13, 0x14, 0x15];
    let span = SourceSpan {
        effective_offset: 1,
        byte_len: 3,
        canonical_cpu_address: None,
    };
    assert_eq!(selected_bytes(&bytes, span), Some(&[0x11, 0x12, 0x13][..]));
    assert_eq!(
        visible_hex_rows(span, selected_bytes(&bytes, span).unwrap(), 2, 0..2),
        vec![(1, 0x11), (2, 0x12), (3, 0x13)]
    );
}

#[test]
fn workspace_selects_embedded_modules_without_a_fake_cpu_address() {
    let (mut report, _) = report_and_bytes();
    report.candidates.clear();
    report.gax_songs.clear();
    report
        .tracker_modules
        .push(crate::audio_discovery::tracker::EmbeddedModule {
            format: crate::audio_discovery::tracker::EmbeddedFormat::Xm,
            span: crate::audio_discovery::tracker::FileSpan {
                offset: 0x240,
                byte_len: 0x80,
            },
            name: "Fixture module".to_owned(),
            channels: 4,
            orders: 1,
            patterns: 1,
            instruments: 1,
            samples: 1,
            sample_points: 16,
            source: crate::audio_discovery::tracker::ModuleSource::Embedded,
        });
    let mut workspace = AudioWorkspace::default();
    workspace.ensure_selection(&report);
    assert_eq!(workspace.selected_candidate, Some(SongId::Module(0)));
    assert_eq!(
        workspace.selected_span.as_ref().unwrap().span,
        SourceSpan {
            effective_offset: 0x240,
            byte_len: 0x80,
            canonical_cpu_address: None,
        }
    );
}

#[test]
fn large_song_lists_are_virtualized_and_clicks_move_the_hex_selection() {
    let mut bytes = vec![0; 261 * 0x1000];
    for index in 0..261 {
        crate::audio_discovery::test_support::collection(&mut bytes, index * 0x1000 + 0x100);
    }
    let report = crate::audio_discovery::scan(
        System::Gba,
        &bytes,
        ScanLimits::default(),
        &AtomicBool::new(false),
    );
    assert_eq!(report.candidates.len(), 261);
    for width in [1_000.0, 420.0] {
        let context = egui::Context::default();
        context.all_styles_mut(|style| {
            for font in style.text_styles.values_mut() {
                font.size *= 1.4;
            }
        });
        let mut workspace = AudioWorkspace::default();
        let render = |workspace: &mut AudioWorkspace, events| {
            context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 480.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    super::workspace::draw(
                        ui,
                        workspace,
                        &report,
                        &bytes,
                        &crate::audio_discovery::roles::classify(&report),
                        |_| true,
                    )
                },
            )
        };
        let _ = render(&mut workspace, Vec::new());
        let output = render(&mut workspace, Vec::new());
        let rows = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text)
                    if text.galley.job.text.starts_with("MP2k +")
                        && text.galley.job.text.contains("tracks") =>
                {
                    Some(text)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            (2..40).contains(&rows.len()),
            "only visible song rows should be laid out: {}",
            rows.len()
        );
        assert!(
            rows.iter()
                .all(|text| text.pos.x + text.galley.size().x <= width + 0.1)
        );
        let second = rows
            .iter()
            .find(|text| text.galley.job.text.starts_with("MP2k +001100"))
            .unwrap();
        let position = second.pos + egui::vec2(8.0, second.galley.size().y / 2.0);
        for pressed in [true, false] {
            let _ = render(
                &mut workspace,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
            );
        }
        assert_eq!(
            workspace.selected_span.as_ref().unwrap().span,
            report.candidates[1].header.into()
        );
    }
}

#[test]
fn large_song_catalog_keeps_preview_transport_in_the_viewport() {
    let entry_count = 507;
    let mut bytes = vec![0; entry_count * 0x1000];
    for index in 0..entry_count {
        crate::audio_discovery::test_support::collection(&mut bytes, index * 0x1000 + 0x100);
    }
    let source = Arc::new(ScanInput {
        cdda: None,
        system: Some(System::Gba),
        standalone_audio: None,
        bytes: bytes.into(),
        provenance: None,
        analysis_profile: "audio-preview-layout-test-v1",
        display_name: Some("Large catalog".to_owned()),
    });
    for (width, height, minimum_rows) in [(760.0, 570.0, 4), (640.0, 480.0, 2)] {
        let context = egui::Context::default();
        let mut state = super::AudioDiscoveryState::default();
        state.bind_source(Some(Arc::clone(&source)));
        let mut manifest = source.analyze(ScanLimits::default(), &AtomicBool::new(false));
        let track = manifest.scan.candidates[0].tracks[0].clone();
        manifest.scan.candidates[0].tracks = vec![track; 16];
        assert_eq!(manifest.scan.candidates.len(), entry_count);
        state.session.manifest = Some(manifest);
        let render = |state: &mut super::AudioDiscoveryState| {
            context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, height),
                    )),
                    ..Default::default()
                },
                |ui| super::draw_audio_explorer(ui, state),
            )
        };
        let _ = render(&mut state);
        let request = crate::audio_discovery::preview::PreviewRequest::prepare_song(
            &source,
            state.session.manifest.as_ref().unwrap(),
            SongId::Mp2k(0),
            crate::audio_discovery::render::RenderOptions {
                max_seconds: 2,
                ..Default::default()
            },
        )
        .unwrap();
        let receiver = state.preview.player.start_captured(request);
        let _callback = receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !state
            .preview
            .player
            .snapshot()
            .is_some_and(|snapshot| !snapshot.preparing && snapshot.duration != 0)
        {
            state.preview.player.poll();
            assert!(
                state.preview.player.error.is_none(),
                "{:?}",
                state.preview.player.error
            );
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let output = render(&mut state);
        let play = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "Pause preview" => Some(text),
                _ => None,
            })
            .next()
            .expect("the selected MP2k song exposes preview transport");
        assert!(
            play.pos.y + play.galley.size().y <= height,
            "preview transport fell below a {height}px viewport with {entry_count} songs"
        );
        let text = |label: &str| {
            output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Text(value) if value.galley.job.text == label)
            })
        };
        let visible_rows = output
            .shapes
            .iter()
            .filter(|shape| {
                matches!(
                    &shape.shape,
                    egui::Shape::Text(value)
                        if value.galley.job.text.starts_with("MP2k +")
                            && value.galley.job.text.contains("tracks")
                            && shape.clip_rect.contains_rect(egui::Rect::from_min_size(value.pos, value.galley.size()))
                )
            })
            .count();
        assert!(
            visible_rows >= minimum_rows,
            "the song list should retain usable rows in a {width}x{height}px viewport"
        );
        assert!(text("Position"));
        assert!(text("Options"));
        assert!(text("Tracks"));
        assert!(!text("Candidates"));
        assert!(!text("Reset tracks"));
    }
}
