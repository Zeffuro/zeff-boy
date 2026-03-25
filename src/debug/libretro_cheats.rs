use std::path::{Path, PathBuf};

const GITHUB_CONTENTS_URL: &str =
    "https://api.github.com/repos/libretro/libretro-database/contents/cht/";
const RAW_BASE_URL: &str =
    "https://raw.githubusercontent.com/libretro/libretro-database/master/cht/";

fn platform_dir(is_gbc: bool) -> &'static str {
    if is_gbc {
        "Nintendo - Game Boy Color"
    } else {
        "Nintendo - Game Boy"
    }
}

pub(super) fn fetch_cheat_list(is_gbc: bool, cache_dir: &Path) -> Result<Vec<String>, String> {
    let cache_file = cache_dir.join(if is_gbc {
        "libretro_gbc_index.json"
    } else {
        "libretro_gb_index.json"
    });

    if let Ok(meta) = std::fs::metadata(&cache_file)
        && let Ok(modified) = meta.modified()
            && modified.elapsed().unwrap_or_default().as_secs() < 86400
                && let Ok(content) = std::fs::read_to_string(&cache_file)
                    && let Ok(names) = parse_file_list_from_json(&content)
                        && !names.is_empty() {
                            return Ok(names);
                        }

    let dir = platform_dir(is_gbc);
    let url = format!("{}{}", GITHUB_CONTENTS_URL, urlencoded(dir));
    log::info!("Fetching libretro cheat index from {}", url);

    let response = ureq::get(&url)
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "zeff-boy-emulator")
        .call()
        .map_err(|e| format!("GitHub API request failed: {e}"))?;

    let body = response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("Failed to read response: {e}"))?;

    let names = parse_file_list_from_json(&body)?;

    let _ = std::fs::create_dir_all(cache_dir);
    let _ = std::fs::write(&cache_file, &body);

    Ok(names)
}

pub(super) fn download_cht_content(
    filename: &str,
    is_gbc: bool,
    cache_dir: &Path,
) -> Result<String, String> {
    let cht_cache_dir = cache_dir.join("libretro-cht");
    let safe_name = filename.replace(['/', '\\', ':'], "_");
    let cache_file = cht_cache_dir.join(&safe_name);

    // Try disk cache
    if let Ok(content) = std::fs::read_to_string(&cache_file) {
        return Ok(content);
    }

    let dir = platform_dir(is_gbc);
    let url = format!(
        "{}{}/{}",
        RAW_BASE_URL,
        urlencoded(dir),
        urlencoded(filename)
    );
    log::info!("Downloading cheat file from {}", url);

    let response = ureq::get(&url)
        .header("User-Agent", "zeff-boy-emulator")
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;

    let content = response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("Failed to read file: {e}"))?;

    let _ = std::fs::create_dir_all(&cht_cache_dir);
    let _ = std::fs::write(&cache_file, &content);

    Ok(content)
}

pub(super) fn search_filenames(query: &str, file_list: &[String], limit: usize) -> Vec<String> {
    if query.is_empty() {
        return file_list.iter().take(limit).cloned().collect();
    }
    let query_lower = query.to_lowercase();
    let terms: Vec<&str> = query_lower.split_whitespace().collect();

    file_list
        .iter()
        .filter(|name| {
            let name_lower = name.to_lowercase();
            terms.iter().all(|term| name_lower.contains(term))
        })
        .take(limit)
        .cloned()
        .collect()
}

fn normalized_words(input: &str) -> Vec<String> {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(|v| v.to_string())
        .collect()
}

fn title_core(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut depth_round = 0usize;
    let mut depth_square = 0usize;

    for c in input.chars() {
        match c {
            '(' => depth_round += 1,
            ')' => depth_round = depth_round.saturating_sub(1),
            '[' => depth_square += 1,
            ']' => depth_square = depth_square.saturating_sub(1),
            _ if depth_round == 0 && depth_square == 0 => out.push(c),
            _ => {}
        }
    }

    normalized_words(out.trim()).join(" ")
}

fn score_filename(candidate: &str, query_terms: &[String], hints: &[String]) -> i32 {
    let candidate_no_ext = candidate.trim_end_matches(".cht");
    let candidate_words = normalized_words(candidate_no_ext);
    let candidate_folded = candidate_words.join(" ");
    let candidate_core = title_core(candidate_no_ext);
    let mut score = 0i32;

    if !query_terms.is_empty() {
        let all_query_terms_match = query_terms
            .iter()
            .all(|term| candidate_folded.contains(term));
        if !all_query_terms_match {
            return i32::MIN / 2;
        }
        score += 10_000;
        score += (query_terms.len() as i32) * 50;
    }

    for hint in hints {
        let hint_words = normalized_words(hint);
        if hint_words.is_empty() {
            continue;
        }
        let hint_folded = hint_words.join(" ");
        let hint_core = title_core(hint);

        if !hint_core.is_empty() && !candidate_core.is_empty() {
            if candidate_core == hint_core {
                score += 3_000;
            } else if candidate_core.contains(&hint_core) || hint_core.contains(&candidate_core) {
                score += 1_200;
            }
        }

        if candidate_folded == hint_folded {
            score += 2_000;
            continue;
        }

        if candidate_folded.contains(&hint_folded) || hint_folded.contains(&candidate_folded) {
            score += 800;
        }

        let shared = hint_words
            .iter()
            .filter(|w| candidate_folded.contains(*w))
            .count() as i32;
        score += shared * 40;
    }

    score
}

pub(super) fn search_filenames_with_hints(
    query: &str,
    file_list: &[String],
    limit: usize,
    hints: &[String],
) -> Vec<String> {
    if hints.is_empty() {
        return search_filenames(query, file_list, limit);
    }

    let query_terms = normalized_words(query);

    let mut scored: Vec<(i32, &String)> = file_list
        .iter()
        .map(|name| (score_filename(name, &query_terms, hints), name))
        .filter(|(score, _)| *score > i32::MIN / 4)
        .collect();

    scored.sort_by(|(sa, na), (sb, nb)| sb.cmp(sa).then_with(|| na.cmp(nb)));

    scored
        .into_iter()
        .take(limit)
        .map(|(_, name)| name.clone())
        .collect()
}

pub(super) fn libretro_cache_dir() -> PathBuf {
    crate::settings::Settings::settings_dir().join("libretro-cache")
}

pub(super) fn browse_url(is_gbc: bool) -> String {
    let dir = platform_dir(is_gbc);
    format!(
        "https://github.com/libretro/libretro-database/tree/master/cht/{}",
        urlencoded(dir)
    )
}

fn urlencoded(s: &str) -> String {
    s.replace(' ', "%20")
}

fn parse_file_list_from_json(json_body: &str) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for entry in json_body.split(r#""name":"#).skip(1) {
        let trimmed = entry.trim_start();
        if !trimmed.starts_with('"') {
            continue;
        }
        let inner = &trimmed[1..];
        if let Some(close) = inner.find('"') {
            let name = &inner[..close];
            if name.ends_with(".cht") {
                names.push(name.to_string());
            }
        }
    }
    if names.is_empty() && json_body.len() > 100
        && json_body.contains("\"message\"")
            && let Some(msg_start) = json_body.find(r#""message":"#) {
                let rest = &json_body[msg_start + 11..];
                if let Some(end) = rest.find('"') {
                    return Err(format!("GitHub API: {}", &rest[..end]));
                }
            }
    Ok(names)
}

#[cfg(test)]
mod tests {
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
}
