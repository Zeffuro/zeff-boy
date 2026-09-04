use super::*;
use std::fs::{FileTimes, OpenOptions};
use std::io::{Seek, SeekFrom};

fn ppf1(offset: u32, bytes: &[u8]) -> Vec<u8> {
    let mut patch = b"PPF10\0".to_vec();
    patch.resize(56, 0);
    patch.extend_from_slice(&offset.to_le_bytes());
    patch.push(bytes.len() as u8);
    patch.extend_from_slice(bytes);
    patch
}

fn load_ppf(path: &Path) -> Result<PceCdArchivePpfLoad, PceCdLoadError> {
    load_7z_cue_with_control_and_archive_ppf(
        path,
        &AtomicBool::new(false),
        &PceCdPackageProgress::default(),
        DEFAULT_DECODER_MEMORY_LIMIT_MIB,
    )
}

#[test]
fn seven_zip_archive_ppf_is_ordered_bound_and_source_mutation_safe() {
    let first = ppf1(0, &[0x51]);
    let second = ppf1(1, &[0x62]);
    let archive = temp_archive(
        "ppf-stack",
        &[
            ("dir/disc.cue", cue()),
            ("dir/disc.bin", vec![0; 2048]),
            ("dir/disc.ppf/0002.ppf", second),
            ("dir/disc.ppf/0001.ppf", first),
            ("unrelated.ppf", b"ignored".to_vec()),
        ],
        true,
    );
    let candidates = inspect_7z_ppf_candidates_with_archive_identity(
        &archive,
        &AtomicBool::new(false),
        &PceCdPackageProgress::default(),
        DEFAULT_DECODER_MEMORY_LIMIT_MIB,
    )
    .unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].patches.len(), 2);
    let loaded = load_ppf(&archive).unwrap();
    assert_eq!(
        loaded
            .patch_identities()
            .into_iter()
            .map(|patch| patch.member_path)
            .collect::<Vec<_>>(),
        ["dir/disc.ppf/0001.ppf", "dir/disc.ppf/0002.ppf"]
    );
    assert_eq!(
        loaded.unpatched_disc_sha256,
        loaded.loaded.source_disc_sha256
    );
    assert_ne!(
        loaded.unpatched_disc_sha256,
        loaded.loaded.disc.content_hash()
    );
    assert_eq!(
        &loaded.loaded.disc.read_user_sector(0).unwrap()[..2],
        &[0x51, 0x62]
    );

    let replacement = temp_archive(
        "ppf-stack-replacement",
        &[("disc.cue", cue()), ("disc.bin", vec![0xEE; 2048])],
        false,
    );
    std::fs::copy(replacement, &archive).unwrap();
    assert_eq!(
        &loaded.loaded.disc.read_user_sector(0).unwrap()[..2],
        &[0x51, 0x62]
    );
    let _ = std::fs::remove_file(archive);
}

#[test]
fn seven_zip_archive_ppf_full_load_succeeds_after_held_source_reauthentication() {
    let archive = temp_archive(
        "ppf-full-load-reauthentication",
        &[
            ("disc.cue", cue()),
            ("disc.bin", vec![0; 2048]),
            ("disc.ppf/0001.ppf", ppf1(0, &[0xA7])),
        ],
        false,
    );
    let expected_source_sha256: [u8; 32] = Sha256::digest(std::fs::read(&archive).unwrap()).into();
    let loaded = load_ppf(&archive).unwrap();
    assert_eq!(
        loaded.archive_identity.source_sha256,
        expected_source_sha256
    );
    assert_eq!(loaded.patches.len(), 1);
    assert_eq!(loaded.loaded.disc.read_user_sector(0).unwrap()[0], 0xA7);
    let _ = std::fs::remove_file(archive);
}

#[test]
fn seven_zip_archive_ppf_rejects_missing_gap_and_invalid_bytes() {
    let missing = temp_archive(
        "ppf-missing",
        &[("disc.cue", cue()), ("disc.bin", vec![0; 2048])],
        false,
    );
    assert!(matches!(
        load_ppf(&missing),
        Err(PceCdLoadError::NoArchivePpfStack)
    ));
    assert!(matches!(
        inspect_7z_ppf_candidates_with_archive_identity(
            &missing,
            &AtomicBool::new(false),
            &PceCdPackageProgress::default(),
            DEFAULT_DECODER_MEMORY_LIMIT_MIB,
        ),
        Err(PceCdLoadError::NoArchivePpfStack)
    ));

    let gap = temp_archive(
        "ppf-gap",
        &[
            ("disc.cue", cue()),
            ("disc.bin", vec![0; 2048]),
            ("disc.ppf/0002.ppf", ppf1(0, &[1])),
        ],
        false,
    );
    assert!(matches!(load_ppf(&gap), Err(PceCdLoadError::Disc(_))));

    let invalid = temp_archive(
        "ppf-invalid",
        &[
            ("disc.cue", cue()),
            ("disc.bin", vec![0; 2048]),
            ("disc.ppf/0001.ppf", b"invalid".to_vec()),
        ],
        false,
    );
    assert!(matches!(load_ppf(&invalid), Err(PceCdLoadError::Disc(_))));
    for path in [missing, gap, invalid] {
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn seven_zip_archive_ppf_reauthentication_detects_same_metadata_mutation() {
    let archive = temp_archive(
        "ppf-held-source-mutation",
        &[
            ("disc.cue", cue()),
            ("disc.bin", vec![0; 2048]),
            ("disc.ppf/0001.ppf", ppf1(0, &[1])),
        ],
        false,
    );
    let cancel = AtomicBool::new(false);
    let progress = PceCdPackageProgress::default();
    let (reader, _, expected, mut verifier) = open_validated_with_source_verifier(
        &archive,
        DEFAULT_DECODER_MEMORY_LIMIT_MIB,
        &cancel,
        &progress,
    )
    .unwrap();
    drop(reader);
    let metadata = verifier.metadata().unwrap();
    let modified = metadata.modified().unwrap();
    let mut writer = OpenOptions::new().write(true).open(&archive).unwrap();
    writer.seek(SeekFrom::Start(0)).unwrap();
    writer.write_all(b"8").unwrap();
    writer.sync_data().unwrap();
    writer
        .set_times(FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(writer.metadata().unwrap().len(), metadata.len());
    assert_eq!(writer.metadata().unwrap().modified().unwrap(), modified);
    assert!(matches!(
        reauthenticate_source(&mut verifier, expected, &archive, &cancel, &progress),
        Err(PceCdLoadError::ArchiveChanged)
    ));
    let _ = std::fs::remove_file(archive);
}
