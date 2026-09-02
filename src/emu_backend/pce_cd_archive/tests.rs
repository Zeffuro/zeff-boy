use std::io::Cursor;

use super::*;
use crate::emu_backend::{BackendLoadConfig, EmuBackend, pce_cd::load_direct_cue};
use crate::emu_core_trait::EmulatorCore;
use sevenz_rust2::{
    ArchiveEntry, ArchiveWriter, EncoderConfiguration, EncoderMethod, SourceReader,
    encoder_options::Lzma2Options,
};

fn temp_archive(name: &str, entries: &[(&str, Vec<u8>)], solid: bool) -> PathBuf {
    temp_archive_with_methods(
        name,
        entries,
        solid,
        vec![EncoderConfiguration::new(EncoderMethod::LZMA2)],
    )
}

fn temp_archive_with_methods(
    name: &str,
    entries: &[(&str, Vec<u8>)],
    solid: bool,
    methods: Vec<EncoderConfiguration>,
) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("zeff-pce-cd-7z-{}-{name}.7z", std::process::id()));
    let mut writer = ArchiveWriter::create(&path).unwrap();
    writer.set_content_methods(methods);
    if solid {
        writer
            .push_archive_entries(
                entries
                    .iter()
                    .map(|(name, _)| ArchiveEntry::new_file(name))
                    .collect(),
                entries
                    .iter()
                    .map(|(_, bytes)| SourceReader::new(Cursor::new(bytes.clone())))
                    .collect(),
            )
            .unwrap();
    } else {
        for (name, bytes) in entries {
            writer
                .push_archive_entry(
                    ArchiveEntry::new_file(name),
                    Some(Cursor::new(bytes.clone())),
                )
                .unwrap();
        }
    }
    writer.finish().unwrap();
    path
}

fn cue() -> Vec<u8> {
    b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n".to_vec()
}

fn temp_cache(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("zeff-pce-cd-cache-{}-{name}", std::process::id()))
}

fn load_with_cache(
    archive: &Path,
    cache: &Path,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
) -> Result<(PathBuf, LoadedPceCd), PceCdLoadError> {
    load_7z_cue_with_cache_root(
        archive,
        cancel,
        progress,
        DEFAULT_DECODER_MEMORY_LIMIT_MIB,
        false,
        cache,
    )
}

fn load_with_cache_and_mods(
    archive: &Path,
    cache: &Path,
    apply_mods: bool,
) -> Result<(PathBuf, LoadedPceCd), PceCdLoadError> {
    load_7z_cue_with_cache_root(
        archive,
        &AtomicBool::new(false),
        &PceCdPackageProgress::default(),
        DEFAULT_DECODER_MEMORY_LIMIT_MIB,
        apply_mods,
        cache,
    )
}

fn complete_cache_dirs(cache: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(cache)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join(CACHE_COMPLETE_FILE).is_file())
        .collect()
}

#[test]
fn solid_and_non_solid_packages_match_direct_content_identity() {
    let bin = vec![0x5A; 2_048];
    let direct_root = std::env::temp_dir().join(format!(
        "zeff-pce-cd-direct-equivalence-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&direct_root).unwrap();
    std::fs::write(direct_root.join("disc.cue"), cue()).unwrap();
    std::fs::write(direct_root.join("disc.bin"), &bin).unwrap();
    let direct = load_direct_cue(&direct_root.join("disc.cue")).unwrap();

    for solid in [false, true] {
        let archive = temp_archive(
            if solid { "solid" } else { "non-solid" },
            &[("set/disc.bin", bin.clone()), ("set/disc.cue", cue())],
            solid,
        );
        let loaded = load_7z_cue(&archive).unwrap();
        assert_eq!(loaded.content_sha256, direct.content_sha256);
        assert_eq!(loaded.content_crc32, direct.content_crc32);
        assert_eq!(loaded.disc, direct.disc);
        assert_eq!(
            inspect_7z_cue_path(&archive).unwrap(),
            archive.join("set").join("disc.cue")
        );
    }
}

#[test]
fn multifile_index_zero_payload_matches_direct_and_archive_identity() {
    let cue = b"FILE \"a.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nFILE \"b.bin\" BINARY\nTRACK 02 MODE1/2352\nINDEX 00 00:00:00\nINDEX 01 00:00:01\n";
    let first = vec![0x11; 2_048];
    let mut second = vec![0; 2 * 2_352];
    second[16] = 0x20;
    second[2_352 + 16] = 0x21;
    let direct_root = std::env::temp_dir().join(format!(
        "zeff-pce-cd-index-zero-equivalence-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&direct_root).unwrap();
    std::fs::write(direct_root.join("disc.cue"), cue).unwrap();
    std::fs::write(direct_root.join("a.bin"), &first).unwrap();
    std::fs::write(direct_root.join("b.bin"), &second).unwrap();
    let direct = load_direct_cue(&direct_root.join("disc.cue")).unwrap();

    for solid in [false, true] {
        let archive = temp_archive(
            if solid {
                "index-zero-solid"
            } else {
                "index-zero-non-solid"
            },
            &[
                ("set/disc.cue", cue.to_vec()),
                ("set/a.bin", first.clone()),
                ("set/b.bin", second.clone()),
            ],
            solid,
        );
        let loaded = load_7z_cue(&archive).unwrap();
        assert_eq!(loaded.content_sha256, direct.content_sha256);
        assert_eq!(loaded.content_crc32, direct.content_crc32);
        assert_eq!(loaded.disc, direct.disc);
        let second_track = loaded.disc.track(2).unwrap();
        assert_eq!(
            loaded
                .disc
                .read_user_sector(second_track.stored_start_lba())
                .unwrap()[0],
            0x20
        );
        assert_eq!(
            loaded
                .disc
                .read_user_sector(second_track.index1_lba())
                .unwrap()[0],
            0x21
        );
    }
}

#[test]
fn shared_file_virtual_pregap_matches_direct_identity_and_audio() {
    let cue = b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2352\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nPREGAP 00:00:01\nINDEX 01 00:00:02\n";
    let mut bin = vec![0; 4 * 2_352];
    bin[16] = 0x11;
    bin[2_352 + 16] = 0x22;
    bin[2 * 2_352..2 * 2_352 + 2].copy_from_slice(&0x3456_i16.to_le_bytes());
    let direct_root = std::env::temp_dir().join(format!(
        "zeff-pce-cd-pregap-equivalence-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&direct_root).unwrap();
    std::fs::write(direct_root.join("disc.cue"), cue).unwrap();
    std::fs::write(direct_root.join("disc.bin"), &bin).unwrap();
    let direct = load_direct_cue(&direct_root.join("disc.cue")).unwrap();
    let archive = temp_archive(
        "shared-virtual-pregap",
        &[("disc.cue", cue.to_vec()), ("disc.bin", bin)],
        true,
    );
    let loaded = load_7z_cue(&archive).unwrap();

    assert_eq!(loaded.content_sha256, direct.content_sha256);
    assert_eq!(loaded.content_crc32, direct.content_crc32);
    assert_eq!(loaded.source_disc_sha256, direct.source_disc_sha256);
    assert_eq!(loaded.disc, direct.disc);
    assert!(loaded.disc.read_audio_sample(2, 0).is_err());
    assert_eq!(loaded.disc.read_audio_sample(3, 0).unwrap().0, 0x3456);
}

#[test]
fn ordinary_solid_archive_lists_and_extracts_multiple_roms() {
    let first = vec![0x11; 32 * 1024];
    let second = vec![0x22; 64 * 1024];
    let archive = temp_archive(
        "ordinary-multi-rom",
        &[
            ("games/first.gb", first.clone()),
            ("games/second.gbc", second.clone()),
            ("notes/readme.txt", b"ignored".to_vec()),
        ],
        true,
    );
    let SevenZipContents::Roms(entries) =
        inspect_7z_contents(&archive, DEFAULT_DECODER_MEMORY_LIMIT_MIB).unwrap()
    else {
        panic!("ordinary ROM archive was classified as a CD set");
    };
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, "games/first.gb");
    assert_eq!(entries[1].name, "games/second.gbc");

    let progress = PceCdPackageProgress::default();
    let (virtual_path, bytes, system) = load_7z_rom_entry_with_control(
        &archive,
        entries[1].index,
        &AtomicBool::new(false),
        &progress,
        DEFAULT_DECODER_MEMORY_LIMIT_MIB,
    )
    .unwrap();
    assert_eq!(virtual_path, archive.join("games").join("second.gbc"));
    assert_eq!(bytes, second);
    assert_eq!(system, ActiveSystem::GameBoy);
    assert_eq!(progress.phase(), PceCdPackageLoadPhase::ReadingRom);
    assert_eq!(progress.completed_bytes(), progress.total_bytes());
}

#[test]
fn ordinary_single_rom_archive_builds_a_backend_transactionally() {
    let mut rom = vec![0xEA; 0x2000];
    rom[..4].copy_from_slice(&[0xD4, 0xEA, 0x80, 0xFD]);
    rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
    let archive = temp_archive("ordinary-pce", &[("game.pce", rom)], true);
    let progress = PceCdPackageProgress::default();
    let prepared = crate::emu_backend::loader::prepare_seven_zip_backend(
        &archive,
        None,
        None,
        &BackendLoadConfig::default(),
        &AtomicBool::new(false),
        &progress,
    )
    .unwrap();
    let crate::emu_backend::loader::PreparedSevenZipBackend::Ready {
        rom_path,
        system,
        loaded,
    } = prepared
    else {
        panic!("single ROM unexpectedly requested a selection");
    };
    assert_eq!(rom_path, archive.join("game.pce"));
    assert_eq!(system, ActiveSystem::Pce);
    assert!(matches!(loaded.backend, EmuBackend::Pce(_)));
    assert_eq!(progress.phase(), PceCdPackageLoadPhase::Complete);
}

#[test]
fn raw_lzma_package_matches_direct_content_identity() {
    let bin = vec![0xA5; 2_048];
    let direct_root = std::env::temp_dir().join(format!(
        "zeff-pce-cd-direct-lzma-equivalence-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&direct_root).unwrap();
    std::fs::write(direct_root.join("disc.cue"), cue()).unwrap();
    std::fs::write(direct_root.join("disc.bin"), &bin).unwrap();
    let direct = load_direct_cue(&direct_root.join("disc.cue")).unwrap();

    let archive = temp_archive_with_methods(
        "raw-lzma",
        &[("disc.bin", bin), ("disc.cue", cue())],
        true,
        vec![EncoderConfiguration::new(EncoderMethod::LZMA)],
    );
    let loaded = load_7z_cue(&archive).unwrap();

    assert_eq!(loaded.content_sha256, direct.content_sha256);
    assert_eq!(loaded.content_crc32, direct.content_crc32);
    assert_eq!(loaded.disc, direct.disc);
}

#[test]
fn package_reference_uses_exact_then_unique_ascii_case_match() {
    let archive = temp_archive(
        "case-match",
        &[("set/DISC.BIN", vec![0; 2_048]), ("set/disc.cue", cue())],
        true,
    );
    assert!(load_7z_cue(&archive).is_ok());

    let duplicate = temp_archive(
        "case-collision",
        &[
            ("set/DISC.BIN", vec![0; 2_048]),
            ("set/disc.bin", vec![0; 2_048]),
            ("set/disc.cue", cue()),
        ],
        false,
    );
    assert!(matches!(
        inspect_7z_cue_path(&duplicate),
        Err(PceCdLoadError::DuplicateArchiveEntry(_))
    ));
}

#[test]
fn missing_multiple_unsafe_and_cancelled_packages_are_typed() {
    let missing = temp_archive("no-cue", &[("disc.bin", vec![0; 2_048])], false);
    assert_eq!(
        inspect_7z_cue_path(&missing),
        Err(PceCdLoadError::NoArchiveCue)
    );

    let multiple = temp_archive(
        "multi-cue",
        &[
            ("a.cue", cue()),
            ("b.cue", cue()),
            ("disc.bin", vec![0; 2_048]),
        ],
        false,
    );
    assert_eq!(
        inspect_7z_cue_path(&multiple),
        Err(PceCdLoadError::MultipleArchiveCues)
    );

    let unsafe_path = temp_archive(
        "unsafe",
        &[("../disc.cue", cue()), ("disc.bin", vec![0; 2_048])],
        false,
    );
    assert!(matches!(
        inspect_7z_cue_path(&unsafe_path),
        Err(PceCdLoadError::UnsafeArchiveEntry(_))
    ));

    let valid = temp_archive(
        "cancel",
        &[("disc.bin", vec![0; 2_048]), ("disc.cue", cue())],
        true,
    );
    let cancel = AtomicBool::new(true);
    let progress = PceCdPackageProgress::default();
    assert_eq!(
        load_7z_cue_with_control(&valid, &cancel, &progress, DEFAULT_DECODER_MEMORY_LIMIT_MIB,)
            .err(),
        Some(PceCdLoadError::ArchiveCancelled)
    );
}

#[test]
fn controlled_load_reports_complete_cached_preparation() {
    let valid = temp_archive(
        "progress",
        &[("disc.bin", vec![0; 2_048]), ("disc.cue", cue())],
        true,
    );
    let cancel = AtomicBool::new(false);
    let progress = PceCdPackageProgress::default();
    let (virtual_path, _) =
        load_7z_cue_with_control(&valid, &cancel, &progress, DEFAULT_DECODER_MEMORY_LIMIT_MIB)
            .unwrap();
    assert!(virtual_path.ends_with("disc.cue"));
    assert_eq!(progress.phase(), PceCdPackageLoadPhase::ReadingData);
    assert!(progress.total_bytes() > 0);
    assert_eq!(progress.completed_bytes(), progress.total_bytes());
}

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

#[test]
fn controlled_load_cancels_at_a_decode_chunk_boundary() {
    let valid = temp_archive_with_methods(
        "mid-stream-cancel",
        &[
            ("disc.bin", vec![0x5A; STREAM_BUFFER_BYTES * 4]),
            ("disc.cue", cue()),
        ],
        true,
        vec![EncoderConfiguration::new(EncoderMethod::LZMA)],
    );
    let cancel = AtomicBool::new(false);
    let progress = PceCdPackageProgress::default();
    progress.set_cancel_after_completed_bytes(STREAM_BUFFER_BYTES as u64);

    assert_eq!(
        load_7z_cue_with_control(&valid, &cancel, &progress, DEFAULT_DECODER_MEMORY_LIMIT_MIB,)
            .err(),
        Some(PceCdLoadError::ArchiveCancelled)
    );
    assert!(cancel.load(Ordering::Acquire));
    assert!(progress.completed_bytes() >= STREAM_BUFFER_BYTES as u64);
    assert!(progress.completed_bytes() < progress.total_bytes());
}

#[test]
fn parser_rejects_entry_counts_before_application_allocation() {
    let entries = (0..=PCE_CD_7Z_ENTRY_LIMIT)
        .map(|index| (format!("empty-{index}.bin"), Vec::new()))
        .collect::<Vec<_>>();
    let borrowed = entries
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.clone()))
        .collect::<Vec<_>>();
    let archive = temp_archive("entry-limit", &borrowed, false);
    assert!(matches!(
        inspect_7z_cue_path(&archive),
        Err(PceCdLoadError::Archive(_))
    ));
}

#[test]
fn unsupported_codec_memory_limit_and_crc_corruption_are_typed() {
    let entries = [("disc.bin", vec![0; 2_048]), ("disc.cue", cue())];
    let unsupported = temp_archive_with_methods(
        "unsupported-codec",
        &entries,
        false,
        vec![EncoderConfiguration::new(EncoderMethod::DELTA_FILTER)],
    );
    assert!(matches!(
        load_7z_cue(&unsupported),
        Err(PceCdLoadError::ArchiveCodecUnsupported(_))
    ));

    let mut options = Lzma2Options::from_level(1);
    options.set_dictionary_size(65 * 1024 * 1024);
    let excessive_memory =
        temp_archive_with_methods("memory-limit", &entries, false, vec![options.into()]);
    assert_eq!(
        load_7z_cue_with_control(
            &excessive_memory,
            &AtomicBool::new(false),
            &PceCdPackageProgress::default(),
            64,
        )
        .err(),
        Some(PceCdLoadError::ArchiveMemoryLimit {
            allowed_mib: 64,
            required_mib: 65,
        })
    );

    let corrupt = temp_archive_with_methods(
        "crc",
        &entries,
        true,
        vec![EncoderConfiguration::new(EncoderMethod::COPY)],
    );
    let mut bytes = std::fs::read(&corrupt).unwrap();
    bytes[32] ^= 0x80;
    std::fs::write(&corrupt, bytes).unwrap();
    assert_eq!(
        load_7z_cue(&corrupt).err(),
        Some(PceCdLoadError::ArchiveChecksumMismatch)
    );
}

#[test]
fn packaged_loader_preserves_virtual_cue_and_real_source_paths() {
    let archive = temp_archive(
        "backend",
        &[("set/disc.bin", vec![0; 2_048]), ("set/disc.cue", cue())],
        true,
    );
    let system_card: &'static [u8] = Box::leak(vec![0; 262_144].into_boxed_slice());
    let cancel = AtomicBool::new(false);
    let progress = PceCdPackageProgress::default();
    let (cue_path, loaded) = crate::emu_backend::loader::prepare_pce_cd_7z_backend(
        &archive,
        None,
        &BackendLoadConfig {
            pce_cd_system_card_override: Some(system_card),
            pce_cd_system_card_sha256_override: Some(zeff_firmware::PCE_SYSTEM_CARD_V3_USA_SHA256),
            pce_console_wiring: Some(zeff_pce_core::hardware::PceConsoleWiring::TurboGrafx16),
            ..BackendLoadConfig::default()
        },
        &cancel,
        &progress,
    )
    .unwrap();
    let EmuBackend::Pce(backend) = loaded.backend else {
        panic!("7z CUE loader returned a non-PCE backend");
    };
    assert_eq!(backend.rom_path(), cue_path);
    assert_eq!(backend.source_path(), archive);
    assert_eq!(
        backend.hucard_board(),
        zeff_pce_core::hardware::PceHuCardBoard::SystemCardV3
    );
    assert_eq!(progress.phase(), PceCdPackageLoadPhase::Complete);
}

#[test]
#[ignore = "requires ZEFF_PCE_CD_AUDIO_7Z_SMOKE with a 96 MiB dictionary archive"]
fn local_96_mib_dictionary_loads_mixed_mode_disc() {
    let archive = PathBuf::from(std::env::var("ZEFF_PCE_CD_AUDIO_7Z_SMOKE").unwrap());
    let cancel = AtomicBool::new(false);
    let progress = PceCdPackageProgress::default();
    let (_, loaded) = load_7z_cue_with_control(&archive, &cancel, &progress, 128).unwrap();
    assert!(
        loaded
            .disc
            .tracks()
            .iter()
            .any(|track| track.mode() == zeff_pce_core::hardware::CdTrackMode::Audio)
    );
}
