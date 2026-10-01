use std::sync::atomic::AtomicBool;

use anyhow::Result;

use super::super::{
    NativeRip, NativeRipFormat, NativeRipMetadata, mapped_image, place_wrapper, text_field,
};
use crate::nes_native::{NesNativeSong, NesNativeTiming};

#[cfg(test)]
mod tests;

pub(super) fn supported(song: &NesNativeSong) -> bool {
    crate::nes_native::famistudio::owns(song.profile)
        && song
            .source_sha256
            .as_ref()
            .is_some_and(|hash| hash.len() == 64)
        && song.index < 2
        && u16::from(song.raw_index) == song.index
        && song.native.mapper == 0
        && song.native.timing == NesNativeTiming::Ntsc
        && song.native.driver.byte_len == 0x57b
        && song.native.init.canonical_cpu_address.checked_add(0x14f)
            == Some(song.native.tick.canonical_cpu_address)
}

pub(super) fn encode(bytes: &[u8], song: &NesNativeSong, cancel: &AtomicBool) -> Result<NativeRip> {
    crate::nes_native::prepare_rom(bytes, song, cancel)?;
    let init = u16::try_from(song.native.init.canonical_cpu_address)?;
    let play = u16::try_from(song.native.tick.canonical_cpu_address)?;
    let header = u16::try_from(song.header.canonical_cpu_address)?;
    let mut program = mapped_image(bytes, &song.mapped_spans, 0x8000, 0x10000)?;
    let mut code = vec![
        0x78, 0xd8, 0xa2, 0, 0xa9, 0, 0x9d, 0, 3, 0xe8, 0xe0, 0x7b, 0xd0, 0xf8,
    ];
    for address in 0..8 {
        code.extend_from_slice(&[0x85, address]);
    }
    code.extend_from_slice(&[
        0xa9,
        0x40,
        0x8d,
        0x17,
        0x40,
        0xa9,
        1,
        0xa2,
        header as u8,
        0xa0,
        (header >> 8) as u8,
        0x20,
    ]);
    code.extend_from_slice(&init.to_le_bytes());
    code.extend_from_slice(&[0xa9, song.raw_index, 0x20]);
    code.extend_from_slice(&(init + 0xb0).to_le_bytes());
    code.push(0x60);
    place_wrapper(&mut program, &song.mapped_spans, 0x8000, 0xf800, &code)?;
    let mut output = vec![0; 0x80];
    output[..8].copy_from_slice(b"NESM\x1a\x01\x01\x01");
    output[8..10].copy_from_slice(&0x8000_u16.to_le_bytes());
    output[10..12].copy_from_slice(&0xf800_u16.to_le_bytes());
    output[12..14].copy_from_slice(&play.to_le_bytes());
    text_field(&mut output[0x0e..0x2e], &song.title);
    output[0x6e..0x70].copy_from_slice(&16639_u16.to_le_bytes());
    output[0x78..0x7a].copy_from_slice(&19997_u16.to_le_bytes());
    output.extend(program);
    Ok(NativeRip {
        bytes: output,
        metadata: NativeRipMetadata {
            schema: "zeff-native-music-rip/1",
            format: NativeRipFormat::Nsf,
            source_sha256: zeff_firmware::sha256_hex(bytes),
            source_byte_len: bytes.len(),
            output_sha256: String::new(),
            output_byte_len: 0,
            detector: "nes-native-driver",
            profile: song.profile,
            raw_selector: u16::from(song.raw_index),
            load_address: 0x8000,
            init_address: 0xf800,
            play_address: play,
            original_init_address: init,
            original_play_address: play,
            play_rate_numerator: 1_000_000,
            play_rate_denominator: 16639,
            frame_divider: 1,
            mapped_spans: song.mapped_spans.clone(),
            warnings: vec![
                "One selected cue; returning INIT clears owned driver state and preserves the player stack.".into(),
                "NTSC NSF uses a 16,639 microsecond period. Original cartridge startup timing, natural end and loop tags are not preserved.".into(),
            ],
        },
    })
}
