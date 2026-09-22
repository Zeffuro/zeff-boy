use std::io::{Cursor, Read};
use std::sync::atomic::AtomicBool;

use serde_json::Value;

use super::*;

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn psglib_options_require_offsets_and_projection_rate() -> Result<()> {
    let base = ["--audio-psglib", "new-psglib.zip", "source.sms"];
    assert!(parse(&args(&base)).is_err());
    let mut good = args(&base);
    good.extend(args(&[
        "--psglib-rate",
        "60",
        "--psglib-offset",
        "0x80",
        "--psglib-offset",
        "256",
    ]));
    let parsed = parse(&good)?.unwrap();
    assert_eq!(parsed.offsets, [128, 256]);
    assert_eq!(parsed.rate, FrameRate::Hz60);
    for extra in [
        vec!["--psglib-rate", "50"],
        vec!["--psglib-offset", "128"],
        vec!["--psglib-offset"],
        vec!["--audio-discover", "x"],
    ] {
        let mut bad = good.clone();
        bad.extend(args(&extra));
        assert!(parse(&bad).is_err());
    }
    good[4] = "59".into();
    assert!(parse(&good).is_err());
    good[4] = "50".into();
    assert!(parse(&good)?.is_some());
    good[2] = "new-psglib.zip".into();
    assert!(parse(&good).is_err());
    assert!(parse(&args(&["--psglib-rate", "60"])).is_err());
    let auto = args(&[
        "--audio-psglib",
        "auto.zip",
        "source.sms",
        "--psglib-rate",
        "60",
        "--psglib-auto",
    ]);
    assert!(parse(&auto)?.unwrap().automatic);
    for extra in [vec!["--psglib-offset", "128"], vec!["--psglib-auto"]] {
        let mut bad = auto.clone();
        bad.extend(args(&extra));
        assert!(parse(&bad).is_err());
    }
    assert!(parse(&args(&["--psglib-auto"])).is_err());
    Ok(())
}

#[test]
fn psglib_bundle_retains_source_events_and_playable_vgm_without_discovery_claims() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("two.sms");
    let mut source = vec![0xee; 256];
    source[32..37].copy_from_slice(&[0x80, 0x42, 0x90, 0x3f, 0]);
    source[64..69].copy_from_slice(&[0xa4, 0x44, 0xb2, 0x39, 0]);
    std::fs::write(&input, &source)?;
    let request = Request {
        input,
        output: dir.path().join("songs.zip"),
        offsets: vec![32, 64],
        automatic: false,
        rate: FrameRate::Hz60,
    };
    run(&request)?;
    let original = std::fs::read(&request.output)?;
    let mut zip = zip::ZipArchive::new(Cursor::new(&original))?;
    let mut manifest = String::new();
    zip.by_name("manifest.json")?
        .read_to_string(&mut manifest)?;
    let manifest: Value = serde_json::from_str(&manifest)?;
    assert_eq!(
        manifest["source"]["sha256"],
        zeff_firmware::sha256_hex(&source)
    );
    assert_eq!(manifest["streams"].as_array().unwrap().len(), 2);
    assert_eq!(manifest["projection"]["frame_hz"], 60);
    assert!(
        manifest["limitations"][0]
            .as_str()
            .unwrap()
            .contains("does not discover")
    );
    let cancel = AtomicBool::new(false);
    for (index, offset) in [32, 64].into_iter().enumerate() {
        let item = &manifest["streams"][index];
        let mut bytes = Vec::new();
        zip.by_name(item["vgm"]["path"].as_str().unwrap())?
            .read_to_end(&mut bytes)?;
        assert_eq!(item["vgm"]["sha256"], zeff_firmware::sha256_hex(&bytes));
        let log = zeff_audio_discovery::vgm::inspect(&bytes, Default::default(), &cancel)
            .unwrap()
            .unwrap();
        let pcm = crate::audio_discovery::pcm::song::PcmSong::Vgm(Box::new(log));
        let mut session = pcm.session(
            &bytes,
            crate::audio_discovery::render::RenderOptions {
                sample_rate: 48000,
                max_seconds: 1,
                fade_seconds: 0,
                loops: 1,
                ..Default::default()
            },
            &cancel,
        )?;
        let mut frames = 0;
        let mut nonzero = false;
        let mut buffer = [0; 1024];
        loop {
            let count = session.read(&mut buffer, &cancel)?;
            if count == 0 {
                break;
            }
            frames += count / 2;
            nonzero |= buffer[..count].iter().any(|&sample| sample != 0);
        }
        assert_eq!(frames, if index == 0 { 7200 } else { 2400 });
        assert!(nonzero);
        let mut retained = Vec::new();
        zip.by_name(&format!("stream-{offset:08x}/source-{offset:08x}.bin"))?
            .read_to_end(&mut retained)?;
        assert_eq!(retained, source[offset..offset + 5]);
    }
    assert!(run(&request).is_err());
    assert_eq!(std::fs::read(&request.output)?, original);
    assert_eq!(std::fs::read(&request.input)?, source);
    let duplicate = dir.path().join("duplicate.zip");
    assert!(
        crate::audio_discovery::psglib_export::write_new(
            &duplicate,
            &source,
            crate::audio_discovery::psglib_export::Selection::Offsets(&[32, 32]),
            FrameRate::Hz60,
            &cancel,
            &AtomicU32::new(0),
        )
        .is_err()
    );
    assert!(!duplicate.exists());
    let bad = Request {
        output: dir.path().join("invalid.zip"),
        offsets: vec![255],
        ..request
    };
    assert!(run(&bad).is_err());
    assert!(!bad.output.exists());
    Ok(())
}

#[test]
fn psglib_auto_empty_scan_publishes_evidence_without_exportable_streams() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("empty.sms");
    std::fs::write(&input, vec![0; 0x8000])?;
    let request = Request {
        input,
        output: dir.path().join("evidence.zip"),
        offsets: Vec::new(),
        automatic: true,
        rate: FrameRate::Hz60,
    };
    run(&request)?;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&request.output)?)?;
    assert_eq!(zip.len(), 1);
    let manifest: Value = serde_json::from_reader(zip.by_name("manifest.json")?)?;
    assert_eq!(manifest["selection"]["kind"], "static_calls");
    assert_eq!(manifest["selection"]["system"], "sms");
    assert_eq!(
        manifest["selection"]["mapping"],
        "reset_initial_identity_32k"
    );
    assert!(manifest["streams"].as_array().unwrap().is_empty());
    assert!(
        manifest["selection"]["discovery"]["bound"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        manifest["limitations"][0]
            .as_str()
            .unwrap()
            .contains("does not prove")
    );
    let invalid_system = Request {
        input: dir.path().join("empty.gb"),
        output: dir.path().join("bad.zip"),
        ..request
    };
    std::fs::write(&invalid_system.input, vec![0; 0x8000])?;
    assert!(run(&invalid_system).is_err());
    assert!(!invalid_system.output.exists());
    Ok(())
}

#[test]
fn psglib_auto_banked_inputs_remain_evidence_only_and_cancellation_publishes_nothing() -> Result<()>
{
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("banked.gg");
    let source = vec![0; 0x10000];
    std::fs::write(&input, &source)?;
    let request = Request {
        input,
        output: dir.path().join("banked.zip"),
        offsets: Vec::new(),
        automatic: true,
        rate: FrameRate::Hz50,
    };
    run(&request)?;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&request.output)?)?;
    assert_eq!(zip.len(), 1);
    let manifest: Value = serde_json::from_reader(zip.by_name("manifest.json")?)?;
    assert_eq!(manifest["selection"]["system"], "gg");
    assert_eq!(
        manifest["selection"]["discovery"]["held"][0]["kind"],
        "unsupported_mapping"
    );
    assert!(manifest["streams"].as_array().unwrap().is_empty());
    let cancelled = dir.path().join("cancelled.zip");
    assert!(
        crate::audio_discovery::psglib_export::write_new(
            &cancelled,
            &source,
            crate::audio_discovery::psglib_export::Selection::Automatic(
                zeff_emu_common::system::System::Gg
            ),
            FrameRate::Hz50,
            &AtomicBool::new(true),
            &AtomicU32::new(0),
        )
        .is_err()
    );
    assert!(!cancelled.exists());
    Ok(())
}
