use anyhow::Context;
use std::path::{Path, PathBuf};

use crate::libretro_common::LibretroPlatform;

#[cfg(not(target_arch = "wasm32"))]
const GITHUB_CHT_DIR_URL: &str =
    "https://api.github.com/repos/libretro/libretro-database/contents/cht";
#[cfg(not(target_arch = "wasm32"))]
const GITHUB_TREES_URL: &str = "https://api.github.com/repos/libretro/libretro-database/git/trees/";
#[cfg(not(target_arch = "wasm32"))]
const RAW_BASE_URL: &str =
    "https://raw.githubusercontent.com/libretro/libretro-database/master/cht/";
#[cfg(not(target_arch = "wasm32"))]
const CACHE_TTL_SECS: u64 = 86400;

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn fetch_cheat_list(
    platform: LibretroPlatform,
    cache_dir: &Path,
) -> anyhow::Result<Vec<String>> {
    let cache_file = cache_dir.join(format!("libretro_{}_index_v2.txt", platform.cache_suffix()));

    if let Ok(meta) = std::fs::metadata(&cache_file)
        && let Ok(modified) = meta.modified()
        && modified.elapsed().unwrap_or_default().as_secs() < CACHE_TTL_SECS
        && let Ok(content) = std::fs::read_to_string(&cache_file)
    {
        let names: Vec<String> = content
            .lines()
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect();
        if !names.is_empty() {
            return Ok(names);
        }
    }

    let names = fetch_cheat_list_via_trees(platform)?;

    if let Err(e) = std::fs::create_dir_all(cache_dir) {
        log::warn!(
            "failed to create cheat cache dir {}: {e}",
            cache_dir.display()
        );
    } else if let Err(e) = std::fs::write(&cache_file, names.join("\n")) {
        log::warn!(
            "failed to write cheat index cache {}: {e}",
            cache_file.display()
        );
    }

    Ok(names)
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_cheat_list_via_trees(platform: LibretroPlatform) -> anyhow::Result<Vec<String>> {
    log::info!(
        "Fetching libretro cheat index via Git Trees API for {}",
        platform.label()
    );

    let dir_json = crate::libretro_common::ureq_get_github_json(GITHUB_CHT_DIR_URL)?
        .read_to_string()
        .context("failed to read cht directory listing")?;
    let platform_sha = parse_dir_entry_sha(&dir_json, platform.platform_dir())?;

    let tree_url = format!("{GITHUB_TREES_URL}{platform_sha}");
    let tree_json = crate::libretro_common::ureq_get_github_json(&tree_url)?
        .read_to_string()
        .context("failed to read platform tree")?;
    let names = parse_tree_blob_names(&tree_json);

    if names.is_empty() {
        anyhow::bail!("no .cht files found for {}", platform.label());
    }

    log::info!("Found {} cheat files for {}", names.len(), platform.label());
    Ok(names)
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn download_cht_content(
    filename: &str,
    platform: LibretroPlatform,
    cache_dir: &Path,
) -> anyhow::Result<String> {
    let cht_cache_dir = cache_dir.join("libretro-cht");
    let safe_name = filename.replace(['/', '\\', ':'], "_");
    let cache_file = cht_cache_dir.join(&safe_name);

    // Try disk cache
    if let Ok(content) = std::fs::read_to_string(&cache_file) {
        return Ok(content);
    }

    let dir = platform.platform_dir();
    let url = format!(
        "{}{}/{}",
        RAW_BASE_URL,
        urlencoded(dir),
        urlencoded(filename)
    );
    log::info!("Downloading cheat file from {}", url);

    let content = crate::libretro_common::ureq_get(&url)?
        .read_to_string()
        .context("failed to read cheat file response")?;

    if let Err(e) = std::fs::create_dir_all(&cht_cache_dir) {
        log::warn!(
            "failed to create cht cache dir {}: {e}",
            cht_cache_dir.display()
        );
    } else if let Err(e) = std::fs::write(&cache_file, &content) {
        log::warn!("failed to write cht cache {}: {e}", cache_file.display());
    }

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
    crate::libretro_common::normalized_words(input)
}

fn title_core(input: &str) -> String {
    let stripped = crate::libretro_common::strip_suffix_groups(input);
    normalized_words(&stripped).join(" ")
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

pub(super) fn browse_url(platform: LibretroPlatform) -> String {
    let dir = platform.platform_dir();
    format!(
        "https://github.com/libretro/libretro-database/tree/master/cht/{}",
        urlencoded(dir)
    )
}

fn urlencoded(s: &str) -> String {
    s.replace(' ', "%20")
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_dir_entry_sha(json_body: &str, dir_name: &str) -> anyhow::Result<String> {
    let target = format!(r#""name":"{dir_name}""#);
    let Some(pos) = json_body.find(&target) else {
        anyhow::bail!("directory '{}' not found in cht listing", dir_name);
    };
    let after = &json_body[pos..];
    let sha_key = r#""sha":""#;
    let Some(sha_pos) = after.find(sha_key) else {
        anyhow::bail!("sha not found for directory '{}'", dir_name);
    };
    let sha_start = sha_pos + sha_key.len();
    let sha_rest = &after[sha_start..];
    let Some(sha_end) = sha_rest.find('"') else {
        anyhow::bail!("unterminated sha for directory '{}'", dir_name);
    };
    Ok(sha_rest[..sha_end].to_string())
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_tree_blob_names(json_body: &str) -> Vec<String> {
    let mut names = Vec::new();
    for segment in json_body.split(r#""path":""#).skip(1) {
        if let Some(end) = segment.find('"') {
            let path = &segment[..end];
            if path.ends_with(".cht") {
                names.push(path.to_string());
            }
        }
    }
    names
}

#[cfg(test)]
mod tests;
