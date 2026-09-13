use std::sync::atomic::AtomicBool;

use anyhow::{Result, ensure};

use super::{NativeRip, NativeRipFormat, NativeRipMetadata, place_wrapper, text_field};
use crate::gb_native::{GbNativeSong, GbNativeTiming};

const LOAD: u16 = 0x0400;
const INIT: u16 = 0x0800;
const PLAY: u16 = 0x0880;

pub(super) fn supported(song: &GbNativeSong) -> bool {
    let profile = song.profile == "gb-native-banked-mbc3-bank2-01";
    #[cfg(any(test, feature = "test-support"))]
    let profile = profile
        || matches!(
            song.profile,
            "gb-native-synthetic" | "gb-native-synthetic-expanded"
        );
    profile
        && song.bank == 2
        && song.native.cartridge_type == 0x13
        && song.native.timing == GbNativeTiming::Dmg
        && song.native.init.canonical_cpu_address == 0x200e
        && song.native.tick.canonical_cpu_address == 0x5103
}

pub(super) fn encode(bytes: &[u8], song: &GbNativeSong, cancel: &AtomicBool) -> Result<NativeRip> {
    crate::gb_native::prepare_rom(bytes, song, cancel)?;
    let mut program = vec![0; 0xc000 - usize::from(LOAD)];
    for span in &song.mapped_spans {
        let start = span.effective_offset as usize;
        let end = start
            .checked_add(span.byte_len as usize)
            .ok_or_else(|| anyhow::anyhow!("GBS source range overflows"))?;
        ensure!(
            start >= usize::from(LOAD) && end <= 0xc000,
            "GBS source exceeds its qualified home/audio banks"
        );
        ensure!(
            span.canonical_cpu_address
                == if start < 0x4000 {
                    start as u32
                } else {
                    0x4000 + start as u32 % 0x4000
                },
            "GBS source bank mapping changed"
        );
        program[start - usize::from(LOAD)..end - usize::from(LOAD)].copy_from_slice(
            bytes
                .get(start..end)
                .ok_or_else(|| anyhow::anyhow!("GBS source lies outside its ROM"))?,
        );
    }
    // The returning initializer replaces cartridge startup; the caller's stack is in HRAM.
    let code = [
        0x21,
        0,
        0xc0,
        0x01,
        0,
        0x20,
        0xaf,
        0x22,
        0x0b,
        0x78,
        0xb1,
        0x20,
        0xf9,
        0x3e,
        1,
        0xe0,
        0xb8,
        0xea,
        0,
        0x20,
        0xcd,
        0x0e,
        0x20,
        0x0e,
        2,
        0x3e,
        song.raw_index,
        0xcd,
        0xa1,
        0x23,
        0x3e,
        2,
        0xe0,
        0xb8,
        0xea,
        0,
        0x20,
        0xc9,
    ];
    place_wrapper(&mut program, &song.mapped_spans, LOAD, INIT, &code)?;
    place_wrapper(
        &mut program,
        &song.mapped_spans,
        LOAD,
        PLAY,
        &[0xcd, 0xcb, 0x28, 0xcd, 3, 0x51, 0xc9],
    )?;
    let mut output = vec![0; 0x70];
    output[..6].copy_from_slice(b"GBS\x01\x01\x01");
    output[6..8].copy_from_slice(&LOAD.to_le_bytes());
    output[8..10].copy_from_slice(&INIT.to_le_bytes());
    output[10..12].copy_from_slice(&PLAY.to_le_bytes());
    output[12..14].copy_from_slice(&0xfff0_u16.to_le_bytes());
    text_field(&mut output[0x10..0x30], &song.title);
    output.extend(program);
    Ok(NativeRip {
        bytes: output,
        metadata: NativeRipMetadata {
            schema: "zeff-native-music-rip/1",
            format: NativeRipFormat::Gbs,
            source_sha256: zeff_firmware::sha256_hex(bytes),
            source_byte_len: bytes.len(),
            output_sha256: String::new(),
            output_byte_len: 0,
            detector: "gb-native-driver",
            profile: song.profile,
            raw_selector: u16::from(song.raw_index),
            load_address: LOAD,
            init_address: INIT,
            play_address: PLAY,
            original_init_address: 0x200e,
            original_play_address: 0x5103,
            play_rate_numerator: 4_194_304,
            play_rate_denominator: 70_224,
            frame_divider: 1,
            mapped_spans: song.mapped_spans.clone(),
            warnings: vec![
                "One selected original DMG song; returning INIT reconstructs the driver's startup state and PLAY runs the original volume and sequencer routines.".into(),
                "Uses normal-speed VBlank timing and original bank numbers. Natural end and loop metadata are not encoded in the GBS header.".into(),
            ],
        },
    })
}
