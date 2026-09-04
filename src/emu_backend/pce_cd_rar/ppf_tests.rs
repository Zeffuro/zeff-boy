use super::tests::{archive_path, write_archive};
use super::*;

fn ppf1(offset: u32, bytes: &[u8]) -> Vec<u8> {
    let mut patch = b"PPF10\0".to_vec();
    patch.resize(56, 0);
    patch.extend_from_slice(&offset.to_le_bytes());
    patch.push(bytes.len() as u8);
    patch.extend_from_slice(bytes);
    patch
}

#[test]
fn rar_archive_ppf_is_ordered_and_rejects_missing_gap_and_invalid_bytes() {
    let path = archive_path("ppf");
    let cue = b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
    let first = ppf1(0, &[0x31]);
    let second = ppf1(1, &[0x42]);
    write_archive(
        &path,
        &[
            (b"dir/disc.cue", cue),
            (b"dir/disc.bin", &[0; 2048]),
            (b"dir/disc.ppf/0002.ppf", &second),
            (b"dir/disc.ppf/0001.ppf", &first),
        ],
    );
    let loaded = load_rar_cue_with_control_and_archive_ppf(
        &path,
        Arc::new(AtomicBool::new(false)),
        Arc::new(PceCdPackageProgress::default()),
    )
    .unwrap();
    assert_eq!(
        loaded
            .patch_identities()
            .into_iter()
            .map(|patch| patch.member_path)
            .collect::<Vec<_>>(),
        ["dir/disc.ppf/0001.ppf", "dir/disc.ppf/0002.ppf"]
    );
    assert_eq!(
        &loaded.loaded.disc.read_user_sector(0).unwrap()[..2],
        &[0x31, 0x42]
    );
    write_archive(&path, &[(b"disc.cue", cue), (b"disc.bin", &[0; 2048])]);
    assert_eq!(
        &loaded.loaded.disc.read_user_sector(0).unwrap()[..2],
        &[0x31, 0x42]
    );
    assert!(matches!(
        load_rar_cue_with_control_and_archive_ppf(
            &path,
            Arc::new(AtomicBool::new(false)),
            Arc::new(PceCdPackageProgress::default()),
        ),
        Err(PceCdLoadError::NoArchivePpfStack)
    ));
    assert!(matches!(
        inspect_rar_ppf_candidates_with_archive_identity(&path, Arc::new(AtomicBool::new(false)),),
        Err(PceCdLoadError::NoArchivePpfStack)
    ));
    write_archive(
        &path,
        &[
            (b"disc.cue", cue),
            (b"disc.bin", &[0; 2048]),
            (b"disc.ppf/0002.ppf", &first),
        ],
    );
    assert!(matches!(
        load_rar_cue_with_control_and_archive_ppf(
            &path,
            Arc::new(AtomicBool::new(false)),
            Arc::new(PceCdPackageProgress::default()),
        ),
        Err(PceCdLoadError::Disc(_))
    ));
    write_archive(
        &path,
        &[
            (b"disc.cue", cue),
            (b"disc.bin", &[0; 2048]),
            (b"disc.ppf/0001.ppf", b"invalid"),
        ],
    );
    assert!(matches!(
        load_rar_cue_with_control_and_archive_ppf(
            &path,
            Arc::new(AtomicBool::new(false)),
            Arc::new(PceCdPackageProgress::default()),
        ),
        Err(PceCdLoadError::Disc(_))
    ));
    let _ = std::fs::remove_file(path);
}

#[test]
fn rar_archive_ppf_extraction_observes_cancellation() {
    let path = archive_path("ppf-cancel");
    let patch = ppf1(0, &[0x31]);
    write_archive(&path, &[(b"disc.ppf/0001.ppf", &patch)]);
    let open_cancel = AtomicBool::new(false);
    let (archive, manifest, _, _) = open_validated_owned(&path, &open_cancel).unwrap();
    let cancel = Arc::new(AtomicBool::new(true));
    let targets = BTreeSet::from(["disc.ppf/0001.ppf".to_owned()]);
    assert!(matches!(
        extract_targets(
            &archive,
            &manifest,
            &targets,
            cancel,
            Arc::new(PceCdPackageProgress::default()),
            0,
        ),
        Err(PceCdLoadError::ArchiveCancelled)
    ));
    let _ = std::fs::remove_file(path);
}
