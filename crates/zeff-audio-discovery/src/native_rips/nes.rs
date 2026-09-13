use std::sync::atomic::AtomicBool;

use anyhow::Result;

use super::{
    NativeRip, NativeRipFormat, NativeRipMetadata, mapped_image, place_wrapper, text_field,
};
use crate::nes_native::{NesNativeSong, NesNativeTiming};

pub(super) fn supported(song: &NesNativeSong) -> bool {
    let profile = song.profile == "nes-native-konami-cnrom-01";
    #[cfg(any(test, feature = "test-support"))]
    let profile = profile || song.profile == "nes-native-synthetic";
    profile
        && song.native.mapper == 3
        && song.native.timing == NesNativeTiming::Ntsc
        && song.native.init.canonical_cpu_address == 0xec4c
        && song.native.tick.canonical_cpu_address == 0xed30
}

pub(super) fn encode(bytes: &[u8], song: &NesNativeSong, cancel: &AtomicBool) -> Result<NativeRip> {
    crate::nes_native::prepare_rom(bytes, song, cancel)?;
    let init = u16::try_from(song.native.init.canonical_cpu_address)?;
    let play = u16::try_from(song.native.tick.canonical_cpu_address)?;
    let mut program = mapped_image(bytes, &song.mapped_spans, 0x8000, 0x10000)?;
    let mut code = vec![
        0x78,
        0xd8,
        0xa9,
        0x1f,
        0x8d,
        0x15,
        0x40,
        0xa9,
        0xc0,
        0x8d,
        0x17,
        0x40,
        0xa9,
        song.raw_index,
        0x20,
    ];
    code.extend(init.to_le_bytes());
    code.push(0x60);
    place_wrapper(&mut program, &song.mapped_spans, 0x8000, 0x8000, &code)?;
    let mut output = vec![0; 0x80];
    output[..8].copy_from_slice(b"NESM\x1a\x01\x01\x01");
    output[8..10].copy_from_slice(&0x8000_u16.to_le_bytes());
    output[10..12].copy_from_slice(&0x8000_u16.to_le_bytes());
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
            init_address: 0x8000,
            play_address: play,
            original_init_address: init,
            original_play_address: play,
            play_rate_numerator: 1_000_000,
            play_rate_denominator: 16639,
            frame_divider: 1,
            mapped_spans: song.mapped_spans.clone(),
            warnings: vec![
                "One selected original audio cue; the player initializes RAM and calls returning INIT/PLAY routines.".into(),
                "NTSC NSF timing uses the format's 16,639 microsecond period; it rounds the original frame interval. Natural end and loop tags are not encoded.".into(),
            ],
        },
    })
}
