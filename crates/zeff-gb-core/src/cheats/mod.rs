mod cht_file;
mod parsers;
mod types;

#[cfg(test)]
mod tests;

pub use cht_file::{export_cht_file, parse_cht_file};
pub use types::{CheatCode, CheatPatch, CheatType, CheatValue};

use parsers::{
    try_parse_game_genie, try_parse_gameshark, try_parse_gameshark_single, try_parse_raw,
    try_parse_xploder,
};

pub fn parse_cheat(input: &str) -> Result<(Vec<CheatPatch>, CheatType), &'static str> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Empty cheat code");
    }

    let parts: Vec<&str> = trimmed.split('+').collect();

    if parts.len() == 1 {
        if let Some(result) = try_parse_game_genie(trimmed) {
            return Ok(result);
        }
        if let Some(result) = try_parse_raw(trimmed) {
            return Ok(result);
        }
        if let Some(result) = try_parse_xploder(trimmed) {
            return Ok(result);
        }
        if let Some(result) = try_parse_gameshark(trimmed) {
            return Ok(result);
        }
    } else {
        let mut all_patches = Vec::new();
        let mut detected_type: Option<CheatType> = None;

        for part in &parts {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let result = try_parse_game_genie(part)
                .or_else(|| try_parse_raw(part))
                .or_else(|| try_parse_xploder(part))
                .or_else(|| try_parse_gameshark_single(part));

            match result {
                Some((patches, ty)) => {
                    if let Some(prev) = detected_type
                        && prev != ty
                    {}
                    detected_type = Some(ty);
                    all_patches.extend(patches);
                }
                None => {
                    return Err(
                        "Unrecognized format in multi-code. Use GameShark (01VVAAAA), Game Genie (XXX-YYY), XPloder ($XXXXXXXX), or raw (AAAA:VV)",
                    );
                }
            }
        }

        if let Some(ty) = detected_type
            && !all_patches.is_empty()
        {
            return Ok((all_patches, ty));
        }
    }

    Err(
        "Unrecognized format. Use GameShark (01VVAAAA, supports ??/?0/0? values), Game Genie (XXX-YYY), XPloder ($XXXXXXXX), or raw (AAAA:VV)",
    )
}

pub fn collect_enabled_patches(user: &[CheatCode], libretro: &[CheatCode]) -> Vec<CheatPatch> {
    user.iter()
        .chain(libretro.iter())
        .filter(|c| c.enabled)
        .flat_map(|c| {
            c.patches.iter().copied().map(|patch| {
                if patch.has_user_parameter() {
                    let value = c
                        .parameter_value
                        .or_else(|| patch.default_user_value())
                        .unwrap_or(0);
                    patch.resolve_user_parameter(value)
                } else {
                    patch
                }
            })
        })
        .collect()
}
