use zeff_gb_core::hardware::ppu::DmgPalettePreset;
use zeff_pce_core::hardware::{PceArcadeCardMode, PceControllerMode, PceMemoryBaseMode};
use zeff_sega8_core::hardware::region::Sega8Region;
use zeff_sega8_core::hardware::timing::Sega8VideoStandard;

use super::super::types::{HeadlessMemoryDump, HeadlessRegionDump};
use super::numbers::{parse_addr_arg, parse_u64_arg, parse_usize_arg};

pub(super) fn parse_memory_dump_arg(value: &str, flag: &str) -> anyhow::Result<HeadlessMemoryDump> {
    let Some((addr_raw, len_raw)) = value.split_once(':') else {
        anyhow::bail!("{flag} must be addr:len, e.g. C000:60 or 0xC000:0x60");
    };
    let addr = parse_addr_arg(addr_raw, flag)?;
    let len = parse_u64_arg(len_raw, flag)?;
    if addr > u64::from(u16::MAX) {
        anyhow::bail!("{flag} address must fit in the GB 16-bit address space");
    }
    if len == 0 {
        anyhow::bail!("{flag} length must be greater than zero");
    }
    if len > 4096 {
        anyhow::bail!("{flag} length is capped at 4096 bytes");
    }
    Ok(HeadlessMemoryDump {
        start_addr: addr as u16,
        len: len as u16,
    })
}

pub(super) fn parse_region_dump_arg(value: &str, flag: &str) -> anyhow::Result<HeadlessRegionDump> {
    let mut fields = value.split(':');
    let (Some(region), Some(offset_raw), Some(len_raw), None) =
        (fields.next(), fields.next(), fields.next(), fields.next())
    else {
        anyhow::bail!("{flag} must be region:offset:len, e.g. video_ram:0x7000:0x1000");
    };
    if region.trim().is_empty() {
        anyhow::bail!("{flag} region must not be empty");
    }
    let offset = usize::try_from(parse_u64_arg(offset_raw, flag)?)
        .map_err(|_| anyhow::anyhow!("{flag} offset does not fit this platform"))?;
    let len = usize::try_from(parse_u64_arg(len_raw, flag)?)
        .map_err(|_| anyhow::anyhow!("{flag} length does not fit this platform"))?;
    if len == 0 {
        anyhow::bail!("{flag} length must be greater than zero");
    }
    if len > 4096 {
        anyhow::bail!("{flag} length is capped at 4096 bytes");
    }
    Ok(HeadlessRegionDump {
        region: region.trim().to_owned(),
        offset,
        len,
    })
}

pub(super) fn parse_gba_bg_layer_list_arg(value: &str, flag: &str) -> anyhow::Result<[bool; 4]> {
    let mut hidden = [false; 4];
    let mut parsed_any = false;
    for raw in value.split(',') {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        parsed_any = true;
        let raw = raw
            .strip_prefix("bg")
            .or_else(|| raw.strip_prefix("BG"))
            .unwrap_or(raw);
        let index = parse_usize_arg(raw, flag)?;
        if index > 3 {
            anyhow::bail!("{flag} accepts BG layers 0, 1, 2, or 3");
        }
        hidden[index] = true;
    }
    if !parsed_any {
        anyhow::bail!("{flag} requires at least one BG layer");
    }
    Ok(hidden)
}

pub(super) fn parse_dmg_palette_arg(value: &str, flag: &str) -> anyhow::Result<DmgPalettePreset> {
    let token = value.trim().to_ascii_lowercase().replace(['_', '-'], "");
    match token.as_str() {
        "gray" | "grey" | "grayscale" | "greyscale" => Ok(DmgPalettePreset::Gray),
        "dmggreen" | "green" => Ok(DmgPalettePreset::DmgGreen),
        "pocket" | "gameboypocket" => Ok(DmgPalettePreset::Pocket),
        "mint" => Ok(DmgPalettePreset::Mint),
        "chocolate" => Ok(DmgPalettePreset::Chocolate),
        _ => anyhow::bail!(
            "{flag} has unknown palette {value:?}; expected gray, dmg-green, pocket, mint, or chocolate"
        ),
    }
}

pub(super) fn parse_sega8_video_standard_arg(
    value: &str,
    flag: &str,
) -> anyhow::Result<Sega8VideoStandard> {
    Sega8VideoStandard::parse(value)
        .ok_or_else(|| anyhow::anyhow!("{flag} requires one of: auto|ntsc|pal|60hz|50hz"))
}

pub(super) fn parse_sega8_console_region_arg(
    value: &str,
    flag: &str,
) -> anyhow::Result<Sega8Region> {
    Sega8Region::parse(value).ok_or_else(|| {
        anyhow::anyhow!(
            "{flag} requires one of: auto|export|international|japanese|japan|pbc|power-base"
        )
    })
}

pub(super) fn parse_pce_controller_mode_arg(
    value: &str,
    flag: &str,
) -> anyhow::Result<PceControllerMode> {
    let token = value.trim().to_ascii_lowercase().replace(['_', '-'], "");
    match token.as_str() {
        "auto" | "automatic" => Ok(PceControllerMode::Automatic),
        "pad" | "twobutton" | "2button" => Ok(PceControllerMode::TwoButton),
        "sixbutton" | "6button" | "6btn" => Ok(PceControllerMode::SixButton),
        "mouse" => Ok(PceControllerMode::Mouse),
        "multitap" | "tap" => Ok(PceControllerMode::Multitap),
        _ => anyhow::bail!(
            "{flag} has unknown mode {value:?}; expected auto, pad, six-button, mouse, or multitap"
        ),
    }
}

pub(super) fn parse_pce_memory_base_mode_arg(
    value: &str,
    flag: &str,
) -> anyhow::Result<PceMemoryBaseMode> {
    let token = value.trim().to_ascii_lowercase().replace(['_', '-'], "");
    match token.as_str() {
        "auto" | "automatic" => Ok(PceMemoryBaseMode::Automatic),
        "on" | "enabled" | "enable" => Ok(PceMemoryBaseMode::Enabled),
        "off" | "disabled" | "disable" => Ok(PceMemoryBaseMode::Disabled),
        _ => {
            anyhow::bail!("{flag} has unknown mode {value:?}; expected auto, enabled, or disabled")
        }
    }
}

pub(super) fn parse_pce_arcade_card_mode_arg(
    value: &str,
    flag: &str,
) -> anyhow::Result<PceArcadeCardMode> {
    let token = value.trim().to_ascii_lowercase().replace(['_', '-'], "");
    match token.as_str() {
        "auto" | "automatic" => Ok(PceArcadeCardMode::Automatic),
        "on" | "enabled" | "enable" => Ok(PceArcadeCardMode::Enabled),
        "off" | "disabled" | "disable" => Ok(PceArcadeCardMode::Disabled),
        _ => {
            anyhow::bail!("{flag} has unknown mode {value:?}; expected auto, enabled, or disabled")
        }
    }
}

pub(super) fn parse_gba_audio_mute_list_arg(value: &str, flag: &str) -> anyhow::Result<[bool; 6]> {
    let mut mutes = [false; 6];
    let mut parsed_any = false;
    for raw in value.split(',') {
        let token = raw.trim().to_ascii_lowercase().replace(['_', '-'], "");
        if token.is_empty() {
            continue;
        }
        parsed_any = true;
        let index = match token.as_str() {
            "0" | "1" | "psg0" | "psg1" | "square1" | "ch1" => 0,
            "2" | "psg2" | "square2" | "ch2" => 1,
            "3" | "psg3" | "wave" | "ch3" => 2,
            "4" | "psg4" | "noise" | "ch4" => 3,
            "5" | "fifoa" | "directa" | "a" => 4,
            "6" | "fifob" | "directb" | "b" => 5,
            "psg" => {
                mutes[..4].fill(true);
                continue;
            }
            "fifo" | "direct" | "pcm" => {
                mutes[4] = true;
                mutes[5] = true;
                continue;
            }
            other => anyhow::bail!(
                "{flag} has unknown channel {other:?}; expected psg1..psg4, fifoA, fifoB, psg, fifo"
            ),
        };
        mutes[index] = true;
    }
    if !parsed_any {
        anyhow::bail!("{flag} requires at least one audio channel");
    }
    Ok(mutes)
}
