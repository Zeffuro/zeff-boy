use super::*;

#[test]
fn cold_and_warm_cache_loads_preserve_identity_and_virtual_path() {
    let archive = temp_archive(
        "cache-cold-warm",
        &[
            ("set/disc.bin", vec![0x5A; 8 * 2_048]),
            ("set/disc.cue", cue()),
        ],
        true,
    );
    let cache = temp_cache("cold-warm");
    let _ = std::fs::remove_dir_all(&cache);
    let cold_progress = PceCdPackageProgress::default();
    let (cold_path, cold) =
        load_with_cache(&archive, &cache, &AtomicBool::new(false), &cold_progress).unwrap();
    let warm_progress = PceCdPackageProgress::default();
    let (warm_path, warm) =
        load_with_cache(&archive, &cache, &AtomicBool::new(false), &warm_progress).unwrap();

    assert_eq!(cold_path, archive.join("set").join("disc.cue"));
    assert_eq!(warm_path, cold_path);
    assert_eq!(warm.content_sha256, cold.content_sha256);
    assert_eq!(warm.content_crc32, cold.content_crc32);
    assert_eq!(warm.source_disc_sha256, cold.source_disc_sha256);
    assert_eq!(warm.disc, cold.disc);
    assert!(cold_progress.total_bytes() > warm_progress.total_bytes());
    assert_eq!(complete_cache_dirs(&cache).len(), 1);
    assert_eq!(
        crate::mods::mods_dir_for_rom(ActiveSystem::Pce, warm.content_crc32),
        crate::mods::mods_dir_for_rom(ActiveSystem::Pce, cold.content_crc32)
    );
    let _ = std::fs::remove_dir_all(cache);
}

#[test]
fn unmodified_cache_loads_keep_file_backed_mutation_guards() {
    let mut bin = vec![0x5A; 4 * 2_048];
    bin[..8].copy_from_slice(
        &SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_le_bytes()[..8],
    );
    let archive = temp_archive(
        "cache-file-backed",
        &[("disc.bin", bin.clone()), ("disc.cue", cue())],
        true,
    );
    let cache = temp_cache("file-backed");
    let _ = std::fs::remove_dir_all(&cache);
    let (_, loaded) = load_with_cache_and_mods(&archive, &cache, false).unwrap();
    let entry = complete_cache_dirs(&cache).pop().unwrap();
    let data_path = entry.join(CACHE_FILES_DIR).join("disc.bin");
    let mut changed = std::fs::read(&data_path).unwrap();
    changed.extend_from_slice(&[0; 2_048]);
    std::fs::write(&data_path, changed).unwrap();

    assert!(loaded.disc.read_user_sector(0).is_err());
    drop(loaded);
    let (_, recovered) = load_with_cache_and_mods(&archive, &cache, false).unwrap();
    assert_eq!(recovered.disc.read_user_sector(0).unwrap()[0..8], bin[..8]);
    let _ = std::fs::remove_dir_all(cache);
}

#[test]
fn source_metadata_change_creates_a_new_cache_identity() {
    let archive = temp_archive(
        "cache-source-change",
        &[("disc.bin", vec![0x11; 2_048]), ("disc.cue", cue())],
        true,
    );
    let cache = temp_cache("source-change");
    let _ = std::fs::remove_dir_all(&cache);
    let (_, first) = load_with_cache(
        &archive,
        &cache,
        &AtomicBool::new(false),
        &PceCdPackageProgress::default(),
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let replacement = temp_archive(
        "cache-source-change-replacement",
        &[("disc.bin", vec![0x22; 2_048]), ("disc.cue", cue())],
        true,
    );
    std::fs::copy(replacement, &archive).unwrap();
    let (_, second) = load_with_cache(
        &archive,
        &cache,
        &AtomicBool::new(false),
        &PceCdPackageProgress::default(),
    )
    .unwrap();

    assert_ne!(first.content_sha256, second.content_sha256);
    assert_eq!(complete_cache_dirs(&cache).len(), 2);
    let _ = std::fs::remove_dir_all(cache);
}

#[test]
fn source_digest_change_reextracts_when_metadata_matches() {
    let archive = temp_archive_with_methods(
        "cache-source-digest",
        &[("disc.bin", vec![0x41; 2_048]), ("disc.cue", cue())],
        false,
        vec![EncoderConfiguration::new(EncoderMethod::COPY)],
    );
    let cache = temp_cache("source-digest");
    let _ = std::fs::remove_dir_all(&cache);
    let (_, first) = load_with_cache(
        &archive,
        &cache,
        &AtomicBool::new(false),
        &PceCdPackageProgress::default(),
    )
    .unwrap();
    let modified = std::fs::metadata(&archive).unwrap().modified().unwrap();
    let replacement = temp_archive_with_methods(
        "cache-source-digest-replacement",
        &[("disc.bin", vec![0x42; 2_048]), ("disc.cue", cue())],
        false,
        vec![EncoderConfiguration::new(EncoderMethod::COPY)],
    );
    assert_eq!(
        std::fs::metadata(&replacement).unwrap().len(),
        std::fs::metadata(&archive).unwrap().len()
    );
    std::fs::copy(replacement, &archive).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&archive)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    let (_, second) = load_with_cache(
        &archive,
        &cache,
        &AtomicBool::new(false),
        &PceCdPackageProgress::default(),
    )
    .unwrap();

    assert_ne!(first.content_sha256, second.content_sha256);
    assert_eq!(complete_cache_dirs(&cache).len(), 2);
    let _ = std::fs::remove_dir_all(cache);
}

#[test]
fn corrupt_manifest_falls_back_to_clean_extraction() {
    let archive = temp_archive(
        "cache-corruption",
        &[("disc.bin", vec![0x33; 2_048]), ("disc.cue", cue())],
        true,
    );
    let cache = temp_cache("corruption");
    let _ = std::fs::remove_dir_all(&cache);
    let (_, expected) = load_with_cache(
        &archive,
        &cache,
        &AtomicBool::new(false),
        &PceCdPackageProgress::default(),
    )
    .unwrap();
    let entry = complete_cache_dirs(&cache).pop().unwrap();
    std::fs::write(entry.join(CACHE_COMPLETE_FILE), b"not json").unwrap();
    let (_, after_manifest) = load_with_cache(
        &archive,
        &cache,
        &AtomicBool::new(false),
        &PceCdPackageProgress::default(),
    )
    .unwrap();
    assert_eq!(after_manifest.disc, expected.disc);
    let _ = std::fs::remove_dir_all(cache);
}

#[test]
fn same_length_cached_data_tamper_reextracts() {
    let archive = temp_archive(
        "cache-data-tamper",
        &[("disc.bin", vec![0x33; 2_048]), ("disc.cue", cue())],
        true,
    );
    let cache = temp_cache("data-tamper");
    let _ = std::fs::remove_dir_all(&cache);
    let (_, expected) = load_with_cache(
        &archive,
        &cache,
        &AtomicBool::new(false),
        &PceCdPackageProgress::default(),
    )
    .unwrap();
    let entry = complete_cache_dirs(&cache).pop().unwrap();
    std::fs::write(
        entry.join(CACHE_FILES_DIR).join("disc.bin"),
        vec![0x99; 2_048],
    )
    .unwrap();
    let (_, after_member) = load_with_cache(
        &archive,
        &cache,
        &AtomicBool::new(false),
        &PceCdPackageProgress::default(),
    )
    .unwrap();
    assert_eq!(after_member.disc, expected.disc);
    assert_eq!(after_member.content_sha256, expected.content_sha256);
    assert_eq!(after_member.disc.read_user_sector(0).unwrap()[0], 0x33);
    let _ = std::fs::remove_dir_all(cache);
}

#[test]
fn strict_archive_identity_load_ignores_coherently_tampered_warm_cache() {
    let archive = temp_archive(
        "strict-cache-data-tamper",
        &[("disc.bin", vec![0x33; 2_048]), ("disc.cue", cue())],
        true,
    );
    let cache = temp_cache("strict-data-tamper");
    let _ = std::fs::remove_dir_all(&cache);
    let (_, expected) = load_with_cache(
        &archive,
        &cache,
        &AtomicBool::new(false),
        &PceCdPackageProgress::default(),
    )
    .unwrap();
    let expected_content_sha256 = expected.content_sha256;
    drop(expected);

    let entry = complete_cache_dirs(&cache).pop().unwrap();
    let tampered = vec![0x99; 2_048];
    std::fs::write(entry.join(CACHE_FILES_DIR).join("disc.bin"), &tampered).unwrap();
    let marker = entry.join(CACHE_COMPLETE_FILE);
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&marker).unwrap()).unwrap();
    let member = manifest["members"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|member| member["name"].as_str() == Some("disc.bin"))
        .unwrap();
    member["sha256"] = serde_json::to_value(<[u8; 32]>::from(Sha256::digest(&tampered))).unwrap();
    std::fs::write(&marker, serde_json::to_vec(&manifest).unwrap()).unwrap();

    let (_, strict, _) = load_7z_cue_with_cache_root_and_archive_identity_for_test(
        &archive,
        &AtomicBool::new(false),
        &PceCdPackageProgress::default(),
        DEFAULT_DECODER_MEMORY_LIMIT_MIB,
        &cache,
    )
    .unwrap();
    assert_eq!(strict.content_sha256, expected_content_sha256);
    assert_eq!(strict.disc.read_user_sector(0).unwrap()[0], 0x33);
    let _ = std::fs::remove_dir_all(cache);
}

#[test]
fn cancelled_extraction_publishes_no_partial_cache() {
    let archive = temp_archive_with_methods(
        "cache-cancel",
        &[
            ("disc.bin", vec![0x5A; STREAM_BUFFER_BYTES * 4]),
            ("disc.cue", cue()),
        ],
        true,
        vec![EncoderConfiguration::new(EncoderMethod::LZMA)],
    );
    let cache = temp_cache("cancel");
    let _ = std::fs::remove_dir_all(&cache);
    let cancel = AtomicBool::new(false);
    let progress = PceCdPackageProgress::default();
    progress.set_cancel_after_completed_bytes(STREAM_BUFFER_BYTES as u64);

    assert_eq!(
        load_with_cache(&archive, &cache, &cancel, &progress).err(),
        Some(PceCdLoadError::ArchiveCancelled)
    );
    assert!(complete_cache_dirs(&cache).is_empty());
    assert!(
        std::fs::read_dir(&cache)
            .into_iter()
            .flatten()
            .flatten()
            .next()
            .is_none()
    );
    let _ = std::fs::remove_dir_all(cache);
}

#[test]
fn cache_prunes_to_two_complete_entries() {
    let cache = temp_cache("prune");
    let _ = std::fs::remove_dir_all(&cache);
    for index in 0..3_u8 {
        let archive = temp_archive(
            &format!("cache-prune-{index}"),
            &[("disc.bin", vec![index; 2_048]), ("disc.cue", cue())],
            true,
        );
        load_with_cache(
            &archive,
            &cache,
            &AtomicBool::new(false),
            &PceCdPackageProgress::default(),
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        complete_cache_dirs(&cache).len(),
        CACHE_MAX_COMPLETE_ENTRIES
    );
    let _ = std::fs::remove_dir_all(cache);
}

#[test]
fn live_file_source_survives_cache_pruning() {
    let cache = temp_cache("prune-live");
    let _ = std::fs::remove_dir_all(&cache);
    let first_archive = temp_archive(
        "cache-prune-live-first",
        &[("disc.bin", vec![0xA1; 2 * 2_048]), ("disc.cue", cue())],
        true,
    );
    let (_, first) = load_with_cache(
        &first_archive,
        &cache,
        &AtomicBool::new(false),
        &PceCdPackageProgress::default(),
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    for index in 0..2_u8 {
        let archive = temp_archive(
            &format!("cache-prune-live-{index}"),
            &[
                ("disc.bin", vec![0xB0 + index; 2 * 2_048]),
                ("disc.cue", cue()),
            ],
            true,
        );
        load_with_cache(
            &archive,
            &cache,
            &AtomicBool::new(false),
            &PceCdPackageProgress::default(),
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    assert_eq!(first.disc.read_user_sector(0).unwrap()[0], 0xA1);
    drop(first);
    let _ = std::fs::remove_dir_all(cache);
}

#[test]
fn cache_cleanup_rejects_root_and_out_of_root_targets() {
    let base = temp_cache("delete-containment");
    let root = base.join("root");
    let outside = base.join("outside");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("keep"), b"keep").unwrap();

    remove_cache_entry(&root, &root);
    remove_cache_entry(&root, &outside);

    assert!(root.is_dir());
    assert_eq!(std::fs::read(outside.join("keep")).unwrap(), b"keep");
    let _ = std::fs::remove_dir_all(base);
}
