use std::collections::HashMap;

use crate::emu_backend::ActiveSystem;

use super::CheatCode;
use super::parse::{parse_cheat_for_system, parse_cheat_for_system_with_gba_state};

pub(crate) fn parse_cht_file_for_system(content: &str, system: ActiveSystem) -> Vec<CheatCode> {
    let mut entries: HashMap<usize, (Option<String>, Option<String>, bool)> = HashMap::new();
    let mut gba_codebreaker_state = zeff_emu_common::cheats::GbaCodeBreakerState::default();

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

    let mut cheats = Vec::new();
    for idx in indices {
        if let Some((desc, code, enabled)) = entries.remove(&idx) {
            let code_text = code.unwrap_or_default();
            if code_text.is_empty() {
                continue;
            }
            let name = desc.unwrap_or_else(|| code_text.clone());

            let parsed = if system == ActiveSystem::GameBoyAdvance {
                parse_cheat_for_system_with_gba_state(
                    &code_text,
                    system,
                    &mut gba_codebreaker_state,
                )
            } else {
                parse_cheat_for_system(&code_text, system)
            };

            match parsed {
                Ok((patches, code_type)) => {
                    if patches.is_empty() {
                        continue;
                    }
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

pub(crate) fn export_cht_file(cheats: &[CheatCode]) -> String {
    let mut out = String::new();
    out.push_str(&format!("cheats = {}\n\n", cheats.len()));

    for (i, cheat) in cheats.iter().enumerate() {
        out.push_str(&format!("cheat{}_desc = \"{}\"\n", i, cheat.name));
        out.push_str(&format!("cheat{}_code = \"{}\"\n", i, cheat.code_text));
        out.push_str(&format!("cheat{}_enable = {}\n\n", i, cheat.enabled));
    }

    out
}
