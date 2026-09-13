use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{RomSpan, catalog::SongRef};

mod gb;
mod nes;
mod sega;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeRipFormat {
    Gbs,
    Nsf,
    Sgc,
}

impl NativeRipFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Gbs => "gbs",
            Self::Nsf => "nsf",
            Self::Sgc => "sgc",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Gbs => "Game Boy GBS",
            Self::Nsf => "NES NSF",
            Self::Sgc => "Sega SGC",
        }
    }
}

pub struct NativeRip {
    pub bytes: Vec<u8>,
    pub metadata: NativeRipMetadata,
}

#[derive(Clone, Debug, Serialize)]
pub struct NativeRipMetadata {
    pub schema: &'static str,
    pub format: NativeRipFormat,
    pub source_sha256: String,
    pub source_byte_len: usize,
    pub output_sha256: String,
    pub output_byte_len: usize,
    pub detector: &'static str,
    pub profile: &'static str,
    pub raw_selector: u16,
    pub load_address: u16,
    pub init_address: u16,
    pub play_address: u16,
    pub original_init_address: u16,
    pub original_play_address: u16,
    pub play_rate_numerator: u32,
    pub play_rate_denominator: u32,
    pub frame_divider: u8,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

pub fn supported_format(song: SongRef<'_>) -> Option<NativeRipFormat> {
    match song {
        SongRef::GbNative(song) if gb::supported(song) => Some(NativeRipFormat::Gbs),
        SongRef::NesNative(song) if nes::supported(song) => Some(NativeRipFormat::Nsf),
        SongRef::SegaPsg(song) if sega::supported(song) => Some(NativeRipFormat::Sgc),
        _ => None,
    }
}

pub fn encode(bytes: &[u8], song: SongRef<'_>, cancel: &AtomicBool) -> Result<NativeRip> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "native music rip export cancelled"
    );
    let mut rip = match song {
        SongRef::GbNative(song) if gb::supported(song) => gb::encode(bytes, song, cancel)?,
        SongRef::NesNative(song) if nes::supported(song) => nes::encode(bytes, song, cancel)?,
        SongRef::SegaPsg(song) if sega::supported(song) => sega::encode(bytes, song, cancel)?,
        _ => anyhow::bail!("this selection has no qualified native music rip export"),
    };
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "native music rip export cancelled"
    );
    rip.metadata.output_sha256 = zeff_firmware::sha256_hex(&rip.bytes);
    rip.metadata.output_byte_len = rip.bytes.len();
    Ok(rip)
}

fn text_field(output: &mut [u8], value: &str) {
    for (target, value) in output.iter_mut().take(31).zip(value.chars()) {
        *target = if value.is_ascii_graphic() || value == ' ' {
            value as u8
        } else {
            b'?'
        };
    }
}

fn mapped_image(bytes: &[u8], spans: &[RomSpan], load: u16, end: u32) -> Result<Vec<u8>> {
    let mut output = vec![0; usize::try_from(end - u32::from(load))?];
    let mut occupied = vec![false; output.len()];
    for span in spans {
        ensure!(
            span.byte_len != 0 && span.canonical_cpu_address >= u32::from(load),
            "native rip source precedes its load address"
        );
        let start = usize::try_from(span.canonical_cpu_address - u32::from(load))?;
        let finish = start
            .checked_add(span.byte_len as usize)
            .ok_or_else(|| anyhow::anyhow!("native rip mapping overflows"))?;
        let source = span.effective_offset as usize;
        let source_end = source
            .checked_add(span.byte_len as usize)
            .ok_or_else(|| anyhow::anyhow!("native rip source overflows"))?;
        let data = bytes
            .get(source..source_end)
            .ok_or_else(|| anyhow::anyhow!("native rip source is outside its image"))?;
        ensure!(
            finish <= output.len(),
            "native rip mapping exceeds address space"
        );
        for index in start..finish {
            ensure!(
                !occupied[index] || output[index] == data[index - start],
                "native rip source mappings conflict"
            );
            occupied[index] = true;
        }
        output[start..finish].copy_from_slice(data);
    }
    Ok(output)
}

fn place_wrapper(
    output: &mut [u8],
    spans: &[RomSpan],
    load: u16,
    address: u16,
    code: &[u8],
) -> Result<()> {
    let end = u32::from(address) + u32::try_from(code.len())?;
    ensure!(
        spans.iter().all(|span| end <= span.canonical_cpu_address
            || u32::from(address) >= span.canonical_cpu_address + span.byte_len),
        "native rip wrapper overlaps original audio data"
    );
    let start = usize::from(
        address
            .checked_sub(load)
            .ok_or_else(|| anyhow::anyhow!("native rip wrapper precedes load address"))?,
    );
    output
        .get_mut(start..start + code.len())
        .ok_or_else(|| anyhow::anyhow!("native rip wrapper exceeds address space"))?
        .copy_from_slice(code);
    Ok(())
}
