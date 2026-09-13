use super::*;

use crate::audio_discovery::relations::{AssetEdge, AssetNode};
use crate::audio_discovery::{ScanLimits, ScanStatus, catalog::SongId};
use std::sync::atomic::AtomicBool;
use zeff_emu_common::system::System;

fn graph(song: SongId, nodes: Vec<AssetNode>) -> AssetGraph {
    AssetGraph {
        schema: "test",
        song,
        media_system: "gba",
        media_byte_len: 0x100,
        media_sha256: None,
        scan_status: ScanStatus::Complete,
        detector: None,
        profile: None,
        mp2k_evidence: None,
        logical_source: None,
        limits: GraphLimits::default(),
        status: GraphStatus::Complete,
        work_used: 0,
        root: Some(0),
        nodes,
        edges: Vec::new(),
    }
}

fn node(id: u32, location: AssetLocation) -> AssetNode {
    AssetNode {
        id,
        kind: AssetKind::SequenceData,
        label: format!("Node {id}"),
        location: Some(location),
        sample: None,
    }
}

#[test]
fn only_media_locations_change_the_hex_selection() {
    let mut cached = CachedGraph {
        song: SongId::Mp2k(0),
        graph: graph(
            SongId::Mp2k(0),
            vec![
                node(
                    0,
                    AssetLocation::MediaBytes {
                        offset: 0x20,
                        byte_len: 8,
                        cpu_address: Some(0x0800_0020),
                    },
                ),
                node(
                    1,
                    AssetLocation::LogicalVgm {
                        offset: 0x30,
                        byte_len: 4,
                    },
                ),
                node(
                    2,
                    AssetLocation::DiscTrack {
                        number: 2,
                        index1_lba: 150,
                        end_lba: 151,
                        pcm_frames: 588,
                    },
                ),
            ],
        ),
        selected_node: None,
        copy_error: None,
    };
    let mut workspace = AudioWorkspace::default();

    select_node(&mut workspace, &mut cached, 0);
    let media_selection = workspace.selected_span.clone();
    assert_eq!(cached.selected_node, Some(0));
    assert_eq!(
        media_selection.as_ref().unwrap().span.effective_offset,
        0x20
    );

    select_node(&mut workspace, &mut cached, 1);
    assert_eq!(cached.selected_node, Some(1));
    assert!(workspace.selected_span.is_none());
    assert!(location_label(cached.graph.nodes[1].location.unwrap()).contains("VGM logical"));

    select_node(&mut workspace, &mut cached, 2);
    assert_eq!(cached.selected_node, Some(2));
    assert!(workspace.selected_span.is_none());
    assert!(location_label(cached.graph.nodes[2].location.unwrap()).contains("CD track 2"));
}

#[test]
fn expanded_relationships_reuse_the_cache_and_selection_change_replaces_it() {
    let mut bytes = vec![0; 0x2_100];
    crate::audio_discovery::test_support::collection(&mut bytes, 0x100);
    crate::audio_discovery::test_support::collection(&mut bytes, 0x1_100);
    let report = crate::audio_discovery::scan(
        System::Gba,
        &bytes,
        ScanLimits::default(),
        &AtomicBool::new(false),
    );
    let first = report.song_ids().next().expect("first fixture song");
    let second = report.song_ids().nth(1).expect("second fixture song");
    let context = egui::Context::default();
    let mut workspace = AudioWorkspace::default();
    workspace.ensure_selection(&report);
    let render = |workspace: &mut AudioWorkspace, events| {
        context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 480.0),
                )),
                events,
                ..Default::default()
            },
            |ui| draw(ui, workspace, &report),
        )
    };

    let output = render(&mut workspace, Vec::new());
    assert!(workspace.relationships.is_none());
    let header = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "Asset relationships" => Some(text),
            _ => None,
        })
        .expect("relationship section header");
    let position = header.pos + header.galley.size() / 2.0;
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
    let _ = render(&mut workspace, Vec::new());
    let first_cache = workspace
        .relationships
        .as_mut()
        .expect("expanded graph cache");
    assert_eq!(first_cache.song, first);
    let retained_node = first_cache
        .graph
        .nodes
        .last()
        .expect("fixture relationship node")
        .id;
    first_cache.selected_node = Some(retained_node);
    let _ = render(&mut workspace, Vec::new());
    assert_eq!(
        workspace.relationships.as_ref().unwrap().selected_node,
        Some(retained_node)
    );

    let song = report.song(second).expect("second fixture song");
    super::super::select_song(&mut workspace, second, song);
    assert!(workspace.relationships.is_none());
    let _ = render(&mut workspace, Vec::new());
    assert_eq!(workspace.relationships.as_ref().unwrap().song, second);
}

#[test]
fn non_media_selection_suppresses_the_next_hex_default() {
    let bytes = crate::audio_discovery::test_support::gba_fixture();
    let report = crate::audio_discovery::scan(
        System::Gba,
        &bytes,
        ScanLimits::default(),
        &AtomicBool::new(false),
    );
    let song = report.song_ids().next().expect("fixture song");
    let mut workspace = AudioWorkspace {
        selected_candidate: Some(song),
        relationships: Some(CachedGraph {
            song,
            graph: graph(
                song,
                vec![node(
                    0,
                    AssetLocation::LogicalVgm {
                        offset: 0x30,
                        byte_len: 4,
                    },
                )],
            ),
            selected_node: Some(0),
            copy_error: None,
        }),
        ..Default::default()
    };

    workspace.ensure_selection(&report);
    assert!(workspace.selected_span.is_none());
}

#[test]
fn related_node_list_reports_connections_beyond_its_display_limit() {
    let mut nodes = vec![node(
        0,
        AssetLocation::LogicalVgm {
            offset: 0,
            byte_len: 4,
        },
    )];
    nodes.extend((1..=25).map(|id| {
        node(
            id,
            AssetLocation::LogicalVgm {
                offset: id * 4,
                byte_len: 4,
            },
        )
    }));
    let mut graph = graph(SongId::Mp2k(0), nodes);
    graph.edges = (1..=25)
        .map(|to| AssetEdge {
            from: 0,
            to,
            relation: Relation::Contains,
            evidence: RelationEvidence::ParsedStructure,
        })
        .collect();

    let related = related_nodes(&graph, 0);
    assert_eq!(related.total, 25);
    assert_eq!(related.items.len(), MAX_RELATED_NODES);
}
