use std::io::{Read, Write};

use super::*;
use crate::audio_discovery::{
    ScanLimits, catalog::SongId, export::SongExportRequest, media::SourceIdentity,
    render::RenderOptions,
};

pub(crate) fn fixture(gzip: bool) -> Vec<u8> {
    let mut bytes = vec![0; 0x40];
    bytes[..4].copy_from_slice(b"Vgm ");
    bytes[8..12].copy_from_slice(&0x150u32.to_le_bytes());
    bytes[0x0c..0x10].copy_from_slice(&3_579_545u32.to_le_bytes());
    bytes[0x18..0x1c].copy_from_slice(&735u32.to_le_bytes());
    bytes[0x28..0x2a].copy_from_slice(&9u16.to_le_bytes());
    bytes[0x2a] = 16;
    bytes.extend_from_slice(&[0x50, 0x8f, 0x50, 0x0f, 0x50, 0x90, 0x62, 0x66]);
    let eof = bytes.len() as u32 - 4;
    bytes[4..8].copy_from_slice(&eof.to_le_bytes());
    bytes.extend_from_slice(b"unparsed tail");
    if !gzip {
        return bytes;
    }
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&bytes).unwrap();
    encoder.finish().unwrap()
}

fn input(gzip: bool) -> ScanInput {
    let bytes = fixture(gzip);
    let source = SourceIdentity {
        kind: "vgm-test",
        sha256: zeff_firmware::sha256_hex(&bytes),
        len: bytes.len(),
        container: None,
        selected_member: None,
    };
    ScanInput::standalone(
        bytes,
        source,
        StandaloneFormat::Vgm,
        Some("Fixture".to_owned()),
    )
}

#[test]
fn raw_and_gzip_exports_preserve_source_logical_bytes_and_provenance() -> Result<()> {
    let directory = tempfile::tempdir()?;
    for gzip in [false, true] {
        let input = input(gzip);
        let manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
        assert_eq!(manifest.scan.song_count(), 1);
        let id = manifest.scan.song_at_offset(0)?;
        assert_eq!(id, SongId::Vgm(0));
        assert_eq!(
            manifest.scan.song(id).unwrap().supports(SongFormat::Vgz),
            gzip
        );
        for format in [SongFormat::Vgm, SongFormat::Vgz, SongFormat::MappedAssets] {
            if format == SongFormat::Vgz && !gzip {
                continue;
            }
            let path = directory.path().join(format!(
                "{gzip}-{}.{}",
                format.info().id,
                format.info().extension
            ));
            let prepare = || {
                SongExportRequest::prepare(&input, &manifest, id, format, RenderOptions::default())
            };
            prepare()?.write_new(&path, &AtomicBool::new(false), &AtomicU32::new(0))?;
            let bytes = std::fs::read(&path)?;
            if format == SongFormat::MappedAssets {
                let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes))?;
                let mut original = Vec::new();
                zip.by_name(if gzip { "source.vgz" } else { "source.vgm" })?
                    .read_to_end(&mut original)?;
                assert_eq!(original.as_slice(), input.bytes.as_ref());
                if gzip {
                    let mut logical = Vec::new();
                    zip.by_name("decoded.vgm")?.read_to_end(&mut logical)?;
                    assert_eq!(logical, fixture(false));
                }
                let metadata: Value = serde_json::from_reader(zip.by_name("manifest.json")?)?;
                assert_eq!(
                    metadata["source"]["sha256"],
                    manifest.source.as_ref().unwrap().sha256
                );
                assert_eq!(
                    metadata["log"]["logical"]["sha256"],
                    zeff_firmware::sha256_hex(&fixture(false))
                );
            } else {
                assert_eq!(bytes, fixture(format == SongFormat::Vgz));
            }
            assert!(
                prepare()?
                    .write_new(&path, &AtomicBool::new(false), &AtomicU32::new(0))
                    .is_err()
            );
            assert_eq!(std::fs::read(&path)?, bytes);
        }
    }
    Ok(())
}

#[test]
fn vgm_export_rejects_stale_identity_inventory_wrong_source_and_cancellation() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut input = input(true);
    let mut manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
    let prepare = |input: &ScanInput, manifest: &ScanManifest| {
        SongExportRequest::prepare(
            input,
            manifest,
            SongId::Vgm(0),
            SongFormat::Vgm,
            RenderOptions::default(),
        )
    };
    let path = directory.path().join("must-not-exist.vgm");
    assert!(
        prepare(&input, &manifest)?
            .write_new(&path, &AtomicBool::new(true), &AtomicU32::new(0))
            .is_err()
    );
    manifest.scan.vgm_logs[0].samples += 1;
    assert!(
        prepare(&input, &manifest)?
            .write_new(&path, &AtomicBool::new(false), &AtomicU32::new(0))
            .is_err()
    );
    manifest.scan.vgm_logs[0].samples -= 1;
    input.standalone_audio = Some(StandaloneFormat::Tracker(
        crate::audio_discovery::tracker::EmbeddedFormat::Xm,
    ));
    assert!(prepare(&input, &manifest).is_err());
    input.standalone_audio = Some(StandaloneFormat::Vgm);
    let request = prepare(&input, &manifest)?;
    manifest.scan.media.sha256 = Some("0".repeat(64));
    assert!(
        prepare(&input, &manifest)?
            .write_new(&path, &AtomicBool::new(false), &AtomicU32::new(0))
            .is_err()
    );
    assert!(
        request
            .write_new(&path, &AtomicBool::new(true), &AtomicU32::new(0))
            .is_err()
    );
    assert!(!path.exists());
    Ok(())
}
