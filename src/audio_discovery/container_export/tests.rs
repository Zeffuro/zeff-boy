use std::io::Read;

use super::*;
use crate::audio_discovery::{
    ScanLimits, catalog::SongId, export::SongExportRequest, media::SourceIdentity,
    render::RenderOptions, rips::RipFormat, test_support::rips::fixture,
};

fn input(format: RipFormat) -> ScanInput {
    let mut bytes = fixture(format);
    if format == RipFormat::Nsf {
        bytes[0x7d..0x80].copy_from_slice(&[0x50, 0, 0]);
        bytes.extend_from_slice(b"opaque metadata");
    }
    let source = SourceIdentity {
        kind: "rip-test",
        sha256: zeff_firmware::sha256_hex(&bytes),
        len: bytes.len(),
        container: None,
        selected_member: None,
    };
    ScanInput::standalone(
        bytes,
        source,
        StandaloneFormat::Rip(format),
        Some("Fixture".to_owned()),
    )
}

#[test]
fn native_and_asset_exports_preserve_complete_source_and_physical_spans() -> Result<()> {
    let directory = tempfile::tempdir()?;
    for rip_format in [RipFormat::Gbs, RipFormat::Nsf] {
        let input = input(rip_format);
        let manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
        assert_eq!(manifest.scan.song_count(), 1);
        assert_eq!(manifest.scan.song_at_offset(0)?, SongId::Rip(0));
        assert_eq!(
            manifest
                .scan
                .song(SongId::Rip(0))
                .unwrap()
                .span()
                .unwrap()
                .canonical_cpu_address,
            None
        );
        let native = if rip_format == RipFormat::Gbs {
            SongFormat::Gbs
        } else {
            SongFormat::Nsf
        };
        for format in [native, SongFormat::MappedAssets] {
            let path = directory.path().join(format!(
                "{}-{}.{}",
                rip_format.extension(),
                format.info().id,
                format.info().extension
            ));
            let prepare = || {
                SongExportRequest::prepare(
                    &input,
                    &manifest,
                    SongId::Rip(0),
                    format,
                    RenderOptions::default(),
                )
            };
            prepare()?.write_new(&path, &AtomicBool::new(false), &AtomicU32::new(0))?;
            let bytes = std::fs::read(&path)?;
            if format == native {
                assert_eq!(bytes.as_slice(), input.bytes.as_ref());
            } else {
                let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes))?;
                let mut source = Vec::new();
                zip.by_name(&format!("source.{}", rip_format.extension()))?
                    .read_to_end(&mut source)?;
                assert_eq!(source.as_slice(), input.bytes.as_ref());
                let metadata: Value = serde_json::from_reader(zip.by_name("manifest.json")?)?;
                assert_eq!(metadata["detector_outcomes"][0]["retained_matches"], 1);
                for asset in metadata["assets"].as_array().unwrap() {
                    let mut data = Vec::new();
                    zip.by_name(asset["path"].as_str().unwrap())?
                        .read_to_end(&mut data)?;
                    let start = asset["span"]["offset"].as_u64().unwrap() as usize;
                    let len = asset["span"]["byte_len"].as_u64().unwrap() as usize;
                    assert_eq!(data.as_slice(), &input.bytes[start..start + len]);
                    assert_eq!(asset["sha256"], zeff_firmware::sha256_hex(&data));
                }
                assert_eq!(
                    zip.by_name("metadata.bin").is_ok(),
                    rip_format == RipFormat::Nsf
                );
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
fn export_fails_closed_for_stale_inventory_identity_source_and_cancellation() -> Result<()> {
    let directory = tempfile::tempdir()?;
    for format in [RipFormat::Gbs, RipFormat::Nsf] {
        let mut input = input(format);
        let mut manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
        let path = directory.path().join(format.extension());
        let prepare = |input: &ScanInput, manifest: &ScanManifest| {
            SongExportRequest::prepare(
                input,
                manifest,
                SongId::Rip(0),
                SongFormat::MappedAssets,
                RenderOptions::default(),
            )
        };
        assert!(
            prepare(&input, &manifest)?
                .write_new(&path, &AtomicBool::new(true), &AtomicU32::new(0))
                .is_err()
        );
        manifest.scan.music_rips[0].program.byte_len = u32::MAX;
        assert!(
            prepare(&input, &manifest)?
                .write_new(&path, &AtomicBool::new(false), &AtomicU32::new(0))
                .is_err()
        );
        manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
        manifest.scan.media.sha256 = Some("00".repeat(32));
        assert!(prepare(&input, &manifest).is_err());
        manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
        Arc::make_mut(&mut input.bytes)[0] ^= 1;
        assert!(
            prepare(&input, &manifest)?
                .write_new(&path, &AtomicBool::new(false), &AtomicU32::new(0))
                .is_err()
        );
        input.standalone_audio = Some(StandaloneFormat::Vgm);
        assert!(prepare(&input, &manifest).is_err());
        assert!(!path.exists());
    }
    Ok(())
}
