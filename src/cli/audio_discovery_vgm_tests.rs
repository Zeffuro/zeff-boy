use super::*;
use crate::audio_discovery::{ScanLimits, vgm_export::tests::fixture};

#[test]
fn vgm_and_vgz_direct_and_selected_zip_inputs_keep_source_identity() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-cli-vgm")?;
    for (extension, gzip) in [("vgm", false), ("vgz", true), ("vgm", true), ("vgz", false)] {
        let bytes = fixture(gzip);
        let label = format!("{extension}-{gzip}");
        let source = directory.path().join(format!("{label}.{extension}"));
        let archive = directory.path().join(format!("{label}.zip"));
        let member = format!("logs/{label}.{extension}");
        std::fs::write(&source, &bytes)?;
        crate::test_support::write_zip(
            &archive,
            &[(member.as_str(), &bytes), ("unselected.vgm", b"invalid")],
        )?;
        assert!(normalize_archive_member(&member).is_ok());
        for zipped in [false, true] {
            let output = directory.path().join(format!("{label}-{zipped}.vgm"));
            let request = AudioDiscoveryRequest {
                output_path: directory.path().join(format!("{label}-{zipped}.json")),
                input_path: if zipped {
                    archive.clone()
                } else {
                    source.clone()
                },
                archive_member: zipped.then(|| member.clone()),
                max_work: None,
                max_candidates: None,
                relations: None,
                export: Some(OfflineExport {
                    format: SongFormat::Vgm,
                    output_path: output.clone(),
                    selection: SongSelection::Offset(0),
                    options: RenderOptions::default(),
                    explicit: ExplicitExportSettings::default(),
                }),
            };
            let input = load_input(&request)?;
            assert_eq!(input.bytes.as_ref(), bytes);
            let manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
            assert_eq!(manifest.scan.song_count(), 1);
            assert_eq!(
                manifest.scan.media.sha256.as_deref(),
                Some(sha256_hex(&bytes).as_str())
            );
            assert_eq!(
                manifest.source.as_ref().unwrap().selected_member.is_some(),
                zipped
            );
            assert!(run_request(&request)?);
            assert_eq!(std::fs::read(output)?, fixture(false));
            let report: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&request.output_path)?)?;
            assert_eq!(report["scan"]["media"]["system"], "standalone_vgm");
            assert_eq!(
                report["scan"]["vgm_logs"][0]["logical"]["address_space"],
                if gzip {
                    "decompressed_vgm"
                } else {
                    "source_file"
                }
            );
            if zipped {
                assert_eq!(
                    report["source"]["container"]["sha256"],
                    sha256_hex(&std::fs::read(&archive)?)
                );
                assert_eq!(report["source"]["selected_member"]["name"], member);
            }
        }
    }
    Ok(())
}
