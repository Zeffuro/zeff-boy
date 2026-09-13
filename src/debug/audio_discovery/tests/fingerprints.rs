use super::*;

#[test]
fn driver_only_results_are_visible_and_inspectable_without_playback_selection() {
    let mut bytes = vec![0; 32768];
    bytes[1000..1016].copy_from_slice(b"GHX Audio Engine");
    let source = Arc::new(ScanInput {
        cdda: None,
        system: Some(System::Gb),
        standalone_audio: None,
        bytes: bytes.into(),
        provenance: None,
        analysis_profile: "fingerprint-ui-test",
        display_name: None,
    });
    let manifest = source.analyze(ScanLimits::default(), &AtomicBool::new(false));
    assert_eq!(manifest.scan.song_count(), 0);
    assert_eq!(manifest.scan.driver_candidates.len(), 1);
    for width in [420.0, 1000.0] {
        let mut state = super::super::AudioDiscoveryState::default();
        state.bind_source(Some(source.clone()));
        state.session.manifest = Some(manifest.clone());
        let context = egui::Context::default();
        let draw = |state: &mut super::super::AudioDiscoveryState, events| {
            context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| super::super::draw_audio_explorer(ui, state),
            )
        };
        let _ = draw(&mut state, Vec::new());
        let output = draw(&mut state, Vec::new());
        let texts: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text),
                _ => None,
            })
            .collect();
        assert!(
            texts
                .iter()
                .any(|text| text.galley.job.text.contains("Possible sound drivers (1)"))
        );
        assert!(texts.iter().any(|text| text.galley.job.text == "GHX"));
        let evidence = texts
            .iter()
            .find(|text| text.galley.job.text.starts_with("ghx_audio:"))
            .unwrap();
        let pos = evidence.pos + egui::vec2(8.0, evidence.galley.size().y / 2.0);
        for pressed in [true, false] {
            let _ = draw(
                &mut state,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
            );
        }
        let selected = state.workspace.selected_span.as_ref().unwrap();
        assert_eq!(selected.span.effective_offset, 1000);
        assert_eq!(selected.span.byte_len, 16);
        assert_eq!(selected.span.canonical_cpu_address, None);
        assert!(state.workspace.selected_candidate.is_none());
        assert!(state.workspace.take_preview_request().is_none());
        assert!(state.workspace.take_export_request().is_none());
    }
}
