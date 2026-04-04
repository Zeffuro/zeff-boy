use super::*;

#[test]
fn discover_empty_dir() {
    let dir = std::env::temp_dir().join("zeff_test_mods_empty");
    let _ = std::fs::create_dir_all(&dir);
    let mods = discover_mods(&dir);
    assert!(
        mods.is_empty()
            || mods.iter().all(|m| {
                m.filename.ends_with(".ips")
                    || m.filename.ends_with(".bps")
                    || m.filename.ends_with(".ups")
            })
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn discover_finds_ips_and_bps_files() {
    let dir = std::env::temp_dir().join("zeff_test_mods_discover_both");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(dir.join("patch_a.ips"), b"PATCHEOF").unwrap();
    std::fs::write(dir.join("patch_b.IPS"), b"PATCHEOF").unwrap();
    std::fs::write(dir.join("patch_c.bps"), make_test_bps(&[0; 4], &[0; 4])).unwrap();
    std::fs::write(dir.join("readme.txt"), b"not a patch").unwrap();
    let mods = discover_mods(&dir);
    let names: Vec<&str> = mods.iter().map(|m| m.filename.as_str()).collect();
    assert!(names.contains(&"patch_a.ips"));
    assert!(names.contains(&"patch_b.IPS"));
    assert!(names.contains(&"patch_c.bps"));
    assert!(!names.iter().any(|n| n.contains("readme")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn discover_finds_ups_files() {
    let dir = std::env::temp_dir().join("zeff_test_mods_discover_ups");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(dir.join("patch.ups"), make_test_ups(&[0; 4], &[0; 4])).unwrap();
    std::fs::write(dir.join("not_ups.ups"), b"NOPE not a real ups file here").unwrap();
    let mods = discover_mods(&dir);
    let names: Vec<&str> = mods.iter().map(|m| m.filename.as_str()).collect();
    assert!(names.contains(&"patch.ups"));
    assert!(!names.contains(&"not_ups.ups"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_save_roundtrip() {
    let dir = std::env::temp_dir().join("zeff_test_mods_roundtrip");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(dir.join("hack.ips"), b"PATCHEOF").unwrap();

    let mut mods = load_mod_config(&dir);
    assert_eq!(mods.len(), 1);
    assert!(!mods[0].enabled);

    mods[0].enabled = true;
    save_mod_config(&dir, &mods);

    let reloaded = load_mod_config(&dir);
    assert_eq!(reloaded.len(), 1);
    assert!(reloaded[0].enabled);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn apply_enabled_mods_applies_ips_patches() {
    let dir = std::env::temp_dir().join("zeff_test_mods_apply_ips");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);

    let mut patch = Vec::new();
    patch.extend_from_slice(b"PATCH");
    patch.extend_from_slice(&[0x00, 0x00, 0x02, 0x00, 0x01, 0xFF]);
    patch.extend_from_slice(b"EOF");
    std::fs::write(dir.join("test.ips"), &patch).unwrap();

    let entries = vec![ModEntry {
        filename: "test.ips".to_string(),
        enabled: true,
    }];
    let mut rom = vec![0u8; 16];
    let warnings = apply_enabled_mods(&mut rom, &dir, &entries);
    assert!(warnings.is_empty());
    assert_eq!(rom[2], 0xFF);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn apply_enabled_mods_applies_bps_patches() {
    let dir = std::env::temp_dir().join("zeff_test_mods_apply_bps");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);

    let source = vec![0u8; 8];
    let target = vec![0xAA, 0xBB, 0xCC, 0xDD, 0x00, 0x00, 0x00, 0x00];
    let patch = make_test_bps(&source, &target);
    std::fs::write(dir.join("test.bps"), &patch).unwrap();

    let entries = vec![ModEntry {
        filename: "test.bps".to_string(),
        enabled: true,
    }];
    let mut rom = source;
    let warnings = apply_enabled_mods(&mut rom, &dir, &entries);
    assert!(warnings.is_empty());
    assert_eq!(rom, target);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn apply_enabled_mods_applies_ups_patches() {
    let dir = std::env::temp_dir().join("zeff_test_mods_apply_ups");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);

    let source = vec![0u8; 8];
    let target = vec![0xAA, 0xBB, 0xCC, 0xDD, 0x00, 0x00, 0x00, 0x00];
    let patch = make_test_ups(&source, &target);
    std::fs::write(dir.join("test.ups"), &patch).unwrap();

    let entries = vec![ModEntry {
        filename: "test.ups".to_string(),
        enabled: true,
    }];
    let mut rom = source;
    let warnings = apply_enabled_mods(&mut rom, &dir, &entries);
    assert!(warnings.is_empty());
    assert_eq!(rom, target);

    let _ = std::fs::remove_dir_all(&dir);
}

fn make_test_bps(source: &[u8], target: &[u8]) -> Vec<u8> {
    fn encode_varint(mut value: u64) -> Vec<u8> {
        let mut buf = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                byte |= 0x80;
                buf.push(byte);
                break;
            }
            buf.push(byte);
            value -= 1;
        }
        buf
    }

    let mut patch = Vec::new();
    patch.extend_from_slice(b"BPS1");
    patch.extend(encode_varint(source.len() as u64));
    patch.extend(encode_varint(target.len() as u64));
    patch.extend(encode_varint(0));

    let cmd = ((target.len() as u64 - 1) << 2) | 1;
    patch.extend(encode_varint(cmd));
    patch.extend_from_slice(target);

    let source_crc = crc32fast::hash(source);
    let target_crc = crc32fast::hash(target);
    patch.extend_from_slice(&source_crc.to_le_bytes());
    patch.extend_from_slice(&target_crc.to_le_bytes());
    let patch_crc = crc32fast::hash(&patch);
    patch.extend_from_slice(&patch_crc.to_le_bytes());
    patch
}

fn make_test_ups(source: &[u8], target: &[u8]) -> Vec<u8> {
    fn encode_varint(mut value: u64) -> Vec<u8> {
        let mut buf = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                byte |= 0x80;
                buf.push(byte);
                break;
            }
            buf.push(byte);
            value -= 1;
        }
        buf
    }

    let mut patch = Vec::new();
    patch.extend_from_slice(b"UPS1");
    patch.extend(encode_varint(source.len() as u64));
    patch.extend(encode_varint(target.len() as u64));

    let max_len = source.len().max(target.len());
    let mut write_offset: usize = 0;
    let mut i = 0;
    while i < max_len {
        let s = if i < source.len() { source[i] } else { 0 };
        let t = if i < target.len() { target[i] } else { 0 };
        if s != t {
            let skip = i - write_offset;
            patch.extend(encode_varint(skip as u64));
            while i < max_len {
                let s = if i < source.len() { source[i] } else { 0 };
                let t = if i < target.len() { target[i] } else { 0 };
                let xor = s ^ t;
                if xor == 0 {
                    break;
                }
                patch.push(xor);
                i += 1;
            }
            patch.push(0x00);
            write_offset = i + 1;
        }
        i += 1;
    }

    let source_crc = crc32fast::hash(source);
    let target_crc = crc32fast::hash(target);
    patch.extend_from_slice(&source_crc.to_le_bytes());
    patch.extend_from_slice(&target_crc.to_le_bytes());
    let patch_crc = crc32fast::hash(&patch);
    patch.extend_from_slice(&patch_crc.to_le_bytes());
    patch
}
