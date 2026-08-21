use crate::emu_backend::ActiveSystem;

use super::{CheatPatch, CheatType, CheatValue};

pub(crate) fn try_parse_nes_game_genie(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    let patch = zeff_nes_core::cheats::decode_nes_game_genie(input)?;
    let cheat_patch = match patch.compare {
        Some(cmp) => CheatPatch::RomWriteIfEquals {
            address: patch.address,
            value: CheatValue::constant(patch.value),
            compare: CheatValue::constant(cmp),
        },
        None => CheatPatch::RomWrite {
            address: patch.address,
            value: CheatValue::constant(patch.value),
        },
    };
    Some((vec![cheat_patch], CheatType::GameGenie))
}

fn try_parse_wide_raw(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    zeff_emu_common::cheats::parse_wide_raw_cheats(input).map(|patches| (patches, CheatType::Raw))
}

fn try_parse_gba_codebreaker(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    zeff_emu_common::cheats::parse_gba_codebreaker_cheats(input)
        .map(|patches| (patches, CheatType::XPloder))
}

fn try_parse_gba_codebreaker_with_state(
    input: &str,
    state: &mut zeff_emu_common::cheats::GbaCodeBreakerState,
) -> Option<(Vec<CheatPatch>, CheatType)> {
    zeff_emu_common::cheats::parse_gba_codebreaker_cheats_with_state(input, state)
        .map(|patches| (patches, CheatType::XPloder))
}

fn try_parse_gba(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    try_parse_wide_raw(input).or_else(|| try_parse_gba_codebreaker(input))
}

fn try_parse_gba_with_state(
    input: &str,
    state: &mut zeff_emu_common::cheats::GbaCodeBreakerState,
) -> Option<(Vec<CheatPatch>, CheatType)> {
    try_parse_wide_raw(input).or_else(|| try_parse_gba_codebreaker_with_state(input, state))
}

fn try_parse_single_for_system(
    input: &str,
    system: ActiveSystem,
) -> Option<(Vec<CheatPatch>, CheatType)> {
    match system {
        ActiveSystem::GameBoy => zeff_gb_core::cheats::parse_cheat(input).ok(),
        ActiveSystem::Nes => zeff_gb_core::cheats::parse_cheat(input)
            .ok()
            .or_else(|| try_parse_nes_game_genie(input)),
        ActiveSystem::Pce => None,
        ActiveSystem::MasterSystem | ActiveSystem::GameGear | ActiveSystem::Sg1000 => {
            zeff_sega8_core::cheats::parse_cheat(input).ok()
        }
        ActiveSystem::GameBoyAdvance => try_parse_gba(input),
        ActiveSystem::WonderSwan => try_parse_wide_raw(input),
    }
}

fn try_parse_single_for_system_with_gba_state(
    input: &str,
    system: ActiveSystem,
    gba_codebreaker_state: &mut zeff_emu_common::cheats::GbaCodeBreakerState,
) -> Option<(Vec<CheatPatch>, CheatType)> {
    match system {
        ActiveSystem::GameBoyAdvance => try_parse_gba_with_state(input, gba_codebreaker_state),
        _ => try_parse_single_for_system(input, system),
    }
}

pub(crate) fn parse_cheat_for_system(
    input: &str,
    system: ActiveSystem,
) -> Result<(Vec<CheatPatch>, CheatType), &'static str> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Empty cheat code");
    }

    if let Some(result) = try_parse_single_for_system(trimmed, system) {
        return Ok(result);
    }

    let parts: Vec<&str> = trimmed.split('+').collect();
    if parts.len() > 1 {
        let mut all_patches = Vec::new();
        let mut detected_type: Option<CheatType> = None;

        for part in &parts {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Some((patches, ty)) = try_parse_single_for_system(part, system) {
                detected_type = Some(ty);
                all_patches.extend(patches);
            } else {
                return Err(
                    "Unrecognized format in multi-code. For GB: GameShark, Game Genie, raw. For NES: Game Genie (AAAAAA/AAAAAAAA), raw (AAAA:VV). For Sega 8-bit: raw (AAAA:VV), Action Replay (00AA-AAVV), Game Genie (XXX-XXX-XXX). For GBA: raw (AAAAAAAA:VV), CodeBreaker/XPloder RAM writes. For WS: raw (AAAAAAAA:VV)",
                );
            }
        }

        if let Some(ty) = detected_type
            && !all_patches.is_empty()
        {
            return Ok((all_patches, ty));
        }
    }

    Err(
        "Unrecognized format. For GB: GameShark (01VVAAAA), Game Genie (XXX-YYY), raw (AAAA:VV). For NES: Game Genie (AAAAAA or AAAAAAAA), raw (AAAA:VV). For Sega 8-bit: raw (AAAA:VV), Action Replay (00AA-AAVV), Game Genie (XXX-XXX-XXX). For GBA: raw (AAAAAAAA:VV), CodeBreaker/XPloder RAM writes. For WS: raw (AAAAAAAA:VV)",
    )
}

pub(crate) fn parse_cheat_for_system_with_gba_state(
    input: &str,
    system: ActiveSystem,
    gba_codebreaker_state: &mut zeff_emu_common::cheats::GbaCodeBreakerState,
) -> Result<(Vec<CheatPatch>, CheatType), &'static str> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Empty cheat code");
    }

    if let Some(result) =
        try_parse_single_for_system_with_gba_state(trimmed, system, gba_codebreaker_state)
    {
        return Ok(result);
    }

    parse_cheat_for_system(trimmed, system)
}
