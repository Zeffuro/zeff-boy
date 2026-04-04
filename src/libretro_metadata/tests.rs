use super::*;

#[test]
fn parse_dat_entries_extracts_crc_title_and_rom_name() {
    let dat = r#"
        game (
            name "Pokemon Red Version (USA, Europe)"
            rom ( name "Pokemon Red Version (USA, Europe) (SGB Enhanced).gb" size 524288 crc D7037C83 md5 0 sha1 0 )
        )
    "#;

    let entries = parse_dat_entries(dat, LibretroPlatform::Gb);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].crc32, 0xD7037C83);
    assert_eq!(entries[0].title, "Pokemon Red Version (USA, Europe)");
    assert_eq!(
        entries[0].rom_name,
        "Pokemon Red Version (USA, Europe) (SGB Enhanced).gb"
    );
    assert_eq!(entries[0].platform, LibretroPlatform::Gb);
}

#[test]
fn parse_dat_entries_nes() {
    let dat = r#"
        game (
            name "Super Mario Bros. (World)"
            rom ( name "Super Mario Bros. (World).nes" size 40976 crc 3337EC46 md5 0 sha1 0 )
        )
    "#;

    let entries = parse_dat_entries(dat, LibretroPlatform::Nes);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].crc32, 0x3337EC46);
    assert_eq!(entries[0].title, "Super Mario Bros. (World)");
    assert_eq!(entries[0].rom_name, "Super Mario Bros. (World).nes");
    assert_eq!(entries[0].platform, LibretroPlatform::Nes);
}

#[test]
fn build_cheat_search_hints_prefers_metadata_aliases() {
    let meta = RomMetadata {
        crc32: 0,
        title: "Pokemon Red Version (USA, Europe)".to_string(),
        rom_name: "Pokemon Red Version (USA, Europe) (SGB Enhanced).gb".to_string(),
        platform: LibretroPlatform::Gb,
    };

    let hints = build_cheat_search_hints("POKEMON RED", Some(&meta));
    assert!(hints.iter().any(|h| h.contains("Pokemon Red Version")));
    assert!(hints.iter().any(|h| h == "pokemon red version usa europe"));
}

#[test]
fn build_cheat_search_hints_nes() {
    let meta = RomMetadata {
        crc32: 0,
        title: "Super Mario Bros. (World)".to_string(),
        rom_name: "Super Mario Bros. (World).nes".to_string(),
        platform: LibretroPlatform::Nes,
    };

    let hints = build_cheat_search_hints("SUPER MARIO BROS", Some(&meta));
    assert!(hints.iter().any(|h| h.contains("Super Mario Bros")));
}

#[test]
fn serialize_roundtrip_preserves_entries() {
    let entries = vec![
        RomMetadata {
            crc32: 0x1234ABCD,
            title: "Test Game".to_string(),
            rom_name: "Test Game.gb".to_string(),
            platform: LibretroPlatform::Gb,
        },
        RomMetadata {
            crc32: 0xDEADBEEF,
            title: "NES Test".to_string(),
            rom_name: "NES Test.nes".to_string(),
            platform: LibretroPlatform::Nes,
        },
    ];

    let bytes = serialize_entries(&entries).unwrap();
    let parsed = deserialize_entries(&bytes).unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].crc32, 0x1234ABCD);
    assert_eq!(parsed[0].title, "Test Game");
    assert_eq!(parsed[0].rom_name, "Test Game.gb");
    assert_eq!(parsed[0].platform, LibretroPlatform::Gb);
    assert_eq!(parsed[1].crc32, 0xDEADBEEF);
    assert_eq!(parsed[1].title, "NES Test");
    assert_eq!(parsed[1].rom_name, "NES Test.nes");
    assert_eq!(parsed[1].platform, LibretroPlatform::Nes);
}

#[test]
fn lookup_cached_filters_by_platform() {
    let entries = vec![RomMetadata {
        crc32: 0xAABBCCDD,
        title: "Some GB Game".to_string(),
        rom_name: "Some GB Game.gb".to_string(),
        platform: LibretroPlatform::Gb,
    }];

    let bytes = serialize_entries(&entries).unwrap();
    let parsed = deserialize_entries(&bytes).unwrap();
    let index = build_index(parsed);

    let gb_hit = index.by_crc.get(&0xAABBCCDD);
    assert!(gb_hit.is_some());
    assert_eq!(gb_hit.unwrap().platform, LibretroPlatform::Gb);

    let nes_hit = index
        .by_crc
        .get(&0xAABBCCDD)
        .filter(|e| e.platform == LibretroPlatform::Nes);
    assert!(nes_hit.is_none());
}
