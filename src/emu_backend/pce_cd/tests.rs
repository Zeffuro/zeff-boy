use super::*;
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use crate::emu_core_trait::EmulatorCore;

fn temp_set(name: &str, files: &[(&str, &[u8])], cue: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("zeff-pce-cd-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for (name, bytes) in files {
        let path = root.join(portable_path(name));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    let cue_path = root.join("disc.cue");
    std::fs::write(&cue_path, cue).unwrap();
    cue_path
}

#[test]
fn direct_cue_strips_first_index_zero_and_reads_mode1_payload() {
    let mut bin = vec![0xE0; 3 * 2_352];
    bin[2_352 + 16..2_352 + 16 + 2_048].fill(0x4C);
    let cue =
        "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2352\nINDEX 00 00:00:00\nINDEX 01 00:00:01\n";
    let path = temp_set("valid", &[("disc.bin", &bin)], cue);
    let loaded = load_direct_cue(&path).unwrap();
    assert_eq!(loaded.disc.track(1).unwrap().index0_lba(), None);
    assert_eq!(loaded.disc.track(1).unwrap().index1_lba(), 0);
    assert_eq!(loaded.disc.read_user_sector(0).unwrap()[0], 0x4C);
}

#[test]
fn selecting_iso_uses_its_unique_referencing_cue() {
    let data = vec![0x5A; 2 * 2_048];
    let cue = "FILE \"DISC.ISO\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
    let cue_path = temp_set("iso-sidecar", &[("disc.iso", &data)], cue);
    let iso_path = cue_path.with_extension("iso");

    assert_eq!(cue_path_for_iso(&iso_path).unwrap(), cue_path);
    let via_cue = load_direct_cue(&cue_path).unwrap();
    let via_iso = load_direct_cue(&cue_path_for_iso(&iso_path).unwrap()).unwrap();
    assert_eq!(via_iso.disc, via_cue.disc);
    assert_eq!(via_iso.content_sha256, via_cue.content_sha256);
}

#[test]
fn selecting_iso_rejects_missing_and_ambiguous_cue_metadata() {
    let missing_root =
        std::env::temp_dir().join(format!("zeff-pce-cd-{}-iso-missing", std::process::id()));
    std::fs::create_dir_all(&missing_root).unwrap();
    let missing_iso = missing_root.join("disc.iso");
    std::fs::write(&missing_iso, [0; 2_048]).unwrap();
    assert_eq!(
        cue_path_for_iso(&missing_iso),
        Err(PceCdLoadError::IsoCueMissing(missing_iso.clone()))
    );

    let ambiguous_root =
        std::env::temp_dir().join(format!("zeff-pce-cd-{}-iso-ambiguous", std::process::id()));
    std::fs::create_dir_all(&ambiguous_root).unwrap();
    let ambiguous_iso = ambiguous_root.join("disc.iso");
    std::fs::write(&ambiguous_iso, [0; 2_048]).unwrap();
    let cue = "FILE \"disc.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
    std::fs::write(ambiguous_root.join("one.cue"), cue).unwrap();
    std::fs::write(ambiguous_root.join("two.cue"), cue).unwrap();
    assert!(matches!(
        cue_path_for_iso(&ambiguous_iso),
        Err(PceCdLoadError::IsoCueAmbiguous(cues)) if cues.len() == 2
    ));
}

#[test]
fn multiple_files_reset_indices_and_allow_distinct_data_sector_sizes() {
    let mut first = vec![0; 2 * 2_048];
    first[0] = 0x11;
    let mut second = vec![0; 3 * 2_352];
    second[16] = 0x12;
    second[2_352 + 16] = 0x22;
    let cue = "FILE \"a.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nFILE \"sub\\b.bin\" BINARY\nTRACK 02 MODE1/2352\nINDEX 00 00:00:00\nINDEX 01 00:00:01\n";
    let path = temp_set("multi", &[("a.bin", &first), ("sub/b.bin", &second)], cue);
    let loaded = load_direct_cue(&path).unwrap();
    let second_track = loaded.disc.track(2).unwrap();
    assert_eq!(second_track.index0_lba(), Some(2));
    assert_eq!(second_track.index1_lba(), 3);
    assert_eq!(loaded.disc.read_user_sector(0).unwrap()[0], 0x11);
    assert_eq!(loaded.disc.read_user_sector(2).unwrap()[0], 0x12);
    assert_eq!(loaded.disc.read_user_sector(3).unwrap()[0], 0x22);
}

#[test]
fn same_file_later_track_retains_index_zero_payload() {
    let mut bin = vec![0; 5 * 2_048];
    bin[2 * 2_048] = 0x20;
    bin[3 * 2_048] = 0x21;
    let cue = "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nTRACK 02 MODE1/2048\nINDEX 00 00:00:02\nINDEX 01 00:00:03\n";
    let path = temp_set("same-file-pregap", &[("disc.bin", &bin)], cue);
    let loaded = load_direct_cue(&path).unwrap();
    let second = loaded.disc.track(2).unwrap();
    assert_eq!(second.stored_start_lba(), 2);
    assert_eq!(second.index1_lba(), 3);
    assert_eq!(loaded.disc.read_user_sector(2).unwrap()[0], 0x20);
    assert_eq!(loaded.disc.read_user_sector(3).unwrap()[0], 0x21);
}

#[test]
fn same_file_virtual_pregap_advances_timeline_without_consuming_payload() {
    let mut data = vec![0; 5 * 2_048];
    data[2 * 2_048] = 0x22;
    let cue = "FILE \"disc.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nTRACK 02 MODE1/2048\nPREGAP 00:00:02\nINDEX 01 00:00:02\n";
    let path = temp_set("virtual-pregap", &[("disc.iso", &data)], cue);
    let loaded = load_direct_cue(&path).unwrap();
    let first = loaded.disc.track(1).unwrap();
    let second = loaded.disc.track(2).unwrap();

    assert_eq!(first.end_lba(), 2);
    assert_eq!(second.index0_lba(), Some(2));
    assert_eq!(second.index1_lba(), 4);
    assert_eq!(second.stored_start_lba(), 4);
    assert!(loaded.disc.read_user_sector(2).is_err());
    assert_eq!(loaded.disc.read_user_sector(4).unwrap()[0], 0x22);
}

#[test]
fn cue_rejects_stored_and_virtual_pregap_on_the_same_track() {
    let cue = "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nPREGAP 00:00:02\nINDEX 00 00:00:00\nINDEX 01 00:00:01\n";
    assert_eq!(parse_cue(cue), Err(PceCdLoadError::InvalidIndexOrder(1)));
}

#[test]
fn lemmings_shaped_multifile_pregaps_keep_track_two_three_and_twenty_five() {
    let mut cue = String::new();
    let mut files = Vec::new();
    for number in 1..=25_u8 {
        let mode = if matches!(number, 2 | 25) {
            CdTrackMode::Mode1_2352
        } else {
            CdTrackMode::Audio
        };
        let pregap = match number {
            2 | 25 => 224,
            3 => 150,
            _ => 0,
        };
        cue.push_str(&format!(
            "FILE \"track{number:02}.bin\" BINARY\nTRACK {number:02} {}\n",
            if mode == CdTrackMode::Audio {
                "AUDIO"
            } else {
                "MODE1/2352"
            }
        ));
        if pregap != 0 {
            cue.push_str("INDEX 00 00:00:00\n");
            cue.push_str(&format!(
                "INDEX 01 00:{:02}:{:02}\n",
                pregap / 75,
                pregap % 75
            ));
        } else {
            cue.push_str("INDEX 01 00:00:00\n");
        }
        let mut bytes = vec![0; (pregap + 1) * 2_352];
        if mode == CdTrackMode::Audio {
            bytes[..2].copy_from_slice(&i16::from(number).to_le_bytes());
            bytes[pregap * 2_352..pregap * 2_352 + 2]
                .copy_from_slice(&i16::from(number + 0x40).to_le_bytes());
        } else {
            bytes[16] = number;
            bytes[pregap * 2_352 + 16] = number + 0x40;
        }
        files.push(bytes);
    }
    let sheet = parse_cue(&cue).unwrap();
    let loaded = build_disc(cue.into_bytes(), &sheet, files).unwrap();
    for (number, pregap) in [(2, 224), (3, 150), (25, 224)] {
        let track = loaded.disc.track(number).unwrap();
        assert_eq!(track.index1_lba() - track.stored_start_lba(), pregap);
    }
    for number in [2, 25] {
        let track = loaded.disc.track(number).unwrap();
        assert_eq!(
            loaded
                .disc
                .read_user_sector(track.stored_start_lba())
                .unwrap()[0],
            number
        );
        assert_eq!(
            loaded.disc.read_user_sector(track.index1_lba()).unwrap()[0],
            number + 0x40
        );
    }
    let track = loaded.disc.track(3).unwrap();
    assert_eq!(
        loaded
            .disc
            .read_audio_sample(track.stored_start_lba(), 0)
            .unwrap()
            .0,
        3
    );
    assert_eq!(
        loaded
            .disc
            .read_audio_sample(track.index1_lba(), 0)
            .unwrap()
            .0,
        0x43
    );
}

#[test]
fn later_files_anchor_at_first_index_zero_or_index_one() {
    let first = vec![0; 2 * 2_048];
    let mut second = vec![0; 10 * 2_048];
    second[7 * 2_048] = 0x27;
    let mut third = vec![0; 8 * 2_048];
    third[5 * 2_048] = 0x35;
    let cue = "FILE \"a.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nFILE \"b.bin\" BINARY\nTRACK 02 MODE1/2048\nINDEX 00 00:00:05\nINDEX 01 00:00:07\nFILE \"c.bin\" BINARY\nTRACK 03 MODE1/2048\nINDEX 01 00:00:05\n";
    let path = temp_set(
        "file-anchors",
        &[("a.bin", &first), ("b.bin", &second), ("c.bin", &third)],
        cue,
    );
    let loaded = load_direct_cue(&path).unwrap();
    assert_eq!(loaded.disc.track(2).unwrap().index0_lba(), Some(2));
    assert_eq!(loaded.disc.track(2).unwrap().index1_lba(), 4);
    assert_eq!(loaded.disc.track(3).unwrap().index1_lba(), 7);
    assert_eq!(loaded.disc.leadout_lba(), 10);
    assert_eq!(loaded.disc.read_user_sector(4).unwrap()[0], 0x27);
    assert_eq!(loaded.disc.read_user_sector(7).unwrap()[0], 0x35);
}

#[test]
fn portable_references_reject_unsafe_and_colliding_forms() {
    for value in [
        "",
        "/a.bin",
        "\\\\server\\a.bin",
        "C:\\a.bin",
        "a:stream",
        "../a.bin",
        "a/./b.bin",
        "a//b.bin",
        "a.bin/",
        "a\0.bin",
    ] {
        assert!(
            normalize_portable_path(value).is_err(),
            "accepted {value:?}"
        );
    }
    assert_eq!(normalize_portable_path("a\\b.bin").unwrap(), "a/b.bin");

    let cue = "FILE \"A.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nFILE \"a.BIN\" BINARY\nTRACK 02 MODE1/2048\nINDEX 01 00:00:00\n";
    assert_eq!(parse_cue(cue), Err(PceCdLoadError::DuplicateFile));
}

#[test]
fn cue_validation_accepts_audio_and_rejects_missing_indices_and_track_overflow() {
    let audio = "FILE \"disc.bin\" BINARY\nTRACK 01 AUDIO\nINDEX 01 00:00:00\n";
    assert_eq!(parse_cue(audio).unwrap().tracks[0].mode, CdTrackMode::Audio);
    let missing = "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2352\n";
    let sheet = parse_cue(missing).unwrap();
    assert!(matches!(
        build_disc(vec![], &sheet, vec![vec![0; 2_352]]),
        Err(PceCdLoadError::MissingIndex1(1))
    ));

    let mut maximum = String::from("FILE \"disc.bin\" BINARY\n");
    for track in 1..=99 {
        let lba = track - 1;
        maximum.push_str(&format!(
            "TRACK {track:02} MODE1/2352\nINDEX 01 00:{:02}:{:02}\n",
            lba / 75,
            lba % 75,
        ));
    }
    maximum.push_str("TRACK 99 MODE1/2352\nINDEX 01 00:01:39\n");
    assert_eq!(parse_cue(&maximum), Err(PceCdLoadError::InvalidTrackOrder));
}

#[test]
fn canonical_identity_is_length_framed_and_reference_ordered() {
    let cue = b"FILE \"a.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
    let sheet = parse_cue_bytes(cue).unwrap();
    let first = build_disc(cue.to_vec(), &sheet, vec![vec![1; 2_048]]).unwrap();
    let second = build_disc(cue.to_vec(), &sheet, vec![vec![2; 2_048]]).unwrap();
    assert_ne!(first.content_sha256, second.content_sha256);
    assert_ne!(first.content_crc32, second.content_crc32);
}

#[test]
fn normalized_source_identity_uses_the_exact_built_track_layout() {
    let cue = b"FILE \"one.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 00 00:00:00\nINDEX 01 00:00:01\nTRACK 02 MODE1/2048\nPREGAP 00:00:01\nINDEX 01 00:00:03\nFILE \"two.bin\" BINARY\nTRACK 03 AUDIO\nINDEX 00 00:00:00\nINDEX 01 00:00:01\n";
    let sheet = parse_cue_bytes(cue).unwrap();
    let files = vec![vec![0x11; 5 * 2_048], vec![0x22; 3 * 2_352]];
    let layout = cue_track_layout(&sheet, &[files[0].len(), files[1].len()]).unwrap();
    assert_eq!(layout[0][0].index0, None);
    assert_eq!(layout[0][0].index1, 0);
    assert_eq!(layout[0][0].source_bytes, 2_048..3 * 2_048);
    assert_eq!(layout[0][1].index0, Some(2));
    assert_eq!(layout[0][1].index1, 3);
    assert_eq!(layout[0][1].stored_start, 3);
    assert_eq!(layout[0][1].source_bytes, 3 * 2_048..5 * 2_048);
    assert_eq!(layout[1][0].index0, Some(5));
    assert_eq!(layout[1][0].index1, 6);
    assert_eq!(layout[1][0].source_bytes, 0..3 * 2_352);
    let source_hash = normalized_disc_identity(&sheet, &files).unwrap();
    let loaded = build_disc(cue.to_vec(), &sheet, files).unwrap();
    assert_eq!(source_hash, loaded.disc.content_hash());
    assert_eq!(source_hash, loaded.source_disc_sha256);
}

#[test]
fn chd_track_metadata_preserves_embedded_pregap_and_rejects_virtual_pregap() {
    let track = parse_chd_track_metadata(
            b"TRACK:2 TYPE:MODE1_RAW SUBTYPE:NONE FRAMES:3 PREGAP:1 PGTYPE:VMODE1_RAW PGSUB:NONE POSTGAP:0",
        )
        .unwrap();
    assert_eq!(track.number, 2);
    assert_eq!(track.mode, CdTrackMode::Mode1_2352);
    assert_eq!(track.frames, 3);
    assert_eq!(track.pregap, 1);
    assert_eq!(
            parse_chd_track_metadata(
                b"TRACK:2 TYPE:MODE1_RAW SUBTYPE:NONE FRAMES:3 PREGAP:1 PGTYPE:MODE1_RAW PGSUB:NONE POSTGAP:0",
            ),
            Err(PceCdLoadError::UnsupportedChdPregap(2))
        );
}

#[test]
fn resized_chd_track_payload_updates_the_reconstructed_toc() {
    let mut tracks = vec![ChdTrack {
        number: 1,
        mode: CdTrackMode::Audio,
        frames: 1,
        pregap: 0,
    }];
    let payloads = vec![vec![0; 3 * 2_352]];

    refresh_chd_track_lengths(&mut tracks, &payloads).unwrap();
    let disc = build_chd_disc(&tracks, &payloads).unwrap();

    assert_eq!(tracks[0].frames, 3);
    assert_eq!(disc.tracks()[0].end_lba(), 3);
}

#[test]
fn reconstructed_chd_tracks_match_cue_disc_and_mod_identity() {
    let cue = b"FILE \"one.bin\" BINARY\nTRACK 01 MODE1/2352\nINDEX 01 00:00:00\nFILE \"two.bin\" BINARY\nTRACK 02 AUDIO\nINDEX 00 00:00:00\nINDEX 01 00:00:01\n";
    let sheet = parse_cue_bytes(cue).unwrap();
    let first = vec![0x11; 2 * 2_352];
    let second = vec![0x22; 2 * 2_352];
    let loaded = build_disc(cue.to_vec(), &sheet, vec![first.clone(), second.clone()]).unwrap();
    let tracks = vec![
        ChdTrack {
            number: 1,
            mode: CdTrackMode::Mode1_2352,
            frames: 2,
            pregap: 0,
        },
        ChdTrack {
            number: 2,
            mode: CdTrackMode::Audio,
            frames: 2,
            pregap: 1,
        },
    ];
    let chd_disc = build_chd_disc(&tracks, &[first, second]).unwrap();
    assert_eq!(chd_disc, loaded.disc);
    assert_eq!(crc32fast::hash(&chd_disc.content_hash()), loaded.mod_crc32);
}

#[test]
fn chd_audio_payload_is_normalized_for_the_cd_audio_reader() {
    let tracks = vec![ChdTrack {
        number: 1,
        mode: CdTrackMode::Audio,
        frames: 1,
        pregap: 0,
    }];
    let payload = vec![0x12, 0x34, 0x56, 0x78]
        .into_iter()
        .cycle()
        .take(2_352)
        .collect::<Vec<_>>();
    let mut payloads = vec![payload];
    normalize_chd_audio_payloads(&tracks, &mut payloads);
    let disc = build_chd_disc(&tracks, &payloads).unwrap();
    assert_eq!(disc.read_audio_sample(0, 0), Ok((0x1234, 0x5678)));
}

#[test]
fn chd_audio_xdelta_targets_the_normalized_track_payload() {
    let tracks = vec![ChdTrack {
        number: 1,
        mode: CdTrackMode::Audio,
        frames: 1,
        pregap: 0,
    }];
    let raw = vec![0x12, 0x34, 0x56, 0x78]
        .into_iter()
        .cycle()
        .take(2_352)
        .collect::<Vec<_>>();
    let mut payloads = vec![raw];
    normalize_chd_audio_payloads(&tracks, &mut payloads);
    let source = payloads[0].clone();
    let mut expected = source.clone();
    expected[..4].copy_from_slice(&[0xCD, 0xAB, 0x34, 0x12]);

    let dir =
        std::env::temp_dir().join(format!("zeff-pce-chd-audio-xdelta-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("translation-track01.xdelta"),
        xdelta3::encode(&expected, &source).unwrap(),
    )
    .unwrap();
    let targets = vec![crate::mods::PceCdPatchTarget::Track {
        number: 1,
        segment: 0,
        bytes: 0..payloads[0].len(),
    }];
    let entries = vec![crate::mods::ModEntry {
        filename: "translation-track01.xdelta".to_owned(),
        enabled: true,
        target: None,
    }];

    assert!(
        crate::mods::apply_enabled_pce_cd_mods(&mut payloads, &targets, &dir, &entries).is_empty()
    );
    assert_eq!(payloads[0], expected);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn direct_loader_mounts_cd_with_test_system_card() {
    let bin = vec![0; 2_048];
    let cue = "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
    let cue_path = temp_set("loader", &[("disc.bin", &bin)], cue);
    let system_card: &'static [u8] = Box::leak(vec![0; 262_144].into_boxed_slice());
    let config = BackendLoadConfig {
        pce_cd_system_card_override: Some(system_card),
        pce_cd_system_card_sha256_override: Some(zeff_firmware::PCE_SYSTEM_CARD_V3_USA_SHA256),
        pce_console_wiring: Some(zeff_pce_core::hardware::PceConsoleWiring::TurboGrafx16),
        ..BackendLoadConfig::default()
    };
    let loaded =
        load_backend_from_rom_source(ActiveSystem::Pce, &cue_path, &cue_path, None, config)
            .unwrap();
    let EmuBackend::Pce(backend) = loaded.backend else {
        panic!("CUE loader returned a non-PCE backend");
    };
    assert_eq!(
        backend.hucard_board(),
        zeff_pce_core::hardware::PceHuCardBoard::SystemCardV3
    );
    assert_eq!(backend.source_path(), cue_path);
}

#[test]
fn direct_iso_route_mounts_the_referencing_cue() {
    let data = vec![0; 2_048];
    let cue = "FILE \"disc.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
    let cue_path = temp_set("iso-loader", &[("disc.iso", &data)], cue);
    let iso_path = cue_path.with_extension("iso");
    let system_card: &'static [u8] = Box::leak(vec![0; 262_144].into_boxed_slice());
    let loaded = load_backend_from_rom_source(
        ActiveSystem::Pce,
        &iso_path,
        &iso_path,
        None,
        BackendLoadConfig {
            pce_cd_system_card_override: Some(system_card),
            pce_cd_system_card_sha256_override: Some(zeff_firmware::PCE_SYSTEM_CARD_V3_USA_SHA256),
            pce_console_wiring: Some(zeff_pce_core::hardware::PceConsoleWiring::TurboGrafx16),
            ..BackendLoadConfig::default()
        },
    )
    .unwrap();
    let EmuBackend::Pce(backend) = loaded.backend else {
        panic!("ISO loader returned a non-PCE backend");
    };
    assert_eq!(backend.rom_path(), cue_path);
    assert_eq!(backend.source_path(), iso_path);
}

#[test]
fn direct_loader_rejects_exact_system_card_from_the_wrong_region() {
    let bin = vec![0; 2_048];
    let cue = "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
    let cue_path = temp_set("wrong-region", &[("disc.bin", &bin)], cue);
    let system_card: &'static [u8] = Box::leak(vec![0; 262_144].into_boxed_slice());
    let error = load_backend_from_rom_source(
        ActiveSystem::Pce,
        &cue_path,
        &cue_path,
        None,
        BackendLoadConfig {
            pce_cd_system_card_override: Some(system_card),
            pce_cd_system_card_sha256_override: Some(zeff_firmware::PCE_SYSTEM_CARD_V3_USA_SHA256),
            pce_console_wiring: Some(zeff_pce_core::hardware::PceConsoleWiring::PcEngine),
            ..BackendLoadConfig::default()
        },
    )
    .err()
    .unwrap()
    .downcast::<PceCdLoadError>()
    .unwrap();
    assert_eq!(
        error,
        PceCdLoadError::SystemCardRegionMismatch {
            expected: zeff_firmware::PceSystemCardRegion::Japan,
            actual: zeff_firmware::PceSystemCardRegion::Usa,
        }
    );
}
