use super::*;
use crate::audio_discovery::preview::PreviewRequest;
use crate::audio_discovery::render::RenderOptions;

fn song_positions(output: &egui::FullOutput) -> std::collections::BTreeMap<String, egui::Pos2> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text)
                if text.galley.job.text.starts_with("MP2k +")
                    && text.galley.job.text.contains("tracks") =>
            {
                Some((
                    text.galley.job.text.clone(),
                    text.pos + egui::vec2(8.0, text.galley.size().y / 2.0),
                ))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn preview_transitions_keep_song_rows_under_the_pointer() {
    let mut bytes = vec![0; 32 * 0x1000];
    for index in 0..32 {
        crate::audio_discovery::test_support::collection(&mut bytes, index * 0x1000 + 0x100);
    }
    let source = Arc::new(ScanInput {
        cdda: None,
        system: Some(System::Gba),
        standalone_audio: None,
        bytes: bytes.into(),
        provenance: None,
        analysis_profile: "stable-preview-transport-test",
        display_name: Some("Transport fixture".to_owned()),
    });
    let manifest = source.analyze(ScanLimits::default(), &AtomicBool::new(false));
    for (width, font_scale) in [(420.0, 1.0), (1000.0, 1.0), (1000.0, 1.4)] {
        let context = egui::Context::default();
        context.all_styles_mut(|style| {
            for font in style.text_styles.values_mut() {
                font.size *= font_scale;
            }
        });
        let mut preview = super::super::preview::PreviewState::default();
        let mut workspace = AudioWorkspace::default();
        let mut clock = 0.0;
        let mut render = |preview: &mut super::super::preview::PreviewState,
                          workspace: &mut AudioWorkspace,
                          events| {
            clock += 0.04;
            context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 600.0),
                    )),
                    time: Some(clock),
                    events,
                    ..Default::default()
                },
                |ui| {
                    workspace.ensure_selection(&manifest.scan);
                    super::super::preview::draw(
                        ui,
                        preview,
                        &source,
                        &manifest,
                        workspace.selected_candidate,
                    );
                    ui.separator();
                    super::super::workspace::draw(
                        ui,
                        workspace,
                        &manifest.scan,
                        &source.bytes,
                        |_| true,
                    );
                },
            )
        };
        let _ = render(&mut preview, &mut workspace, Vec::new());
        let idle = song_positions(&render(&mut preview, &mut workspace, Vec::new()));
        assert!(idle.len() >= 2);
        preview.player.error = Some("A deliberately long preview failure message ".repeat(20));
        assert_eq!(
            song_positions(&render(&mut preview, &mut workspace, Vec::new())),
            idle
        );
        preview.player.error = None;
        let request = PreviewRequest::prepare_song(
            &source,
            &manifest,
            SongId::Mp2k(0),
            RenderOptions {
                max_seconds: 2,
                ..Default::default()
            },
        )
        .unwrap();
        let receiver = preview.player.start_captured(request);
        assert_eq!(
            song_positions(&render(&mut preview, &mut workspace, Vec::new())),
            idle
        );
        let _callback = receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !preview
            .player
            .snapshot()
            .is_some_and(|snapshot| !snapshot.preparing && snapshot.duration != 0)
        {
            preview.player.poll();
            assert!(preview.player.error.is_none());
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        for playing in [true, false, true] {
            preview.player.set_playing(playing);
            let _ = render(&mut preview, &mut workspace, Vec::new());
            let active = song_positions(&render(&mut preview, &mut workspace, Vec::new()));
            assert_eq!(active, idle, "transport shifted rows at width {width}");
        }
        let position = *idle
            .iter()
            .find(|(text, _)| text.starts_with("MP2k +001100"))
            .unwrap()
            .1;
        for pressed in [true, false, true, false] {
            let _ = render(
                &mut preview,
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
            let positions = song_positions(&render(&mut preview, &mut workspace, Vec::new()));
            assert_eq!(positions, idle, "selection shifted rows between clicks");
        }
        assert_eq!(workspace.selected_candidate, Some(SongId::Mp2k(1)));
        assert_eq!(workspace.take_preview_request(), Some(SongId::Mp2k(1)));
        preview.player.stop();
    }
}

#[test]
fn natsume_music_and_control_rows_keep_the_same_table_position() {
    use crate::audio_discovery::natsume::{NatsumeSong, NatsumeSongKind};
    let source = Arc::new(ScanInput {
        cdda: None,
        system: Some(System::Gba),
        standalone_audio: None,
        bytes: vec![0; 0x1000].into(),
        provenance: None,
        analysis_profile: "native-transport-layout",
        display_name: None,
    });
    let mut manifest = source.analyze(Default::default(), &AtomicBool::new(false));
    for index in 0..32 {
        manifest.scan.natsume_songs.push(NatsumeSong {
            profile: crate::audio_discovery::natsume::PROFILE,
            index,
            title: format!("Native row {index:02}"),
            kind: if index == 1 {
                NatsumeSongKind::Setup
            } else {
                NatsumeSongKind::Music
            },
            table_entry: span(0x80 + usize::from(index) * 4, 4),
            header: span(0x100, 8),
            channel_mask: 1,
            priority: 0,
            channels: Vec::new(),
            mapped_spans: Vec::new(),
            warnings: Vec::new(),
        });
    }
    for width in [420.0, 1000.0] {
        let context = egui::Context::default();
        let mut preview = super::super::preview::PreviewState::default();
        let mut workspace = AudioWorkspace::default();
        let mut render = |id| {
            workspace.selected_candidate = Some(id);
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 600.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    super::super::preview::draw(ui, &mut preview, &source, &manifest, Some(id));
                    ui.separator();
                    super::super::workspace::draw(
                        ui,
                        &mut workspace,
                        &manifest.scan,
                        &source.bytes,
                        |_| true,
                    );
                },
            );
            output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text)
                        if text.galley.job.text.starts_with("Song ")
                            && text.galley.job.text.contains("GBA Natsume driver") =>
                    {
                        Some((text.galley.job.text.clone(), text.pos))
                    }
                    _ => None,
                })
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        let _ = render(SongId::Natsume(0));
        let music = render(SongId::Natsume(0));
        assert!(music.len() >= 2);
        for id in [1, 0, 2, 1] {
            let _ = render(SongId::Natsume(id));
            assert_eq!(
                render(SongId::Natsume(id)),
                music,
                "native table shifted at width {width}"
            );
        }
    }
}
