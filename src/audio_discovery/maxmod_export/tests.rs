use std::io::{Cursor, Read};

use super::*;

fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; 0x100];
    bytes.extend_from_slice(&[
        1, 0, 0, 0, b'*', b'm', b'a', b'x', b'm', b'o', b'd', b'*', 16, 0, 0, 0, 32, 0, 0, 0, 1,
        0x18, 1, 0xba, 16, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0xba, 8, 2,
    ]);
    bytes.extend_from_slice(&[
        128, 144, 160, 176, 192, 176, 160, 144, 128, 112, 96, 80, 64, 80, 96, 112,
    ]);
    bytes.extend_from_slice(&[128; 4]);
    bytes
}

#[test]
fn preserves_bank_and_unsigned_sample_with_source_spans() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("assets.zip");
    let mut source = fixture();
    let bank_end = source.len();
    source.extend_from_slice(&[0x42; 17]);
    write_new(&path, &source, &AtomicBool::new(false))?;
    let output = std::fs::read(&path)?;
    let mut zip = zip::ZipArchive::new(Cursor::new(&output))?;
    assert_eq!(zip.len(), 3);
    let mut read = |name: &str| -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        zip.by_name(name)?.read_to_end(&mut bytes)?;
        Ok(bytes)
    };
    assert_eq!(read("bank.bin")?, source[0x100..bank_end]);
    assert_eq!(read("sample.u8")?, source[0x124..0x134]);
    let manifest: serde_json::Value = serde_json::from_slice(&read("manifest.json")?)?;
    assert_eq!(
        manifest["source"]["sha256"],
        zeff_firmware::sha256_hex(&source)
    );
    assert_eq!(manifest["bank"]["payload"]["offset"], 0x124);
    assert_eq!(manifest["bank"]["frequency_code"], 520);
    assert!(write_new(&path, &source, &AtomicBool::new(false)).is_err());
    assert_eq!(std::fs::read(&path)?, output);
    Ok(())
}

#[test]
fn rejects_unsupported_or_cancelled_source_without_publication() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let source = fixture();
    let path = directory.path().join("assets.zip");
    assert!(write_new(&path, &source, &AtomicBool::new(true)).is_err());
    assert!(!path.exists());
    let mut ds = source;
    ds[0x114] = 2;
    assert!(write_new(&path, &ds, &AtomicBool::new(false)).is_err());
    assert!(!path.exists());
    Ok(())
}

#[test]
fn refuses_overlapping_bank_interpretations() -> Result<()> {
    let inner = fixture().split_off(0x100);
    let mut source = inner[..36].to_vec();
    source[16..20].copy_from_slice(&68u32.to_le_bytes());
    source[24..28].copy_from_slice(&52u32.to_le_bytes());
    source.extend_from_slice(&inner);
    source.extend_from_slice(&[0x42; 9]);
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("ambiguous.zip");
    assert_eq!(
        zeff_audio_discovery::maxmod::discover(
            &source,
            zeff_audio_discovery::ScanLimits::default(),
            &AtomicBool::new(false),
        )
        .unwrap()
        .len(),
        2
    );
    assert!(write_new(&path, &source, &AtomicBool::new(false)).is_err());
    assert!(!path.exists());
    Ok(())
}

#[test]
fn refuses_disjoint_banks_without_publication() -> Result<()> {
    let mut source = fixture();
    source.extend_from_slice(&[0x42; 12]);
    source.extend_from_slice(&fixture()[0x100..]);
    source.extend_from_slice(&[0x43; 5]);
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("ambiguous.zip");
    assert!(write_new(&path, &source, &AtomicBool::new(false)).is_err());
    assert!(!path.exists());
    Ok(())
}

fn multiple_samples() -> Vec<u8> {
    let mut bytes = vec![0x42; 32];
    bytes.extend_from_slice(&[2, 0, 0, 0, b'*', b'm', b'a', b'x', b'm', b'o', b'd', b'*']);
    bytes.extend_from_slice(&20u32.to_le_bytes());
    bytes.extend_from_slice(&48u32.to_le_bytes());
    for (payload, frequency) in [(&[1, 2, 3][..], 520u16), (&[0, 255, 128, 127, 1][..], 260)] {
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0xba);
        }
        bytes.extend_from_slice(&(payload.len() as u32 + 16).to_le_bytes());
        bytes.extend_from_slice(&[1, 0x18, 1, 0xba]);
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&[0, 0xba]);
        bytes.extend_from_slice(&frequency.to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes.extend_from_slice(&[0x80; 4]);
    }
    bytes.extend_from_slice(&[0x43; 17]);
    bytes
}

#[test]
fn exports_each_sample_in_table_order_with_exact_raw_bytes_and_identity() -> Result<()> {
    let source = multiple_samples();
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("multiple.zip");
    write_new(&path, &source, &AtomicBool::new(false))?;
    let mut archive = zip::ZipArchive::new(Cursor::new(std::fs::read(&path)?))?;
    assert_eq!(archive.len(), 4);
    let mut read = |name: &str| -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        archive.by_name(name)?.read_to_end(&mut bytes)?;
        Ok(bytes)
    };
    assert_eq!(read("bank.bin")?, source[32..109]);
    assert_eq!(read("samples/0000.u8")?, [1, 2, 3]);
    assert_eq!(read("samples/0001.u8")?, [0, 255, 128, 127, 1]);
    let manifest: serde_json::Value = serde_json::from_slice(&read("manifest.json")?)?;
    assert_eq!(manifest["schema"], "zeff-maxmod-sample-assets/2");
    assert_eq!(manifest["profile"], "msl_gba_nonlooping_sfx_v18");
    assert_eq!(
        manifest["source"]["sha256"],
        zeff_firmware::sha256_hex(&source)
    );
    assert_eq!(manifest["source"]["byte_len"], source.len());
    assert_eq!(
        manifest["bank"]["table"],
        json!({"offset": 44, "byte_len": 8})
    );
    for (index, (offset, length, frequency)) in
        [(72, 3, 520), (100, 5, 260)].into_iter().enumerate()
    {
        assert_eq!(
            manifest["bank"]["samples"][index]["payload"],
            json!({"offset": offset, "byte_len": length})
        );
        assert_eq!(
            manifest["bank"]["samples"][index]["frequency_code"],
            frequency
        );
    }
    for artifact in manifest["artifacts"].as_array().unwrap() {
        let bytes = read(artifact["path"].as_str().unwrap())?;
        assert_eq!(artifact["byte_len"], bytes.len());
        assert_eq!(artifact["sha256"], zeff_firmware::sha256_hex(&bytes));
    }
    Ok(())
}

#[test]
fn invalid_later_sample_or_padding_does_not_publish_a_partial_bundle() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("invalid.zip");
    for offset in [32 + 47, 32 + 48 + 4, 32 + 76] {
        let mut source = multiple_samples();
        source[offset] = 0;
        assert!(write_new(&path, &source, &AtomicBool::new(false)).is_err());
        assert!(!path.exists());
    }
    Ok(())
}
