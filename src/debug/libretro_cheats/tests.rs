use super::*;

#[test]
fn search_filenames_basic() {
    let files = vec![
        "Example Red Version (USA).cht".to_string(),
        "Example Blue Version (USA).cht".to_string(),
        "Platformer Land (World).cht".to_string(),
        "Block Puzzle (World).cht".to_string(),
    ];

    let results = search_filenames("example", &files, 50);
    assert_eq!(results.len(), 2);
    assert!(results[0].contains("Example"));

    let results = search_filenames("platformer", &files, 50);
    assert_eq!(results.len(), 1);

    let results = search_filenames("", &files, 50);
    assert_eq!(results.len(), 4);
}

#[test]
fn search_filenames_multi_term() {
    let files = vec![
        "Example Red Version (USA).cht".to_string(),
        "Example Blue Version (USA).cht".to_string(),
    ];
    let results = search_filenames("example red", &files, 50);
    assert_eq!(results.len(), 1);
    assert!(results[0].contains("Red"));
}

#[test]
fn search_filenames_respects_limit() {
    let files: Vec<String> = (0..100).map(|i| format!("Game {i}.cht")).collect();
    let results = search_filenames("game", &files, 10);
    assert_eq!(results.len(), 10);
}

#[test]
fn search_filenames_with_hints_prefers_exact_like_title() {
    let files = vec![
        "Example Red Version (USA, Europe).cht".to_string(),
        "Example Blue Version (USA, Europe).cht".to_string(),
        "Block Puzzle (World).cht".to_string(),
    ];
    let hints = vec!["Example Red Version".to_string()];
    let results = search_filenames_with_hints("", &files, 10, &hints);
    assert_eq!(
        results.first().unwrap(),
        "Example Red Version (USA, Europe).cht"
    );
}

#[test]
fn parse_dir_entry_sha_finds_platform() {
    let json = r#"[{"name":"Nintendo - Game Boy","sha":"abc123def","type":"dir"},{"name":"Nintendo - Nintendo Entertainment System","sha":"nes456sha","type":"dir"}]"#;
    let sha = parse_dir_entry_sha(json, "Nintendo - Nintendo Entertainment System").unwrap();
    assert_eq!(sha, "nes456sha");
}

#[test]
fn parse_dir_entry_sha_returns_error_for_missing() {
    let json = r#"[{"name":"Nintendo - Game Boy","sha":"abc123","type":"dir"}]"#;
    let result = parse_dir_entry_sha(json, "NonExistent Platform");
    assert!(result.is_err());
}

#[test]
fn parse_tree_blob_names_extracts_cht_files() {
    let json = r#"{"sha":"abc","tree":[{"path":"Game A (USA).cht","mode":"100644","type":"blob","sha":"x"},{"path":"Game B.cht","mode":"100644","type":"blob","sha":"y"},{"path":"README.md","mode":"100644","type":"blob","sha":"z"}],"truncated":false}"#;
    let names = parse_tree_blob_names(json, LibretroPlatform::Gb);
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"Game A (USA).cht".to_string()));
    assert!(names.contains(&"Game B.cht".to_string()));
}

#[test]
fn parse_tree_blob_names_handles_empty_tree() {
    let json = r#"{"sha":"abc","tree":[],"truncated":false}"#;
    let names = parse_tree_blob_names(json, LibretroPlatform::Pce);
    assert!(names.is_empty());
}

#[test]
fn parse_tree_blob_names_includes_pce_text_files_only_for_pce() {
    let json = r#"{"tree":[{"path":"Game.cht"},{"path":"Legacy.txt"},{"path":"README.md"}]}"#;
    assert_eq!(
        parse_tree_blob_names(json, LibretroPlatform::Pce),
        ["Game.cht", "Legacy.txt"]
    );
    assert_eq!(
        parse_tree_blob_names(json, LibretroPlatform::Gb),
        ["Game.cht"]
    );
}

#[test]
fn urlencoded_handles_spaces() {
    assert_eq!(
        urlencoded("Nintendo - Game Boy"),
        "Nintendo%20-%20Game%20Boy"
    );
}

#[test]
fn md_database_platform_directory_extensions_index_and_cache_are_isolated() {
    let platform = LibretroPlatform::Md;
    assert_eq!(platform as u8, 9);
    for (index, peer) in [
        LibretroPlatform::Gb,
        LibretroPlatform::Gbc,
        LibretroPlatform::Nes,
        LibretroPlatform::Gba,
        LibretroPlatform::MasterSystem,
        LibretroPlatform::GameGear,
        LibretroPlatform::Pce,
        LibretroPlatform::PceCd,
        LibretroPlatform::SuperGrafx,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(peer as usize, index);
    }
    assert_eq!(platform.label(), "Mega Drive / Genesis");
    assert_eq!(platform.cache_suffix(), "md");
    assert_eq!(platform.rom_extensions(), &[".md", ".gen", ".bin", ".smd"]);
    assert_eq!(platform.cheat_extensions(), &[".cht"]);
    assert_eq!(
        browse_url(platform),
        "https://github.com/libretro/libretro-database/tree/master/cht/Sega%20-%20Mega%20Drive%20-%20Genesis"
    );
    assert_eq!(
        parse_dir_entry_sha(
            r#"[{"name":"Sega - Mega Drive - Genesis","sha":"md-tree"}]"#,
            platform.platform_dir()
        )
        .unwrap(),
        "md-tree"
    );
    let names = parse_tree_blob_names(
        r#"{"tree":[{"path":"Synthetic.cht"},{"path":"Legacy.txt"}]}"#,
        platform,
    );
    assert_eq!(names, ["Synthetic.cht"]);
    let base = std::env::temp_dir().join(format!(
        "zeff-md-db-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(base.join("libretro-cht/md")).unwrap();
    let peer = base.join("libretro-cht/Synthetic.cht");
    std::fs::write(&peer, "peer cache").unwrap();
    let md = download_cache_path("Synthetic.cht", platform, &base);
    assert_eq!(md, base.join("libretro-cht/md/Synthetic.cht"));
    assert!(!md.exists());
    assert_eq!(
        download_cache_path("Synthetic.cht", LibretroPlatform::Gb, &base),
        peer
    );
    assert_eq!(
        download_cache_path("nested/Synthetic.cht", platform, &base),
        base.join("libretro-cht/md/nested_Synthetic.cht")
    );
    std::fs::write(&md, "md cache").unwrap();
    assert_eq!(
        download_cht_content("Synthetic.cht", platform, &base).unwrap(),
        "md cache"
    );
    assert_eq!(
        download_cht_content("Synthetic.cht", LibretroPlatform::Gb, &base).unwrap(),
        "peer cache"
    );
    std::fs::write(base.join("libretro_md_index_v2.txt"), "Synthetic.cht").unwrap();
    assert_eq!(
        fetch_cheat_list(platform, &base).unwrap(),
        ["Synthetic.cht"]
    );
    assert_eq!(std::fs::read_to_string(peer).unwrap(), "peer cache");
    std::fs::remove_dir_all(base).unwrap();
}
