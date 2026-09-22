use super::*;

#[test]
fn candidate_evidence_is_visible_without_playback_actions() {
    for (bytes, label, detail) in [
        (
            zeff_audio_discovery::drivers::nes_tose_structure::synthetic_rom(),
            "Structural evidence",
            "bounded descriptor group",
        ),
        (
            zeff_audio_discovery::drivers::nes_sound_writes::synthetic_selector_rom(),
            "Decoded sound-write evidence",
            "selector consumer",
        ),
        (
            zeff_audio_discovery::drivers::nes_sound_writes::synthetic_record_rom(),
            "Decoded sound-write evidence",
            "16 record prefixes",
        ),
        (
            zeff_audio_discovery::drivers::nes_sound_writes::synthetic_dispatch_rom(),
            "Decoded sound-write evidence",
            "1 command dispatch",
        ),
        (
            zeff_audio_discovery::drivers::nes_sound_writes::synthetic_binding_rom(),
            "Decoded sound-write evidence",
            "stream pointer-to-reader bindings",
        ),
        (
            zeff_audio_discovery::drivers::nes_sound_writes::synthetic_head_edge_rom(),
            "Decoded sound-write evidence",
            "conditional command-handler links",
        ),
    ] {
        let source = Arc::new(ScanInput {
            cdda: None,
            system: Some(System::Nes),
            standalone_audio: None,
            bytes: bytes.into(),
            provenance: None,
            analysis_profile: "structural-ui-test",
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
            for frame in 0..2 {
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 800.0),
                        )),
                        ..Default::default()
                    },
                    |ui| super::super::draw_audio_explorer(ui, &mut state),
                );
                if frame == 0 {
                    continue;
                }
                let texts: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                        _ => None,
                    })
                    .collect();
                assert!(texts.iter().any(|text| text.contains(label)));
                assert!(texts.iter().any(|text| text.contains(detail)));
                assert!(texts.iter().any(|text| text.contains("unknown")));
                if manifest.scan.driver_candidates[0].code.is_some() {
                    assert!(
                        texts
                            .iter()
                            .any(|text| text.contains("possible caller links"))
                    );
                    if !manifest.scan.driver_candidates[0]
                        .code
                        .as_ref()
                        .unwrap()
                        .selector_consumers
                        .is_empty()
                    {
                        assert!(
                            texts
                                .iter()
                                .any(|text| text.contains("sequences unverified"))
                        );
                    }
                }
            }
            assert!(state.workspace.selected_candidate.is_none());
            assert!(state.workspace.take_preview_request().is_none());
            assert!(state.workspace.take_export_request().is_none());
        }
    }
}

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
