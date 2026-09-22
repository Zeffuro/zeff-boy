use super::*;

#[test]
fn structural_driver_evidence_survives_archive_loading_without_song_selection() -> anyhow::Result<()>
{
    for source in [
        zeff_audio_discovery::drivers::nes_tose_structure::synthetic_rom(),
        zeff_audio_discovery::drivers::nes_sound_writes::synthetic_rom(),
    ] {
        let directory = tempfile::tempdir()?;
        let archive = directory.path().join("source.zip");
        crate::test_support::write_zip(&archive, &[("selected.nes", &source)])?;
        let original = std::fs::read(&archive)?;
        let output = directory.path().join("scan.json");
        let evidence = directory.path().join("evidence.json");
        let request = AudioDiscoveryRequest {
            input_path: archive.clone(),
            output_path: output.clone(),
            archive_member: Some("selected.nes".into()),
            max_work: None,
            max_candidates: None,
            export: None,
            relations: None,
            driver_evidence: Some(evidence.clone()),
        };
        run_request(&request)?;
        let scan: serde_json::Value = serde_json::from_slice(&std::fs::read(output)?)?;
        let sidecar: serde_json::Value = serde_json::from_slice(&std::fs::read(evidence)?)?;
        assert_eq!(scan["source"], sidecar["source"]);
        assert_eq!(scan["scan"]["media"], sidecar["driver_evidence"]["media"]);
        assert_eq!(
            scan["scan"]["driver_candidates"],
            sidecar["driver_evidence"]["driver_candidates"]
        );
        let candidates = scan["scan"]["driver_candidates"].as_array().unwrap();
        assert_eq!(candidates.len(), 1);
        if candidates[0]["qualification"] == "structural" {
            assert!(
                !candidates[0]["inventory"]["entries"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        } else {
            assert_eq!(candidates[0]["qualification"], "static_code");
            assert!(candidates[0].get("inventory").is_none());
        }
        assert!(scan["scan"].get("nes_tose_songs").is_none());
        assert_eq!(std::fs::read(archive)?, original);
    }
    Ok(())
}
