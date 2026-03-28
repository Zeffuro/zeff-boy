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
fn parse_file_list_from_json_extracts_names() {
    let json = r#"[{"name":"Pokemon Red.cht","path":"cht/Pokemon Red.cht"},{"name":"Tetris.cht","path":"cht/Tetris.cht"},{"name":"README.md","path":"cht/README.md"}]"#;
    let names = parse_file_list_from_json(json).unwrap();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"Pokemon Red.cht".to_string()));
    assert!(names.contains(&"Tetris.cht".to_string()));
}

#[test]
fn parse_file_list_from_json_handles_error_message() {
    let json = r#"{"message":"API rate limit exceeded","documentation_url":"..."}"#;
    let result = parse_file_list_from_json(json);
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[test]
fn urlencoded_handles_spaces() {
    assert_eq!(
        urlencoded("Nintendo - Game Boy"),
        "Nintendo%20-%20Game%20Boy"
    );
}

