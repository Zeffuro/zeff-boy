use super::*;
use crate::audio_discovery::{
    extract::ExtractionRequest,
    formats::AudioFormat,
    pcm::song::PcmSong,
    relations::{AssetKind, GraphStatus, Relation},
};
use zeff_audio_discovery::SourceSpan;

#[test]
fn sega_selector_graph_and_mapped_export_preserve_bank_addresses() -> Result<()> {
    let cancel = AtomicBool::new(false);
    let input = input(System::Sms, zeff_audio_discovery::sega_psg::fixture_rom());
    let manifest = input.analyze(Default::default(), &cancel);
    let id = SongId::SegaPsg(0);
    let selected = manifest.scan.song(id).context("synthetic Sega song")?;
    assert!(PcmSong::can_play(selected) && PcmSong::is_native(selected));
    assert!(selected.supports(SongFormat::MappedAssets));
    assert!(!selected.supports(SongFormat::Midi));
    for format in [AudioFormat::Wav, AudioFormat::Flac, AudioFormat::Ogg] {
        assert!(selected.supports(SongFormat::Audio(format)));
        let request = SongExportRequest::prepare(
            &input,
            &manifest,
            id,
            SongFormat::Audio(format),
            Default::default(),
        );
        if format == AudioFormat::Ogg && !cfg!(feature = "audio-recording") {
            assert!(request.is_err());
        } else {
            request?;
        }
    }
    assert_eq!(manifest.scan.song_at_offset(0x8100)?, id);
    assert_eq!(selected.span().unwrap().canonical_cpu_address, Some(0x4100));
    let graph = manifest
        .scan
        .asset_relations(id, Default::default(), &cancel);
    assert_eq!(graph.status, GraphStatus::Complete);
    assert!(
        graph
            .nodes
            .iter()
            .any(|node| node.kind == AssetKind::EntryPoint)
    );
    assert!(
        graph
            .edges
            .iter()
            .any(|edge| edge.relation == Relation::Selects { index: 0x81 })
    );
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("selected.zip");
    let prepare = |source: &ScanInput| {
        SongExportRequest::prepare(
            source,
            &manifest,
            id,
            SongFormat::MappedAssets,
            Default::default(),
        )
    };
    prepare(&input)?.write_new(&path, &cancel, &AtomicU32::new(0))?;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&path)?)?;
    let mut original = Vec::new();
    archive
        .by_name("source/00008000-00008000.bin")?
        .read_to_end(&mut original)?;
    assert_eq!(original, input.bytes[0x8000..0x10000]);
    let metadata: Value = serde_json::from_reader(archive.by_name("manifest.json")?)?;
    assert_eq!(metadata["selection"]["engine"], "sega_psg");
    assert_eq!(metadata["selection"]["song"]["system"], "sms");
    assert_eq!(metadata["selection"]["song"]["raw_index"], 0x81);
    assert_eq!(metadata["selection"]["song"]["timing"], "ntsc");
    let span = selected.span().unwrap();
    ExtractionRequest::prepare(&input, &manifest, span, "Sega table")?;
    let mut forged = span;
    forged.canonical_cpu_address = Some(0x8100);
    assert!(ExtractionRequest::prepare(&input, &manifest, forged, "Sega table").is_err());
    forged = span;
    forged.effective_offset += 1;
    assert!(ExtractionRequest::prepare(&input, &manifest, forged, "Sega table").is_err());
    let mut bytes = input.bytes.to_vec();
    bytes[0x9000] ^= 1;
    let changed = super::input(System::Sms, bytes);
    let stale = directory.path().join("stale.zip");
    assert!(
        prepare(&changed)?
            .write_new(&stale, &cancel, &AtomicU32::new(0))
            .is_err()
    );
    assert!(!stale.exists());
    Ok(())
}

#[test]
fn sega_supplemental_data_survives_graph_extraction_and_asset_export() -> Result<()> {
    let cancel = AtomicBool::new(false);
    let input = input(
        System::Gg,
        zeff_audio_discovery::sega_psg::fixture_rom_supplemental(),
    );
    let manifest = input.analyze(Default::default(), &cancel);
    let id = SongId::SegaPsg(0);
    let song = &manifest.scan.sega_psg_songs[0];
    assert_eq!(song.mapped_spans.len(), 3);
    let graph = manifest
        .scan
        .asset_relations(id, Default::default(), &cancel);
    assert_eq!(graph.status, GraphStatus::Complete);
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("supplemental.zip");
    SongExportRequest::prepare(
        &input,
        &manifest,
        id,
        SongFormat::MappedAssets,
        Default::default(),
    )?
    .write_new(&path, &cancel, &AtomicU32::new(0))?;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    assert_eq!(archive.len(), 4);
    for span in &song.mapped_spans {
        let source = SourceSpan {
            effective_offset: span.effective_offset,
            byte_len: span.byte_len,
            canonical_cpu_address: Some(span.canonical_cpu_address),
        };
        ExtractionRequest::prepare(&input, &manifest, source, "mapped Sega data")?;
        let mut original = Vec::new();
        archive
            .by_name(&format!(
                "source/{:08x}-{:08x}.bin",
                span.effective_offset, span.byte_len
            ))?
            .read_to_end(&mut original)?;
        let start = span.effective_offset as usize;
        assert_eq!(original, input.bytes[start..start + span.byte_len as usize]);
        let encoded = serde_json::to_value(&graph)?;
        assert!(encoded["nodes"].as_array().unwrap().iter().any(|node| {
            node["location"]["offset"] == span.effective_offset
                && node["location"]["byte_len"] == span.byte_len
                && node["location"]["cpu_address"] == span.canonical_cpu_address
        }));
    }
    for span in [
        SourceSpan {
            effective_offset: 0x311,
            byte_len: 2,
            canonical_cpu_address: Some(0x311),
        },
        SourceSpan {
            effective_offset: 0x312,
            byte_len: 1,
            canonical_cpu_address: Some(0x312),
        },
        SourceSpan {
            effective_offset: 0x311,
            byte_len: 1,
            canonical_cpu_address: Some(0x8311),
        },
    ] {
        assert!(ExtractionRequest::prepare(&input, &manifest, span, "forged Sega data").is_err());
    }
    let metadata: Value = serde_json::from_reader(archive.by_name("manifest.json")?)?;
    assert_eq!(metadata["selection"]["song"], serde_json::to_value(song)?);
    Ok(())
}
