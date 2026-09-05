use super::*;
use crate::emu_backend::pce_cd::{build_disc, parse_cue_bytes};
use crate::patching::apply_ppf_patch_segments;

fn temp_file(name: &str, bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "zeff-pce-cd-file-{}-{name}.bin",
        std::process::id()
    ));
    std::fs::write(&path, bytes).unwrap();
    path
}

fn ppf1(records: &[(u32, &[u8])]) -> Vec<u8> {
    let mut patch = b"PPF10\0".to_vec();
    patch.resize(56, 0);
    for (offset, bytes) in records {
        patch.extend_from_slice(&offset.to_le_bytes());
        patch.push(bytes.len() as u8);
        patch.extend_from_slice(bytes);
    }
    patch
}

fn ppf3(records: &[(u64, &[u8])], block: &[u8]) -> Vec<u8> {
    let mut patch = b"PPF30\x02".to_vec();
    patch.resize(56, 0);
    patch.extend_from_slice(&[0, 1, 0, 0]);
    patch.extend_from_slice(block);
    for (offset, bytes) in records {
        patch.extend_from_slice(&offset.to_le_bytes());
        patch.push(bytes.len() as u8);
        patch.extend_from_slice(bytes);
    }
    patch
}

#[test]
fn audio_samples_share_one_file_sector_read() {
    let mut audio = vec![0; 2 * 2_352];
    for sample in 0..588 {
        let offset = sample * 4;
        audio[offset..offset + 2].copy_from_slice(&(sample as i16).to_le_bytes());
    }
    let path = temp_file("audio", &audio);
    let metadata = std::fs::metadata(&path).unwrap();
    let source = FileSliceSource::open(
        &path,
        FileIdentity::from_metadata(&metadata),
        0,
        audio.len(),
        2_352,
        false,
    )
    .unwrap();
    let track_source: Arc<dyn CdTrackSource> = source.clone();
    let disc = CdDisc::new(vec![
        CdTrack::from_index1_source(1, 0, None, 0, CdTrackMode::Audio, track_source).unwrap(),
    ])
    .unwrap();
    source.reset_cache_for_test();
    for sample in 0..588 {
        assert_eq!(disc.read_audio_sample(0, sample).unwrap().0, sample as i16);
    }
    assert_eq!(source.read_count(), 1);
}

#[test]
fn disc_hash_streams_file_sources_without_sector_reads() {
    let bytes = (0..512 * 2_048)
        .map(|index| index as u8)
        .collect::<Vec<_>>();
    let path = temp_file("stream-hash", &bytes);
    let metadata = std::fs::metadata(&path).unwrap();
    let source = FileSliceSource::open(
        &path,
        FileIdentity::from_metadata(&metadata),
        0,
        bytes.len(),
        2_048,
        false,
    )
    .unwrap();
    source.reset_cache_for_test();
    let track_source: Arc<dyn CdTrackSource> = source.clone();

    CdDisc::new(vec![
        CdTrack::from_index1_source(1, 4, None, 0, CdTrackMode::Mode1_2048, track_source).unwrap(),
    ])
    .unwrap();

    assert_eq!(source.read_count(), 0);
}

#[test]
fn content_identity_retains_file_hashes_and_progress() {
    let bytes = (0..5 * 2_048).map(|index| index as u8).collect::<Vec<_>>();
    let path = temp_file("identity-hash", &bytes);
    let cue = b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
    let sheet = parse_cue_bytes(cue).unwrap();
    let metadata = std::fs::metadata(&path).unwrap();
    let expected = <[u8; 32]>::from(Sha256::digest(&bytes));
    let files = [FileBackedCueFile {
        path,
        bytes: bytes.len(),
        identity: FileIdentity::from_metadata(&metadata),
        expected_sha256: Some(expected),
        reject_reparse: false,
    }];
    let mut completed = 0;

    let (_, _, hashes) = content_identity(cue, &sheet, &files, |count| {
        completed += count;
        Ok(())
    })
    .unwrap();

    assert_eq!(hashes, [expected]);
    assert_eq!(completed, bytes.len() as u64);
}

#[test]
fn full_file_precomputed_hash_is_verified() {
    let bytes = vec![0x5A; 2 * 2_048];
    let path = temp_file("prehashed-mismatch", &bytes);
    let metadata = std::fs::metadata(&path).unwrap();
    let source: Arc<dyn CdTrackSource> = FileSliceSource::open_prehashed(
        &path,
        FileIdentity::from_metadata(&metadata),
        bytes.len(),
        2_048,
        false,
        [0; 32],
    )
    .unwrap();
    let track =
        CdTrack::from_index1_source(1, 4, None, 0, CdTrackMode::Mode1_2048, source).unwrap();

    assert!(matches!(
        CdDisc::new(vec![track]).err(),
        Some(zeff_pce_core::hardware::CdDiscError::PayloadHashMismatch(1))
    ));
}

#[test]
fn split_file_tracks_compute_slice_hashes() {
    let root = std::env::temp_dir().join(format!("zeff-pce-cd-file-{}-split", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let cue = b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nTRACK 02 MODE1/2048\nINDEX 01 00:00:02\n";
    let cue_path = root.join("disc.cue");
    let bytes = (0..4 * 2_048).map(|index| index as u8).collect::<Vec<_>>();
    std::fs::write(&cue_path, cue).unwrap();
    std::fs::write(root.join("disc.bin"), &bytes).unwrap();
    let sheet = parse_cue_bytes(cue).unwrap();
    let files = open_files(&cue_path, &sheet).unwrap();

    let disc = super::build_disc(&sheet, &files, &[[0; 32]]).unwrap();

    assert_eq!(
        disc.track(1).unwrap().payload_hash(),
        <[u8; 32]>::from(Sha256::digest(&bytes[..2 * 2_048]))
    );
    assert_eq!(
        disc.track(2).unwrap().payload_hash(),
        <[u8; 32]>::from(Sha256::digest(&bytes[2 * 2_048..]))
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn direct_cue_file_sources_preserve_owned_identity_and_bytes() {
    let root =
        std::env::temp_dir().join(format!("zeff-pce-cd-file-{}-identity", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut raw = vec![0; 4 * 2_352];
    raw[16] = 0xEE;
    raw[2_352 + 16] = 0x11;
    raw[2 * 2_352 + 16] = 0x22;
    raw[3 * 2_352..3 * 2_352 + 2].copy_from_slice(&0x3456_i16.to_le_bytes());
    let cue = b"FILE \"DISC.BIN\" BINARY\nTRACK 01 MODE1/2352\nINDEX 00 00:00:00\nINDEX 01 00:00:01\nTRACK 02 AUDIO\nINDEX 01 00:00:03\n";
    let cue_path = root.join("disc.cue");
    std::fs::write(&cue_path, cue).unwrap();
    std::fs::write(root.join("disc.bin"), &raw).unwrap();
    let sheet = parse_cue_bytes(cue).unwrap();
    let owned = build_disc(cue.to_vec(), &sheet, vec![raw]).unwrap();
    let file_backed = load_direct_cue_file_backed(&cue_path, cue, &sheet).unwrap();
    assert_eq!(file_backed.disc, owned.disc);
    assert_eq!(file_backed.content_sha256, owned.content_sha256);
    assert_eq!(file_backed.content_crc32, owned.content_crc32);
    assert_eq!(file_backed.source_disc_sha256, owned.source_disc_sha256);
    assert_eq!(file_backed.disc.read_user_sector(0).unwrap()[0], 0x11);
    assert_eq!(file_backed.disc.read_user_sector(1).unwrap()[0], 0x22);
    assert_eq!(file_backed.disc.read_audio_sample(2, 0).unwrap().0, 0x3456);
}

#[test]
fn multifile_pregap_sources_match_owned_disc() {
    let root =
        std::env::temp_dir().join(format!("zeff-pce-cd-file-{}-multifile", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut first = vec![0; 2 * 2_048];
    first[0] = 0x10;
    let mut raw = vec![0; 3 * 2_352];
    raw[16] = 0x20;
    raw[2_352 + 16] = 0x21;
    let mut audio = vec![0; 2 * 2_352];
    audio[..2].copy_from_slice(&0x4567_i16.to_le_bytes());
    let cue = b"FILE \"first.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nFILE \"raw.bin\" BINARY\nTRACK 02 MODE1/2352\nINDEX 00 00:00:00\nINDEX 01 00:00:01\nFILE \"audio.bin\" BINARY\nTRACK 03 AUDIO\nPREGAP 00:00:01\nINDEX 01 00:00:00\n";
    let cue_path = root.join("disc.cue");
    std::fs::write(&cue_path, cue).unwrap();
    std::fs::write(root.join("FIRST.ISO"), &first).unwrap();
    std::fs::write(root.join("RAW.BIN"), &raw).unwrap();
    std::fs::write(root.join("AUDIO.BIN"), &audio).unwrap();
    let sheet = parse_cue_bytes(cue).unwrap();
    let owned = build_disc(cue.to_vec(), &sheet, vec![first, raw, audio]).unwrap();
    let file_backed = load_direct_cue_file_backed(&cue_path, cue, &sheet).unwrap();
    assert_eq!(file_backed.disc, owned.disc);
    assert_eq!(file_backed.content_sha256, owned.content_sha256);
    assert_eq!(file_backed.content_crc32, owned.content_crc32);
    assert_eq!(file_backed.source_disc_sha256, owned.source_disc_sha256);
    assert_eq!(file_backed.disc.read_user_sector(0).unwrap()[0], 0x10);
    assert_eq!(file_backed.disc.read_user_sector(2).unwrap()[0], 0x20);
    assert_eq!(file_backed.disc.read_user_sector(3).unwrap()[0], 0x21);
    assert!(file_backed.disc.read_audio_sample(5, 0).is_err());
    assert_eq!(file_backed.disc.read_audio_sample(6, 0).unwrap().0, 0x4567);
}

#[test]
fn owned_and_file_backed_builders_reject_the_same_invalid_layouts() {
    let cases = [
        (
            "missing-index",
            "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\n",
            vec![("disc.bin", 2_048)],
            PceCdLoadError::MissingIndex1(1),
        ),
        (
            "mixed-sectors",
            "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nINDEX 01 00:00:01\n",
            vec![("disc.bin", 2 * 2_352)],
            PceCdLoadError::MixedSectorSizes,
        ),
        (
            "misaligned",
            "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
            vec![("disc.bin", 2_049)],
            PceCdLoadError::MisalignedBin {
                bytes: 2_049,
                sector_bytes: 2_048,
            },
        ),
        (
            "outside",
            "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:01\n",
            vec![("disc.bin", 2_048)],
            PceCdLoadError::TrackOutsideBin(1),
        ),
        (
            "invalid-index-order",
            "FILE \"first.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nFILE \"second.bin\" BINARY\nTRACK 02 MODE1/2048\nINDEX 00 00:00:02\nINDEX 01 00:00:01\n",
            vec![("first.bin", 2_048), ("second.bin", 4 * 2_048)],
            PceCdLoadError::InvalidIndexOrder(2),
        ),
    ];

    for (name, cue, file_specs, expected) in cases {
        let root = std::env::temp_dir().join(format!(
            "zeff-pce-cd-file-{}-layout-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let cue_path = root.join("disc.cue");
        std::fs::write(&cue_path, cue).unwrap();
        let mut owned_files = Vec::with_capacity(file_specs.len());
        for (filename, bytes) in file_specs {
            let data = vec![0; bytes];
            std::fs::write(root.join(filename), &data).unwrap();
            owned_files.push(data);
        }
        let sheet = parse_cue_bytes(cue.as_bytes()).unwrap();
        let owned_error = match build_disc(cue.as_bytes().to_vec(), &sheet, owned_files) {
            Ok(_) => panic!("owned builder accepted invalid case {name}"),
            Err(error) => error,
        };
        let file_backed_error = match load_direct_cue_file_backed(&cue_path, cue.as_bytes(), &sheet)
        {
            Ok(_) => panic!("file-backed builder accepted invalid case {name}"),
            Err(error) => error,
        };
        assert_eq!(owned_error, expected, "owned case {name}");
        assert_eq!(file_backed_error, expected, "file-backed case {name}");
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn direct_and_cached_ppf_overlays_match_owned_raw_file_domain() {
    let root =
        std::env::temp_dir().join(format!("zeff-pce-cd-file-{}-overlay", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let first = (0..13 * 2_048)
        .map(|index| (index as u8).wrapping_mul(7).wrapping_add(3))
        .collect::<Vec<_>>();
    let second = (0..5 * 2_352)
        .map(|index| (index as u8).wrapping_mul(11).wrapping_add(5))
        .collect::<Vec<_>>();
    let third = (0..2 * 2_352)
        .map(|index| (index as u8).wrapping_mul(13).wrapping_add(9))
        .collect::<Vec<_>>();
    let cue = b"FILE \"first.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nFILE \"second.bin\" BINARY\nTRACK 02 MODE1/2352\nINDEX 00 00:00:00\nINDEX 01 00:00:01\nFILE \"third.bin\" BINARY\nTRACK 03 AUDIO\nPREGAP 00:00:01\nINDEX 01 00:00:00\n";
    let cue_path = root.join("disc.cue");
    std::fs::write(&cue_path, cue).unwrap();
    let paths = [
        root.join("first.iso"),
        root.join("second.bin"),
        root.join("third.bin"),
    ];
    for (path, bytes) in paths.iter().zip([&first, &second, &third]) {
        std::fs::write(path, bytes).unwrap();
    }
    let first_boundary = first.len();
    let second_boundary = first.len() + second.len();
    assert!((0x9320..0x9320 + 1024).contains(&second_boundary));
    let first_patch = ppf1(&[(0x9324, &[0xA5])]);
    let mut joined = [&first[..], &second[..], &third[..]].concat();
    crate::patching::apply_ppf_patch(&mut joined, &first_patch).unwrap();
    let second_patch = ppf3(
        &[(
            first_boundary as u64 - 2,
            &[0x10, 0x20, 0x30, 0x40, 0x50, 0x60],
        )],
        &joined[0x9320..0x9320 + 1024],
    );
    std::fs::write(root.join("first.ppf"), &first_patch).unwrap();
    std::fs::write(root.join("second.ppf"), &second_patch).unwrap();
    let mods = [
        crate::mods::ModEntry {
            filename: "first.ppf".to_owned(),
            enabled: true,
            target: None,
        },
        crate::mods::ModEntry {
            filename: "second.ppf".to_owned(),
            enabled: true,
            target: None,
        },
    ];
    let sheet = parse_cue_bytes(cue).unwrap();
    let direct = try_load_direct_cue_ppf_overlay(&cue_path, &sheet, &root, &mods)
        .unwrap()
        .unwrap();

    let mut owned_files = vec![first, second, third];
    apply_ppf_patch_segments(&mut owned_files, &first_patch).unwrap();
    apply_ppf_patch_segments(&mut owned_files, &second_patch).unwrap();
    let owned = build_disc(cue.to_vec(), &sheet, owned_files).unwrap().disc;
    assert_eq!(direct, owned);

    let cached_sources = paths
        .iter()
        .map(|path| {
            let bytes = std::fs::read(path).unwrap();
            CueFileSource {
                path: path.clone(),
                bytes: bytes.len() as u64,
                sha256: Sha256::digest(bytes).into(),
            }
        })
        .collect();
    let cached = try_load_cached_cue_ppf_overlay(&sheet, cached_sources, &root, &mods)
        .unwrap()
        .unwrap();
    assert_eq!(cached, owned);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn file_source_rejects_external_length_changes() {
    let path = temp_file("changed", &[0; 2_352]);
    let metadata = std::fs::metadata(&path).unwrap();
    let source = FileSliceSource::open(
        &path,
        FileIdentity::from_metadata(&metadata),
        0,
        2_352,
        2_352,
        false,
    )
    .unwrap();
    let mut bytes = [0; 4];
    source.read_exact_at(0, &mut bytes).unwrap();
    std::fs::write(&path, [0; 2 * 2_352]).unwrap();
    assert_eq!(
        source.read_exact_at(0, &mut bytes),
        Err(CdSourceError::ReadFailed)
    );
}
