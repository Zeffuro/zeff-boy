use std::io::Read;

use super::*;

fn request(number: u32, pack: bool) -> GsfExportRequest {
    let input = ScanInput {
        system: Some(zeff_emu_common::system::System::Gba),
        standalone_audio: None,
        bytes: super::super::test_support::gba_fixture().into(),
        cdda: None,
        provenance: None,
        analysis_profile: "gsf-publication-test",
        display_name: Some("Test Game".to_owned()),
    };
    let manifest = input.analyze(Default::default(), &AtomicBool::new(false));
    GsfExportRequest {
        driver: driver::synthetic_driver(&input.bytes, number).unwrap(),
        sha256: zeff_firmware::sha256_hex(&input.bytes),
        bytes: input.bytes,
        song: manifest.scan.candidates[0].clone(),
        options: RenderOptions::default(),
        metadata: json!({"schema": "gsf-publication-test"}),
        game: "Test Game".to_owned(),
        mini_filename: format!("Test Game - {number:03}.minigsf"),
        pack,
    }
}

pub(super) fn decode(data: &[u8]) -> (u32, u32, Vec<u8>, String) {
    assert_eq!(&data[..8], b"PSF\x22\0\0\0\0");
    let compressed_len = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    let compressed = &data[16..16 + compressed_len];
    assert_eq!(
        crc32fast::hash(compressed),
        u32::from_le_bytes(data[12..16].try_into().unwrap())
    );
    let mut executable = Vec::new();
    flate2::read::ZlibDecoder::new(compressed)
        .read_to_end(&mut executable)
        .unwrap();
    let entry = u32::from_le_bytes(executable[..4].try_into().unwrap());
    let address = u32::from_le_bytes(executable[4..8].try_into().unwrap());
    assert_eq!(
        u32::from_le_bytes(executable[8..12].try_into().unwrap()) as usize,
        executable.len() - 12
    );
    (
        entry,
        address,
        executable[12..].to_vec(),
        String::from_utf8(data[16 + compressed_len..].to_vec()).unwrap(),
    )
}

#[test]
fn published_minigsf_reconstructs_standalone_and_reuses_library_across_songs() -> Result<()> {
    let directory = crate::test_support::test_directory("gsf-publication")?;
    let cancel = AtomicBool::new(false);
    let progress = AtomicU32::new(0);
    let mut prior_library = None;
    for number in [7, 257] {
        let full_path = directory.path().join(format!("{number}.gsf"));
        request(number, false).write_new(&full_path, &cancel, &progress)?;
        let original = std::fs::read(&full_path)?;
        let (entry, address, executable, tags) = decode(&original);
        assert_eq!(address, 0x0800_0000);
        assert!(tags.contains(&format!("title=Song {number}\n")));
        assert!(
            request(number, false)
                .write_new(&full_path, &cancel, &progress)
                .is_err()
        );
        assert_eq!(std::fs::read(&full_path)?, original);

        let pack_path = directory.path().join(format!("{number}.zip"));
        request(number, true).write_new(&pack_path, &cancel, &progress)?;
        let mut pack = zip::ZipArchive::new(std::fs::File::open(pack_path)?)?;
        assert_eq!(pack.len(), 3);
        let metadata: Value = serde_json::from_reader(pack.by_name("manifest.json")?)?;
        let library_name = metadata["library"]["path"].as_str().unwrap();
        let mut library = Vec::new();
        pack.by_name(library_name)?.read_to_end(&mut library)?;
        assert_eq!(
            metadata["library"]["sha256"],
            zeff_firmware::sha256_hex(&library)
        );
        if let Some(prior) = &prior_library {
            assert_eq!(prior, &library);
        }
        prior_library = Some(library.clone());
        let (lib_entry, lib_address, mut reconstructed, _) = decode(&library);
        let mut mini = Vec::new();
        pack.by_name(&format!("Test Game - {number:03}.minigsf"))?
            .read_to_end(&mut mini)?;
        let (mini_entry, mini_address, overlay, mini_tags) = decode(&mini);
        assert_eq!(
            (lib_entry, mini_entry, lib_address),
            (entry, entry, address)
        );
        assert_eq!(overlay, number.to_le_bytes());
        assert!(mini_tags.contains(&format!("_lib={library_name}\n")));
        let offset = (mini_address - lib_address) as usize;
        reconstructed[offset..offset + 4].copy_from_slice(&overlay);
        assert_eq!(reconstructed, executable);
    }
    Ok(())
}

#[test]
fn unsupported_driver_stale_bytes_and_cancellation_never_publish() -> Result<()> {
    let directory = crate::test_support::test_directory("gsf-rejection")?;
    let path = directory.path().join("rejected.gsf");
    let progress = AtomicU32::new(0);
    assert!(
        request(7, false)
            .write_new(&path, &AtomicBool::new(true), &progress)
            .is_err()
    );
    let mut stale = request(7, false);
    stale.sha256 = "stale".to_owned();
    assert!(
        stale
            .write_new(&path, &AtomicBool::new(false), &progress)
            .is_err()
    );
    let input = ScanInput {
        system: Some(zeff_emu_common::system::System::Gba),
        standalone_audio: None,
        bytes: super::super::test_support::gba_fixture().into(),
        cdda: None,
        provenance: None,
        analysis_profile: "gsf-rejection-test",
        display_name: None,
    };
    let manifest = input.analyze(Default::default(), &AtomicBool::new(false));
    assert!(!available(&input, &manifest, 0));
    assert!(
        GsfExportRequest::prepare(
            &input,
            &manifest,
            0,
            SongFormat::Gsf,
            RenderOptions::default()
        )
        .is_err()
    );
    assert!(!path.exists());
    assert_eq!(directory.path().read_dir()?.count(), 0);
    Ok(())
}
