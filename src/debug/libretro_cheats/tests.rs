use super::*;

#[test]
fn search_filenames_basic() {
    let files = vec![
        "Pokemon Red Version (USA).cht".to_string(),
        "Pokemon Blue Version (USA).cht".to_string(),
        "Super Mario Land (World).cht".to_string(),
        "Tetris (World).cht".to_string(),
    ];

    let results = search_filenames("pokemon", &files, 50);
    assert_eq!(results.len(), 2);
    assert!(results[0].contains("Pokemon"));

    let results = search_filenames("mario", &files, 50);
    assert_eq!(results.len(), 1);

    let results = search_filenames("", &files, 50);
    assert_eq!(results.len(), 4);
}

#[test]
fn search_filenames_multi_term() {
    let files = vec![
        "Pokemon Red Version (USA).cht".to_string(),
        "Pokemon Blue Version (USA).cht".to_string(),
    ];
    let results = search_filenames("pokemon red", &files, 50);
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
        "Pokemon Red Version (USA, Europe).cht".to_string(),
        "Pokemon Blue Version (USA, Europe).cht".to_string(),
        "Tetris (World).cht".to_string(),
    ];
    let hints = vec!["Pokemon Red Version".to_string()];
    let results = search_filenames_with_hints("", &files, 10, &hints);
    assert_eq!(
        results.first().unwrap(),
        "Pokemon Red Version (USA, Europe).cht"
    );
}

#[test]
fn parse_dir_entry_sha_finds_platform() {
    let json = r#"[{"name":"Nintendo - Game Boy","sha":"abc123def","type":"dir"},{"name":"Nintendo - Nintendo Entertainment System","sha":"nes456sha","type":"dir"}]"#;
    let sha =
        parse_dir_entry_sha(json, "Nintendo - Nintendo Entertainment System").unwrap();
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
    let names = parse_tree_blob_names(json);
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"Game A (USA).cht".to_string()));
    assert!(names.contains(&"Game B.cht".to_string()));
}

#[test]
fn parse_tree_blob_names_handles_empty_tree() {
    let json = r#"{"sha":"abc","tree":[],"truncated":false}"#;
    let names = parse_tree_blob_names(json);
    assert!(names.is_empty());
}

#[test]
fn urlencoded_handles_spaces() {
    assert_eq!(
        urlencoded("Nintendo - Game Boy"),
        "Nintendo%20-%20Game%20Boy"
    );
}
