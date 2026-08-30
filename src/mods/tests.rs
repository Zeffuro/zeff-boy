use super::*;
use crate::test_support::test_directory;

#[test]
fn discover_empty_dir() {
    let temp = test_directory("mods-empty").unwrap();
    let dir = temp.path();
    let mods = discover_mods(dir);
    assert!(
        mods.is_empty()
            || mods.iter().all(|m| {
                m.filename.ends_with(".ips")
                    || m.filename.ends_with(".bps")
                    || m.filename.ends_with(".ups")
                    || m.filename.ends_with(".ppf")
                    || m.filename.ends_with(".xdelta")
                    || m.filename.ends_with(".xdelta3")
                    || m.filename.ends_with(".vcdiff")
            })
    );
}

#[test]
fn discover_finds_supported_patch_files() {
    let temp = test_directory("mods-discover-both").unwrap();
    let dir = temp.path();
    std::fs::write(dir.join("patch_a.ips"), b"PATCHEOF").unwrap();
    std::fs::write(dir.join("patch_b.IPS"), b"PATCHEOF").unwrap();
    std::fs::write(dir.join("patch_c.bps"), make_test_bps(&[0; 4], &[0; 4])).unwrap();
    std::fs::write(dir.join("patch_d.ppf"), b"PPF30\x02").unwrap();
    std::fs::write(dir.join("patch_e.xdelta"), b"\xD6\xC3\xC4\0\0").unwrap();
    std::fs::write(dir.join("patch_f.xdelta3"), b"\xD6\xC3\xC4\0\0").unwrap();
    std::fs::write(dir.join("patch_g.vcdiff"), b"\xD6\xC3\xC4\0\0").unwrap();
    std::fs::write(dir.join("readme.txt"), b"not a patch").unwrap();
    let mods = discover_mods(dir);
    let names: Vec<&str> = mods.iter().map(|m| m.filename.as_str()).collect();
    assert!(names.contains(&"patch_a.ips"));
    assert!(names.contains(&"patch_b.IPS"));
    assert!(names.contains(&"patch_c.bps"));
    assert!(names.contains(&"patch_d.ppf"));
    assert!(names.contains(&"patch_e.xdelta"));
    assert!(names.contains(&"patch_f.xdelta3"));
    assert!(names.contains(&"patch_g.vcdiff"));
    assert!(!names.iter().any(|n| n.contains("readme")));
}

#[test]
fn discover_finds_ups_files() {
    let temp = test_directory("mods-discover-ups").unwrap();
    let dir = temp.path();
    std::fs::write(
        dir.join("patch.ups"),
        crate::patching::ups::make_ups(&[0; 4], &[0; 4]),
    )
    .unwrap();
    std::fs::write(dir.join("not_ups.ups"), b"NOPE not a real ups file here").unwrap();
    let mods = discover_mods(dir);
    let names: Vec<&str> = mods.iter().map(|m| m.filename.as_str()).collect();
    assert!(names.contains(&"patch.ups"));
    assert!(!names.contains(&"not_ups.ups"));
}

#[test]
fn load_save_roundtrip() {
    let temp = test_directory("mods-roundtrip").unwrap();
    let dir = temp.path();
    std::fs::write(dir.join("hack.ips"), b"PATCHEOF").unwrap();

    let mut mods = load_mod_config(dir);
    assert_eq!(mods.len(), 1);
    assert!(!mods[0].enabled);

    mods[0].enabled = true;
    mods[0].target = Some("Track 02.bin".to_owned());
    save_mod_config(dir, &mods);

    let reloaded = load_mod_config(dir);
    assert_eq!(reloaded.len(), 1);
    assert!(reloaded[0].enabled);
    assert_eq!(reloaded[0].target.as_deref(), Some("Track 02.bin"));
}

#[test]
fn load_preserves_saved_patch_order_and_appends_new_files() {
    let temp = test_directory("mods-order").unwrap();
    let dir = temp.path();
    for name in ["a.ips", "b.ips", "c.ips"] {
        std::fs::write(dir.join(name), b"PATCHEOF").unwrap();
    }
    save_mod_config(
        dir,
        &[
            ModEntry {
                filename: "b.ips".to_owned(),
                enabled: true,
                target: None,
            },
            ModEntry {
                filename: "a.ips".to_owned(),
                enabled: false,
                target: None,
            },
        ],
    );

    let loaded = load_mod_config(dir);
    assert_eq!(
        loaded
            .iter()
            .map(|entry| entry.filename.as_str())
            .collect::<Vec<_>>(),
        ["b.ips", "a.ips", "c.ips"]
    );
    assert!(loaded[0].enabled);
}

#[test]
fn apply_enabled_mods_applies_ips_patches() {
    let temp = test_directory("mods-apply-ips").unwrap();
    let dir = temp.path();

    let mut patch = Vec::new();
    patch.extend_from_slice(b"PATCH");
    patch.extend_from_slice(&[0x00, 0x00, 0x02, 0x00, 0x01, 0xFF]);
    patch.extend_from_slice(b"EOF");
    std::fs::write(dir.join("test.ips"), &patch).unwrap();

    let entries = vec![ModEntry {
        filename: "test.ips".to_string(),
        enabled: true,
        target: None,
    }];
    let mut rom = vec![0u8; 16];
    let warnings = apply_enabled_mods(&mut rom, dir, &entries);
    assert!(warnings.is_empty());
    assert_eq!(rom[2], 0xFF);
}

#[test]
fn apply_enabled_pce_cd_mods_targets_track_from_xdelta_filename() {
    let temp = test_directory("mods-apply-pce-cd-xdelta").unwrap();
    let dir = temp.path();

    let source = b"original data track".to_vec();
    let mut target = source.clone();
    target[..5].copy_from_slice(b"local");
    let patch = xdelta3::encode(&target, &source).unwrap();
    std::fs::write(dir.join("translation-track02.xdelta"), patch).unwrap();
    let entries = vec![ModEntry {
        filename: "translation-track02.xdelta".to_owned(),
        enabled: true,
        target: None,
    }];
    let mut files = vec![vec![0; source.len()], source];
    let targets = vec![
        PceCdPatchTarget::File {
            reference: "Game (Track 01).bin".to_owned(),
            segment: 0,
        },
        PceCdPatchTarget::File {
            reference: "Game (Track 02).bin".to_owned(),
            segment: 1,
        },
        PceCdPatchTarget::Track {
            number: 1,
            segment: 0,
            bytes: 0..files[0].len(),
        },
        PceCdPatchTarget::Track {
            number: 2,
            segment: 1,
            bytes: 0..files[1].len(),
        },
    ];

    let warnings = apply_enabled_pce_cd_mods(&mut files, &targets, dir, &entries);

    assert!(warnings.is_empty());
    assert_eq!(files[1], target);
}

#[test]
fn apply_enabled_pce_cd_mods_allows_a_whole_track_to_change_length() {
    let temp = test_directory("mods-apply-pce-cd-longer-track").unwrap();
    let dir = temp.path();

    let source = b"short track".to_vec();
    let target = b"a replacement track with a different length".to_vec();
    std::fs::write(
        dir.join("dub-track02.xdelta"),
        xdelta3::encode(&target, &source).unwrap(),
    )
    .unwrap();
    let entries = vec![ModEntry {
        filename: "dub-track02.xdelta".to_owned(),
        enabled: true,
        target: None,
    }];
    let mut files = vec![vec![0; 8], source];
    let targets = vec![PceCdPatchTarget::Track {
        number: 2,
        segment: 1,
        bytes: 0..files[1].len(),
    }];

    let warnings = apply_enabled_pce_cd_mods(&mut files, &targets, dir, &entries);

    assert!(warnings.is_empty());
    assert_eq!(files[1], target);
}

#[test]
fn apply_enabled_mods_applies_bps_patches() {
    let temp = test_directory("mods-apply-bps").unwrap();
    let dir = temp.path();

    let source = vec![0u8; 8];
    let target = vec![0xAA, 0xBB, 0xCC, 0xDD, 0x00, 0x00, 0x00, 0x00];
    let patch = make_test_bps(&source, &target);
    std::fs::write(dir.join("test.bps"), &patch).unwrap();

    let entries = vec![ModEntry {
        filename: "test.bps".to_string(),
        enabled: true,
        target: None,
    }];
    let mut rom = source;
    let warnings = apply_enabled_mods(&mut rom, dir, &entries);
    assert!(warnings.is_empty());
    assert_eq!(rom, target);
}

#[test]
fn apply_enabled_mods_applies_ups_patches() {
    let temp = test_directory("mods-apply-ups").unwrap();
    let dir = temp.path();

    let source = vec![0u8; 8];
    let target = vec![0xAA, 0xBB, 0xCC, 0xDD, 0x00, 0x00, 0x00, 0x00];
    let patch = crate::patching::ups::make_ups(&source, &target);
    std::fs::write(dir.join("test.ups"), &patch).unwrap();

    let entries = vec![ModEntry {
        filename: "test.ups".to_string(),
        enabled: true,
        target: None,
    }];
    let mut rom = source;
    let warnings = apply_enabled_mods(&mut rom, dir, &entries);
    assert!(warnings.is_empty());
    assert_eq!(rom, target);
}

#[test]
fn apply_enabled_pce_cd_mods_treats_cue_files_as_one_image() {
    let temp = test_directory("mods-apply-pce-cd-ppf").unwrap();
    let dir = temp.path();

    let mut patch = b"PPF30\x02".to_vec();
    patch.resize(60, 0);
    patch.extend_from_slice(&3_u64.to_le_bytes());
    patch.push(3);
    patch.extend_from_slice(&[1, 2, 3]);
    std::fs::write(dir.join("disc.ppf"), patch).unwrap();

    let entries = vec![ModEntry {
        filename: "disc.ppf".to_owned(),
        enabled: true,
        target: None,
    }];
    assert_eq!(mod_advisories(dir, &entries).len(), 1);
    let mut files = vec![vec![0; 4], vec![0; 4]];
    let targets = vec![
        PceCdPatchTarget::File {
            reference: "Track 01.bin".to_owned(),
            segment: 0,
        },
        PceCdPatchTarget::File {
            reference: "Track 02.bin".to_owned(),
            segment: 1,
        },
    ];
    let warnings = apply_enabled_pce_cd_mods(&mut files, &targets, dir, &entries);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("no source check"));
    assert_eq!(files, vec![vec![0, 0, 0, 1], vec![2, 3, 0, 0]]);
}

fn make_test_bps(source: &[u8], target: &[u8]) -> Vec<u8> {
    let mut patch = Vec::new();
    patch.extend_from_slice(b"BPS1");
    patch.extend(crate::patching::encode_varint(source.len() as u64));
    patch.extend(crate::patching::encode_varint(target.len() as u64));
    patch.extend(crate::patching::encode_varint(0));

    let cmd = ((target.len() as u64 - 1) << 2) | 1;
    patch.extend(crate::patching::encode_varint(cmd));
    patch.extend_from_slice(target);

    crate::patching::append_patch_crcs(&mut patch, source, target);
    patch
}
