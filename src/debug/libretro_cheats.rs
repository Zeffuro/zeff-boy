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

    if let Ok(meta) = std::fs::metadata(&cache_file) {
        if let Ok(modified) = meta.modified() {
            if modified.elapsed().unwrap_or_default().as_secs() < 86400 {
                if let Ok(content) = std::fs::read_to_string(&cache_file) {
                    if let Ok(names) = parse_file_list_from_json(&content) {
                        if !names.is_empty() {
                            return Ok(names);
                        }
                    }
                }
            }
        }
    }

    let dir = platform_dir(is_gbc);
    let url = format!("{}{}", GITHUB_CONTENTS_URL, urlencoded(dir));
    log::info!("Fetching libretro cheat index from {}", url);

    let response = ureq::get(&url)
        .set("Accept", "application/vnd.github.v3+json")
        .set("User-Agent", "zeff-boy-emulator")
        .call()
        .map_err(|e| format!("GitHub API request failed: {e}"))?;

    let body = response
        .into_string()
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
    let url = format!("{}{}/{}", RAW_BASE_URL, urlencoded(dir), urlencoded(filename));
    log::info!("Downloading cheat file from {}", url);

    let response = ureq::get(&url)
        .set("User-Agent", "zeff-boy-emulator")
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;

    let content = response
        .into_string()
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
    if names.is_empty() && json_body.len() > 100 {
        if json_body.contains("\"message\"") {
            if let Some(msg_start) = json_body.find(r#""message":"#) {
                let rest = &json_body[msg_start + 11..];
                if let Some(end) = rest.find('"') {
                    return Err(format!("GitHub API: {}", &rest[..end]));
                }
            }
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
        let files: Vec<String> = (0..100)
            .map(|i| format!("Game {i}.cht"))
            .collect();
        let results = search_filenames("game", &files, 10);
        assert_eq!(results.len(), 10);
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
        assert_eq!(urlencoded("Nintendo - Game Boy"), "Nintendo%20-%20Game%20Boy");
    }
}

