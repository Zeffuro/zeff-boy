use super::parse_cheat;
use super::types::CheatCode;

pub fn parse_cht_file(content: &str) -> Vec<CheatCode> {
    let mut cheats = Vec::new();
    let mut entries: std::collections::HashMap<usize, (Option<String>, Option<String>, bool)> =
        std::collections::HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(rest) = line.strip_prefix("cheat")
            && let Some(idx_end) = rest.find('_')
            && let Ok(idx) = rest[..idx_end].parse::<usize>()
        {
            let field = &rest[idx_end + 1..];
            if let Some(value) = field.strip_prefix("desc = ") {
                let value = value.trim().trim_matches('"').to_string();
                entries.entry(idx).or_insert((None, None, false)).0 = Some(value);
            } else if let Some(value) = field.strip_prefix("code = ") {
                let value = value.trim().trim_matches('"').to_string();
                entries.entry(idx).or_insert((None, None, false)).1 = Some(value);
            } else if let Some(value) = field.strip_prefix("enable = ") {
                let enabled = value.trim() == "true";
                entries.entry(idx).or_insert((None, None, false)).2 = enabled;
            }
        }
    }

    let mut indices: Vec<usize> = entries.keys().copied().collect();
    indices.sort_unstable();

    for idx in indices {
        if let Some((desc, code, enabled)) = entries.remove(&idx) {
            let code_text = code.unwrap_or_default();
            if code_text.is_empty() {
                continue;
            }
            let name = desc.unwrap_or_else(|| code_text.clone());

            match parse_cheat(&code_text) {
                Ok((patches, code_type)) => {
                    let parameter_value =
                        patches.iter().copied().find_map(|p| p.default_user_value());
                    cheats.push(CheatCode {
                        name,
                        code_text,
                        enabled,
                        parameter_value,
                        code_type,
                        patches,
                    });
                }
                Err(e) => {
                    log::warn!(
                        "Failed to parse cheat '{}': {} (code: {})",
                        name,
                        e,
                        code_text
                    );
                }
            }
        }
    }

    cheats
}

pub fn export_cht_file(cheats: &[CheatCode]) -> String {
    let mut out = String::new();
    out.push_str(&format!("cheats = {}\n\n", cheats.len()));

    for (i, cheat) in cheats.iter().enumerate() {
        out.push_str(&format!("cheat{}_desc = \"{}\"\n", i, cheat.name));
        out.push_str(&format!("cheat{}_code = \"{}\"\n", i, cheat.code_text));
        out.push_str(&format!("cheat{}_enable = {}\n\n", i, cheat.enabled));
    }

    out
}
