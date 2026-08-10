use zeff_gb_core::hardware::ppu::DmgPalettePreset;

use super::super::types::HeadlessMemoryDump;
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
