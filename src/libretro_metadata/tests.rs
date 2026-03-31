use super::*;

#[test]
fn parse_dat_entries_extracts_crc_title_and_rom_name() {
    let dat = r#"
        game (
            name "Pokemon Red Version (USA, Europe)"
            rom ( name "Pokemon Red Version (USA, Europe) (SGB Enhanced).gb" size 524288 crc D7037C83 md5 0 sha1 0 )
        )
    "#;

    let entries = parse_dat_entries(dat, false);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].crc32, 0xD7037C83);
    assert_eq!(entries[0].title, "Pokemon Red Version (USA, Europe)");
    assert_eq!(
        entries[0].rom_name,
        "Pokemon Red Version (USA, Europe) (SGB Enhanced).gb"
    );
}

#[test]
fn build_cheat_search_hints_prefers_metadata_aliases() {
    let meta = RomMetadata {
        crc32: 0,
        title: "Pokemon Red Version (USA, Europe)".to_string(),
        rom_name: "Pokemon Red Version (USA, Europe) (SGB Enhanced).gb".to_string(),
        is_gbc: false,
    };

    let hints = build_cheat_search_hints("POKEMON RED", Some(&meta));
    assert!(hints.iter().any(|h| h.contains("Pokemon Red Version")));
    assert!(hints.iter().any(|h| h == "pokemon red version usa europe"));
}

#[test]
fn serialize_roundtrip_preserves_entries() {
    let entries = vec![RomMetadata {
        crc32: 0x1234ABCD,
        title: "Test Game".to_string(),
        rom_name: "Test Game.gb".to_string(),
        is_gbc: false,
    }];

    let bytes = serialize_entries(&entries).unwrap();
    let parsed = deserialize_entries(&bytes).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].crc32, 0x1234ABCD);
    assert_eq!(parsed[0].title, "Test Game");
    assert_eq!(parsed[0].rom_name, "Test Game.gb");
    assert!(!parsed[0].is_gbc);
}
