use std::io::Write;

use super::*;

fn archive_path(name: &str) -> crate::test_support::TestDirectory {
    crate::test_support::test_directory(&format!("pce-cd-zip-{name}")).unwrap()
}

fn write_entries(path: &Path, entries: &[(&str, &[u8], zip::write::SimpleFileOptions)]) {
    let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, bytes, options) in entries {
        writer.start_file(*name, *options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap();
}

fn mark_first_entry_as_symlink(path: &Path) {
    let mut bytes = std::fs::read(path).unwrap();
    let header = bytes
        .windows(4)
        .position(|window| window == [0x50, 0x4b, 0x01, 0x02])
        .unwrap();
    bytes[header + 5] = 3;
    bytes[header + 38..header + 42].copy_from_slice(&((0o120777_u32) << 16).to_le_bytes());
    std::fs::write(path, bytes).unwrap();
}

fn mark_first_entry_as_encrypted(path: &Path) {
    let mut bytes = std::fs::read(path).unwrap();
    for (signature, flag_offset) in [([0x50, 0x4b, 0x03, 0x04], 6), ([0x50, 0x4b, 0x01, 0x02], 8)] {
        let header = bytes
            .windows(4)
            .position(|window| window == signature)
            .unwrap();
        let flags = u16::from_le_bytes(
            bytes[header + flag_offset..header + flag_offset + 2]
                .try_into()
                .unwrap(),
        );
        bytes[header + flag_offset..header + flag_offset + 2]
            .copy_from_slice(&(flags | 1).to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

fn set_first_entry_uncompressed_size(path: &Path, size: u32) {
    let mut bytes = std::fs::read(path).unwrap();
    for (signature, size_offset) in [
        ([0x50, 0x4b, 0x03, 0x04], 22),
        ([0x50, 0x4b, 0x01, 0x02], 24),
    ] {
        let header = bytes
            .windows(4)
            .position(|window| window == signature)
            .unwrap();
        bytes[header + size_offset..header + size_offset + 4].copy_from_slice(&size.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

fn corrupt_second_member(path: &Path) {
    let mut bytes = std::fs::read(path).unwrap();
    let header = bytes
        .windows(4)
        .enumerate()
        .filter_map(|(offset, window)| (window == [0x50, 0x4b, 0x01, 0x02]).then_some(offset))
        .nth(1)
        .unwrap();
    bytes[header + 16] ^= 1;
    std::fs::write(path, bytes).unwrap();
}

fn load_zip(path: &Path) -> Result<(PathBuf, LoadedPceCd), PceCdLoadError> {
    load_zip_cue_with_control_and_mods(
        path,
        Arc::new(AtomicBool::new(false)),
        Arc::new(PceCdPackageProgress::default()),
        false,
    )
}

#[test]
fn rejects_unsafe_and_case_fold_duplicate_members() {
    let directory = archive_path("unsafe");
    let traversal = directory.path().join("traversal.zip");
    write_entries(
        &traversal,
        &[(
            "../disc.cue",
            b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
            zip::write::SimpleFileOptions::default(),
        )],
    );
    assert!(matches!(
        inspect_zip_cue_members(&traversal),
        Err(PceCdLoadError::UnsafeArchiveEntry(_))
    ));
    assert!(matches!(
        zip_contains_cue(&traversal),
        Err(PceCdLoadError::UnsafeArchiveEntry(_))
    ));

    let duplicate = directory.path().join("duplicate.zip");
    write_entries(
        &duplicate,
        &[
            ("Disc.cue", b"", zip::write::SimpleFileOptions::default()),
            ("disc.cue", b"", zip::write::SimpleFileOptions::default()),
        ],
    );
    assert!(matches!(
        inspect_zip_cue_members(&duplicate),
        Err(PceCdLoadError::DuplicateArchiveEntry(_))
    ));
    assert!(matches!(
        zip_contains_cue(&duplicate),
        Err(PceCdLoadError::DuplicateArchiveEntry(_))
    ));
}

#[test]
fn rejects_links_and_archive_entry_overflow() {
    let directory = archive_path("limits");
    let link = directory.path().join("link.zip");
    write_entries(
        &link,
        &[(
            "disc.cue",
            b"disc.bin",
            zip::write::SimpleFileOptions::default(),
        )],
    );
    mark_first_entry_as_symlink(&link);
    assert!(matches!(
        inspect_zip_cue_members(&link),
        Err(PceCdLoadError::ArchiveLinkUnsupported(_))
    ));
    assert!(matches!(
        zip_contains_cue(&link),
        Err(PceCdLoadError::ArchiveLinkUnsupported(_))
    ));

    let overflow = directory.path().join("entries.zip");
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&overflow).unwrap());
    for index in 0..=ZIP_ENTRY_LIMIT {
        writer
            .start_file(
                format!("member-{index}.bin"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
    }
    writer.finish().unwrap();
    assert_eq!(
        inspect_zip_cue_members(&overflow),
        Err(PceCdLoadError::TooManyArchiveEntries(ZIP_ENTRY_LIMIT + 1))
    );
    assert_eq!(
        zip_contains_cue(&overflow),
        Err(PceCdLoadError::TooManyArchiveEntries(ZIP_ENTRY_LIMIT + 1))
    );
}

#[test]
fn rejects_encrypted_members() {
    let directory = archive_path("encrypted");
    let archive = directory.path().join("encrypted.zip");
    write_entries(
        &archive,
        &[(
            "disc.cue",
            b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
            zip::write::SimpleFileOptions::default(),
        )],
    );
    mark_first_entry_as_encrypted(&archive);
    assert!(matches!(
        inspect_zip_cue_members(&archive),
        Err(PceCdLoadError::Archive(_))
    ));
    assert!(zip_contains_cue(&archive).is_err());
}

#[test]
fn rejects_oversized_cue_before_decompression() {
    let directory = archive_path("cue-limit");
    let archive = directory.path().join("large-cue.zip");
    let cue = vec![0; PCE_CD_CUE_BYTES_LIMIT + 1];
    write_entries(
        &archive,
        &[("disc.cue", &cue, zip::write::SimpleFileOptions::default())],
    );
    assert_eq!(
        inspect_zip_cue_members(&archive),
        Err(PceCdLoadError::CueTooLarge(
            (PCE_CD_CUE_BYTES_LIMIT + 1) as u64
        ))
    );
}

#[test]
fn cue_probe_rejects_declared_decoded_budget_overflow() {
    let directory = archive_path("decoded-limit");
    let archive = directory.path().join("large-member.zip");
    write_entries(
        &archive,
        &[("disc.cue", b"", zip::write::SimpleFileOptions::default())],
    );
    set_first_entry_uncompressed_size(&archive, (ZIP_DECODED_BYTES_LIMIT + 1) as u32);
    assert_eq!(
        zip_contains_cue(&archive),
        Err(PceCdLoadError::ArchiveDecodedLimit)
    );
}

#[test]
fn archive_cue_references_and_checksum_fail_closed() {
    let directory = archive_path("cue-references");
    let unsafe_reference = directory.path().join("unsafe-reference.zip");
    write_entries(
        &unsafe_reference,
        &[(
            "disc.cue",
            b"FILE \"../disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
            zip::write::SimpleFileOptions::default(),
        )],
    );
    assert!(matches!(
        load_zip(&unsafe_reference),
        Err(PceCdLoadError::UnsafeFileReference(_))
    ));

    let missing_reference = directory.path().join("missing-reference.zip");
    write_entries(
        &missing_reference,
        &[(
            "disc.cue",
            b"FILE \"missing.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
            zip::write::SimpleFileOptions::default(),
        )],
    );
    assert!(matches!(
        load_zip(&missing_reference),
        Err(PceCdLoadError::ArchiveMemberMissing(_))
    ));

    let corrupt = directory.path().join("corrupt-member.zip");
    write_entries(
        &corrupt,
        &[
            (
                "disc.cue",
                b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
                zip::write::SimpleFileOptions::default(),
            ),
            (
                "disc.bin",
                &[0; 2048],
                zip::write::SimpleFileOptions::default(),
            ),
        ],
    );
    corrupt_second_member(&corrupt);
    let error = match load_zip(&corrupt) {
        Err(error) => error,
        Ok(_) => panic!("corrupt ZIP member loaded successfully"),
    };
    assert_eq!(error, PceCdLoadError::ArchiveChecksumMismatch);
}

fn ppf1(offset: u32, bytes: &[u8]) -> Vec<u8> {
    let mut patch = b"PPF10\0".to_vec();
    patch.resize(56, 0);
    patch.extend_from_slice(&offset.to_le_bytes());
    patch.push(bytes.len() as u8);
    patch.extend_from_slice(bytes);
    patch
}

#[test]
fn archive_ppf_load_is_ordered_bound_and_detached_from_the_source_path() {
    let directory = archive_path("archive-ppf");
    let archive = directory.path().join("disc.zip");
    let cue = b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
    let first = ppf1(0, &[0x11]);
    let second = ppf1(1, &[0x22]);
    write_entries(
        &archive,
        &[
            (
                "dir/disc.cue",
                cue,
                zip::write::SimpleFileOptions::default(),
            ),
            (
                "dir/disc.bin",
                &[0; 2048],
                zip::write::SimpleFileOptions::default(),
            ),
            (
                "dir/disc.ppf/0002.ppf",
                &second,
                zip::write::SimpleFileOptions::default(),
            ),
            (
                "dir/disc.ppf/0001.ppf",
                &first,
                zip::write::SimpleFileOptions::default(),
            ),
            (
                "elsewhere.ppf",
                b"ignored",
                zip::write::SimpleFileOptions::default(),
            ),
        ],
    );
    let loaded = load_zip_cue_with_control_and_archive_ppf(
        &archive,
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
        loaded.unpatched_disc_sha256,
        loaded.loaded.source_disc_sha256
    );
    assert_ne!(
        loaded.loaded.disc.content_hash(),
        loaded.unpatched_disc_sha256
    );
    assert_eq!(
        &loaded.loaded.disc.read_user_sector(0).unwrap()[..2],
        &[0x11, 0x22]
    );

    write_entries(
        &archive,
        &[
            ("disc.cue", cue, zip::write::SimpleFileOptions::default()),
            (
                "disc.bin",
                &[0xEE; 2048],
                zip::write::SimpleFileOptions::default(),
            ),
        ],
    );
    assert_eq!(
        &loaded.loaded.disc.read_user_sector(0).unwrap()[..2],
        &[0x11, 0x22]
    );
}

#[test]
fn archive_ppf_rejects_missing_gapped_and_invalid_stacks() {
    let directory = archive_path("archive-ppf-reject");
    let archive = directory.path().join("disc.zip");
    let cue = b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
    let load = || {
        load_zip_cue_with_control_and_archive_ppf(
            &archive,
            Arc::new(AtomicBool::new(false)),
            Arc::new(PceCdPackageProgress::default()),
        )
    };
    write_entries(
        &archive,
        &[
            ("disc.cue", cue, zip::write::SimpleFileOptions::default()),
            (
                "disc.bin",
                &[0; 2048],
                zip::write::SimpleFileOptions::default(),
            ),
        ],
    );
    assert!(matches!(load(), Err(PceCdLoadError::NoArchivePpfStack)));
    assert!(matches!(
        inspect_zip_ppf_candidates_with_archive_identity(&archive, &AtomicBool::new(false)),
        Err(PceCdLoadError::NoArchivePpfStack)
    ));
    write_entries(
        &archive,
        &[
            ("disc.cue", cue, zip::write::SimpleFileOptions::default()),
            (
                "disc.bin",
                &[0; 2048],
                zip::write::SimpleFileOptions::default(),
            ),
            (
                "disc.ppf/0002.ppf",
                &ppf1(0, &[1]),
                zip::write::SimpleFileOptions::default(),
            ),
        ],
    );
    assert!(matches!(load(), Err(PceCdLoadError::Disc(_))));
    write_entries(
        &archive,
        &[
            ("disc.cue", cue, zip::write::SimpleFileOptions::default()),
            (
                "disc.bin",
                &[0; 2048],
                zip::write::SimpleFileOptions::default(),
            ),
            (
                "disc.ppf/0001.ppf",
                b"invalid",
                zip::write::SimpleFileOptions::default(),
            ),
        ],
    );
    assert!(matches!(load(), Err(PceCdLoadError::Disc(_))));
    let mut fallback = b"PPF10\0".to_vec();
    fallback.resize(56, 0);
    for _ in 0..=131_072 {
        fallback.extend_from_slice(&0_u32.to_le_bytes());
        fallback.push(0);
    }
    write_entries(
        &archive,
        &[
            ("disc.cue", cue, zip::write::SimpleFileOptions::default()),
            (
                "disc.bin",
                &[0; 2048],
                zip::write::SimpleFileOptions::default(),
            ),
            (
                "disc.ppf/0001.ppf",
                &fallback,
                zip::write::SimpleFileOptions::default(),
            ),
        ],
    );
    assert!(matches!(load(), Err(PceCdLoadError::Disc(_))));
}
