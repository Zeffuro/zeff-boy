use super::*;
use crate::{ScanLimits, test_support, tracker::EmbeddedFormat};
use zeff_emu_common::system::System;

fn scan_gba(bytes: &[u8]) -> ScanReport {
    crate::scan(
        System::Gba,
        bytes,
        ScanLimits::default(),
        &AtomicBool::new(false),
    )
}

fn graph(report: &ScanReport, id: SongId) -> AssetGraph {
    report.asset_relations(id, GraphLimits::default(), &AtomicBool::new(false))
}

fn bounded(graph: &AssetGraph) {
    assert!(graph.nodes.len() <= graph.limits.max_nodes as usize);
    assert!(graph.edges.len() <= graph.limits.max_edges as usize);
    assert!(graph.work_used <= graph.limits.max_work);
    for (index, node) in graph.nodes.iter().enumerate() {
        assert_eq!(node.id as usize, index);
        assert!(node.label.len() <= MAX_LABEL_BYTES);
    }
    assert!(
        graph
            .root
            .is_none_or(|root| (root as usize) < graph.nodes.len())
    );
    for edge in &graph.edges {
        assert!((edge.from as usize) < graph.nodes.len());
        assert!((edge.to as usize) < graph.nodes.len());
    }
}

#[test]
fn mp2k_relations_share_assets_and_preserve_source_evidence() {
    let report = scan_gba(&test_support::fixture());
    let before = serde_json::to_value(&report).unwrap();
    let result = graph(&report, SongId::Mp2k(0));
    assert_eq!(result.status, GraphStatus::Complete);
    assert_eq!(result, graph(&report, SongId::Mp2k(0)));
    assert_eq!(
        result
            .nodes
            .iter()
            .filter(|n| n.kind == AssetKind::Instrument)
            .count(),
        1
    );
    assert_eq!(
        result
            .nodes
            .iter()
            .filter(|n| n.kind == AssetKind::Channel)
            .count(),
        2
    );
    let sample = result
        .nodes
        .iter()
        .find(|n| n.kind == AssetKind::Sample)
        .unwrap();
    assert_eq!(sample.sample, report.candidates[0].instruments[0].sample);
    let evidence = result.mp2k_evidence.as_ref().unwrap();
    assert_eq!(evidence.evidence, report.candidates[0].evidence);
    assert_eq!(evidence.confidence, report.candidates[0].confidence);
    assert_eq!(result.detector.unwrap().id, "mp2k-sequence");
    assert_eq!(before, serde_json::to_value(&report).unwrap());
    bounded(&result);
}

#[test]
fn shared_pcm_bytes_keep_distinct_direction_interpretations() {
    let mut report = scan_gba(&test_support::fixture());
    let mut reverse = report.candidates[0].instruments[0].clone();
    reverse.voice = 1;
    reverse.tone.descriptor = crate::RomSpan::new(0x220, 12);
    reverse.tone.kind = 0x20;
    reverse.tone.sample.as_mut().unwrap().direction = crate::SampleDirection::Reverse;
    report.candidates[0].instruments.push(reverse);
    let result = graph(&report, SongId::Mp2k(0));
    let samples = result
        .nodes
        .iter()
        .filter_map(|node| node.sample)
        .collect::<Vec<_>>();
    assert_eq!(samples.len(), 2);
    assert_ne!(samples[0].direction, samples[1].direction);
    assert_eq!(
        result
            .nodes
            .iter()
            .filter(|node| node.kind == AssetKind::SampleData)
            .count(),
        1
    );
    bounded(&result);
}

#[test]
fn hardware_psg_does_not_invent_a_missing_pcm_sample() {
    let mut report = scan_gba(&test_support::fixture());
    let tone = &mut report.candidates[0].instruments[0].tone;
    tone.kind = 1;
    tone.sample = None;
    tone.sample_header = None;
    report.candidates[0].warnings.clear();
    let result = graph(&report, SongId::Mp2k(0));
    assert_eq!(result.status, GraphStatus::Complete);
    assert!(
        !result
            .nodes
            .iter()
            .any(|node| node.kind == AssetKind::Unresolved)
    );
}

#[test]
fn graph_limits_and_cancellation_leave_no_dangling_edges() {
    let report = scan_gba(&test_support::fixture());
    for (limits, expected) in [
        (
            GraphLimits {
                max_nodes: 1,
                ..GraphLimits::default()
            },
            GraphStop::NodeLimit,
        ),
        (
            GraphLimits {
                max_edges: 0,
                ..GraphLimits::default()
            },
            GraphStop::EdgeLimit,
        ),
        (
            GraphLimits {
                max_work: 1,
                ..GraphLimits::default()
            },
            GraphStop::WorkLimit,
        ),
    ] {
        let result = report.asset_relations(SongId::Mp2k(0), limits, &AtomicBool::new(false));
        assert_eq!(result.status, GraphStatus::Incomplete(expected));
        bounded(&result);
    }
    let cancelled = report.asset_relations(
        SongId::Mp2k(0),
        GraphLimits::default(),
        &AtomicBool::new(true),
    );
    assert_eq!(
        cancelled.status,
        GraphStatus::Incomplete(GraphStop::Cancelled)
    );
    assert!(cancelled.nodes.is_empty());
    let invalid = report.asset_relations(
        SongId::Mp2k(0),
        GraphLimits {
            max_nodes: MAX_NODES + 1,
            ..GraphLimits::default()
        },
        &AtomicBool::new(false),
    );
    assert_eq!(
        invalid.status,
        GraphStatus::Incomplete(GraphStop::InvalidLimits)
    );
    assert_eq!(
        graph(&report, SongId::Mp2k(99)).status,
        GraphStatus::Incomplete(GraphStop::MissingSong)
    );
}

#[test]
fn deduplicated_references_still_consume_work() {
    let mut report = scan_gba(&test_support::fixture());
    report.candidates[0].tracks[0].voices = vec![0; 4096];
    let limits = GraphLimits {
        max_work: 80,
        ..GraphLimits::default()
    };
    let result = report.asset_relations(SongId::Mp2k(0), limits, &AtomicBool::new(false));
    assert_eq!(result.status, GraphStatus::Incomplete(GraphStop::WorkLimit));
    assert_eq!(result.work_used, limits.max_work);
    bounded(&result);
}

#[test]
fn corrupt_ranges_and_cpu_domains_are_rejected() {
    for wrong_cpu in [false, true] {
        let mut report = scan_gba(&test_support::fixture());
        if wrong_cpu {
            report.candidates[0].header.canonical_cpu_address = 0x0200_0000;
        } else {
            report.candidates[0].header.byte_len = u32::MAX;
        }
        let result = graph(&report, SongId::Mp2k(0));
        assert_eq!(
            result.status,
            GraphStatus::Incomplete(GraphStop::InvalidLocation)
        );
        assert!(result.nodes.is_empty());
    }
}

#[test]
fn gax_orders_use_pattern_positions_and_sparse_asset_ids() {
    let report = scan_gba(&test_support::gax::fixture());
    let result = graph(&report, SongId::Gax(0));
    assert_eq!(result.status, GraphStatus::Complete);
    assert_eq!(
        result.profile.as_deref(),
        Some(report.gax_songs[0].version.as_str())
    );
    assert_eq!(
        result
            .nodes
            .iter()
            .filter(|n| n.kind == AssetKind::Order)
            .count(),
        4
    );
    assert_eq!(
        result
            .nodes
            .iter()
            .filter(|n| n.kind == AssetKind::Pattern)
            .count(),
        1
    );
    let sample = result
        .nodes
        .iter()
        .find(|n| n.kind == AssetKind::Sample)
        .unwrap();
    assert_eq!(sample.label, "Sample 2");
    assert!(
        result
            .edges
            .iter()
            .any(|e| e.to == sample.id && e.relation == Relation::DeclaresSampleSlot { slot: 1 })
    );
    bounded(&result);
}

#[test]
fn gax_declared_but_unselected_sample_slots_are_connected() {
    let mut report = scan_gba(&test_support::gax::fixture());
    let song = &mut report.gax_songs[0];
    let mut second = song.samples[0];
    second.index = 7;
    second.header = crate::RomSpan::new(0x238, 8);
    song.samples.push(second);
    song.instruments[0].sample_indices[1] = 7;
    let setting = song.instruments[0].sample_settings[0];
    song.instruments[0].sample_settings.push(setting);
    song.instruments[0].rows[0].sample_slot = 2;
    let result = graph(&report, SongId::Gax(0));
    assert_eq!(result.status, GraphStatus::Complete);
    for sample in result.nodes.iter().filter(|n| n.kind == AssetKind::Sample) {
        assert!(result.edges.iter().any(
            |e| e.to == sample.id && matches!(e.relation, Relation::DeclaresSampleSlot { .. })
        ));
    }
}

#[test]
fn tracker_counts_do_not_become_fabricated_asset_ranges() {
    let bytes = test_support::tracker::xm_fixture();
    let report = crate::scan_standalone_tracker(
        &bytes,
        EmbeddedFormat::Xm,
        ScanLimits::default(),
        &AtomicBool::new(false),
    );
    let result = graph(&report, SongId::Module(0));
    assert_eq!(result.status, GraphStatus::Complete);
    assert_eq!(result.nodes.len(), 2);
    assert_eq!(result.nodes[1].kind, AssetKind::Unresolved);
    assert!(result.nodes[1].location.is_none());
}

#[test]
fn vgz_locations_retain_their_logical_source_identity() {
    use std::io::Write;
    let logical = test_support::vgm::fixture(true);
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&logical).unwrap();
    let bytes = encoder.finish().unwrap();
    let mut report = crate::vgm::scan(&bytes, ScanLimits::default(), &AtomicBool::new(false));
    let result = graph(&report, SongId::Vgm(0));
    assert_eq!(result.status, GraphStatus::Complete);
    assert_eq!(
        result.logical_source.as_ref(),
        Some(&report.vgm_logs[0].logical)
    );
    assert!(matches!(
        result.nodes[0].location,
        Some(AssetLocation::MediaBytes { .. })
    ));
    assert!(result.nodes.iter().any(|n| matches!(
        n.location,
        Some(AssetLocation::LogicalVgm { offset: 0x40, .. })
    )));
    report.vgm_logs[0].header.address_space = crate::vgm::VgmAddressSpace::SourceFile;
    assert_eq!(
        graph(&report, SongId::Vgm(0)).status,
        GraphStatus::Incomplete(GraphStop::InvalidLocation)
    );
}

#[test]
fn container_song_counts_do_not_imply_discovered_sequences() {
    for format in [crate::rips::RipFormat::Gbs, crate::rips::RipFormat::Nsf] {
        let bytes = test_support::rips::fixture(format);
        let report = crate::rips::scan(
            &bytes,
            format,
            ScanLimits::default(),
            &AtomicBool::new(false),
        );
        let result = graph(&report, SongId::Rip(0));
        assert_eq!(result.status, GraphStatus::Complete);
        assert!(!result.nodes.iter().any(|n| n.kind == AssetKind::Song));
        assert!(result.nodes.iter().any(|n| n.kind == AssetKind::Program));
        bounded(&result);
    }
}

#[test]
fn callback_locations_must_belong_to_the_container_program() {
    let format = crate::rips::RipFormat::Gbs;
    let bytes = test_support::rips::fixture(format);
    let mut report = crate::rips::scan(
        &bytes,
        format,
        ScanLimits::default(),
        &AtomicBool::new(false),
    );
    report.music_rips[0].init.initial_source_offset = Some(0);
    let result = graph(&report, SongId::Rip(0));
    assert_eq!(
        result.status,
        GraphStatus::Incomplete(GraphStop::InvalidLocation)
    );
    bounded(&result);
}

#[test]
fn repeated_nes_sections_keep_occurrences_without_inventing_selectors() {
    let bytes = test_support::nes_music::fixture();
    let mut song = test_support::nes_music::synthetic_song(&bytes, 1);
    let mut second = song.sections[0].clone();
    second.start_frame += 1;
    second.end_frame += 1;
    song.sections.push(second);
    let count = song.sections.len();
    let mut report = crate::scan(
        System::Nes,
        &bytes,
        ScanLimits::default(),
        &AtomicBool::new(false),
    );
    report.nes_songs.push(song);
    let result = graph(&report, SongId::Nes(0));
    assert_eq!(result.status, GraphStatus::Complete);
    assert_eq!(
        result
            .nodes
            .iter()
            .filter(|n| n.kind == AssetKind::Section)
            .count(),
        count
    );
    assert_eq!(
        result
            .edges
            .iter()
            .filter(|e| matches!(e.relation, Relation::Selects { .. }))
            .count(),
        1
    );
    assert!(!result.nodes.iter().any(|n| matches!(
        n.location,
        Some(AssetLocation::MediaBytes {
            cpu_address: Some(_),
            ..
        })
    )));
}

#[test]
fn gbass_modules_keep_original_rom_locations_and_runtime_identity() {
    let report = scan_gba(&crate::gbass::fixture_rom_module());
    assert_eq!(report.gbass_songs.len(), 4);
    for (index, song) in report.gbass_songs.iter().enumerate() {
        let module = song.native.module.unwrap();
        let result = graph(&report, SongId::Gbass(index));
        assert_eq!(result.status, GraphStatus::Complete);
        let node = result
            .nodes
            .iter()
            .find(|node| node.kind == AssetKind::Module)
            .unwrap();
        assert_eq!(node.location, Some(module.source.into()));
        assert!(node.label.contains("loaded at 0x02000000"));
        assert!(result.edges.iter().any(|edge| edge.to == node.id
            && edge.relation
                == Relation::Selects {
                    index: u32::from(module.index)
                }));
        assert!(
            result
                .nodes
                .iter()
                .any(|node| node.kind == AssetKind::EntryPoint
                    && node.location == Some(module.loader.into()))
        );
        bounded(&result);
    }
}

#[test]
fn complete_graph_does_not_erase_incomplete_scan_status() {
    let mut report = scan_gba(&test_support::fixture());
    report.status = ScanStatus::Incomplete(crate::ScanStop::CandidateLimit);
    let result = graph(&report, SongId::Mp2k(0));
    assert_eq!(result.status, GraphStatus::Complete);
    assert_eq!(result.scan_status, report.status);
}

#[test]
fn engine_software_empty_ranges_do_not_become_byte_location_nodes() {
    let mut empty_sample = test_support::engine_software::fixture();
    empty_sample[0x48..0x4c].fill(0);
    for bytes in [
        empty_sample,
        test_support::engine_software::unreachable_zero_pattern(),
    ] {
        let report = scan_gba(&bytes);
        assert_eq!(report.engine_software_songs.len(), 1);
        assert!(
            report.engine_software_songs[0]
                .mapped_spans
                .iter()
                .any(|span| span.byte_len == 0)
        );
        let result = graph(&report, SongId::EngineSoftware(0));
        assert_eq!(result.status, GraphStatus::Complete);
        assert!(
            result
                .nodes
                .iter()
                .any(|node| node.kind == AssetKind::MappedData)
        );
        assert!(
            result
                .nodes
                .iter()
                .filter_map(|node| node.location)
                .all(|location| !matches!(location, AssetLocation::MediaBytes { byte_len: 0, .. }))
        );
        bounded(&result);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cdda_is_geometry_without_a_media_byte_range() {
    let mut report = ScanReport::new(
        "cdda",
        1,
        crate::detectors::CDDA,
        &[],
        crate::MediaIdentity {
            system: "pce_cd",
            byte_len: 0,
            sha256: None,
        },
        ScanLimits::default(),
    );
    report.cdda_tracks.push(crate::cdda::CdAudioTrack {
        number: 2,
        index1_lba: 150,
        end_lba: 300,
        pregap_start_lba: Some(0),
        sectors: 150,
        pcm_frames: 150 * 588,
    });
    let result = graph(&report, SongId::Cdda(0));
    assert_eq!(result.status, GraphStatus::Complete);
    assert!(matches!(
        result.nodes[0].location,
        Some(AssetLocation::DiscTrack {
            index1_lba: 150,
            ..
        })
    ));
}
