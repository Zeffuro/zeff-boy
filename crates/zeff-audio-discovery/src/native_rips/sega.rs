use std::sync::atomic::AtomicBool;

use anyhow::{Result, ensure};
use zeff_emu_common::system::System;

use super::{
    NativeRip, NativeRipFormat, NativeRipMetadata, mapped_image, place_wrapper, text_field,
};
use crate::sega_psg::SegaPsgSong;

const LOAD: u16 = 0x0400;
const INIT: u16 = 0x0800;
const PLAY: u16 = 0x0840;

pub(super) fn supported(song: &SegaPsgSong) -> bool {
    qualified_selector(song.profile, song.raw_index)
        && matches!(song.frame_divider, 1 | 2)
        && song
            .mapped_spans
            .iter()
            .all(|span| span.canonical_cpu_address >= u32::from(LOAD))
}

fn qualified_selector(profile: &str, selector: u8) -> bool {
    #[cfg(any(test, feature = "test-support"))]
    if matches!(
        profile,
        "synthetic-sega-psg-native" | "synthetic-sega-psg-native-six-byte"
    ) {
        return selector == 0x81;
    }
    // Keep later low-ROM and refresh-register dependencies outside the qualified selectors.
    matches!((profile, selector),
        ("sega-psg-bank-v1-00", 0x81..=0x9f)
            | ("sega-psg-bank-v1-01", 0x81..=0x9f)
            | ("sega-psg-bank-v1-02", 0x81..=0x93)
            | ("sega-psg-bank-v1-03", 0x81..=0x93)
            | ("sega-psg-bank-v1-04", 0x81..=0x90)
            | ("sega-psg-bank-v1-05", 0x81..=0x90)
            | ("sega-psg-bank-v1-06", 0x82..=0x8f)
            | ("sega-psg-bank-v1-07", 0x81..=0x97)
            | ("sega-psg-bank-v1-08", 0x81..=0x85 | 0x87..=0x8b)
            | ("sega-psg-bank-v1-09", 0x81..=0x94)
            | ("sega-psg-bank-v1-10", 0x81..=0x91)
            | ("sega-psg-bank-v1-11", 0x81..=0x97)
            | ("sega-psg-bank-v1-12", 0x81..=0x8a)
            | ("sega-psg-bank-v1-13", 0x81..=0x8f)
            | ("sega-psg-bank-v1-14", 0x81..=0x8c)
            | ("sega-psg-bank-v1-15", 0x81..=0x82 | 0x84..=0x92)
            | ("sega-psg-bank-v1-16", 0x81..=0x8f)
            | ("sega-psg-bank-v1-17", 0x81..=0x8d | 0x8f)
            | ("sega-psg-bank-v1-18", 0x81..=0x8b)
            | ("sega-psg-bank-v1-19", 0x81..=0x8f)
            | ("sega-psg-bank-v1-20", 0x81..=0x8e)
            | ("sega-psg-bank-v1-21", 0x81..=0x90)
            | ("sega-psg-bank-v1-22", 0x81..=0x94)
            | ("sega-psg-bank-v1-23", 0x81..=0x90)
            | ("sega-psg-bank-v1-24", 0x81..=0x9a)
            | ("sega-psg-bank-v1-25", 0x81..=0x87 | 0x89..=0x98)
            | ("sega-psg-bank-v1-26", 0x81..=0x87 | 0x89 | 0x8b..=0x9a)
            | ("sega-psg-bank-v1-27", 0x81..=0x93)
            | ("sega-psg-bank-v2-00", 0x81..=0x94 | 0x97..=0x99)
            | ("sega-psg-bank-v2-01", 0x81..=0x90)
            | ("sega-psg-bank-v2-02", 0x81..=0x8f)
            | ("sega-psg-bank-v2-03", 0x81..=0x90)
            | ("sega-psg-bank-v2-04", 0x81..=0x8d | 0x8f..=0x90)
            | ("sega-psg-bank-v2-05", 0x81..=0x8c)
            | ("sega-psg-bank-v2-06", 0x81..=0x96)
            | ("sega-psg-bank-v2-07", 0x81..=0x8b | 0x8d..=0x91)
    )
}

pub(super) fn encode(bytes: &[u8], song: &SegaPsgSong, cancel: &AtomicBool) -> Result<NativeRip> {
    let prepared = crate::sega_psg::prepare_rom(bytes, song, cancel)?;
    // Read the validated bootstrap's original call target without duplicating its profile table.
    ensure!(
        prepared.bytes.get(0x120..0x123) == Some(&[0xed, 0xb0, 0xcd]),
        "Sega native initialization contract changed"
    );
    let init = u16::from_le_bytes([prepared.bytes[0x123], prepared.bytes[0x124]]);
    let play = u16::try_from(song.driver.canonical_cpu_address)?;
    let mut program = mapped_image(bytes, &song.mapped_spans, LOAD, 0xc000)?;
    let mut code = vec![
        0xf3, 0xaf, 0x32, 2, 0xc0, 0x21, 0xff, 0xdf, 0x11, 0, 0xe0, 1, 0, 0, 0xcd,
    ];
    code.extend(init.to_le_bytes());
    if song.system == System::Gg {
        code.extend([0x3e, 0xff, 0xd3, 6]);
    }
    code.extend([0x3e, song.raw_index, 0x32, 4, 0xde, 0xc9]);
    place_wrapper(&mut program, &song.mapped_spans, LOAD, INIT, &code)?;
    let mut tick = Vec::new();
    if song.frame_divider == 2 {
        tick.extend([0x3a, 2, 0xc0, 0xee, 1, 0x32, 2, 0xc0, 0xc0]);
    }
    tick.push(0xc3);
    tick.extend(play.to_le_bytes());
    place_wrapper(&mut program, &song.mapped_spans, LOAD, PLAY, &tick)?;
    let mut output = vec![0; 0xa0];
    output[..5].copy_from_slice(b"SGC\x1a\x01");
    output[8..10].copy_from_slice(&LOAD.to_le_bytes());
    output[10..12].copy_from_slice(&INIT.to_le_bytes());
    output[12..14].copy_from_slice(&PLAY.to_le_bytes());
    output[14..16].copy_from_slice(&0xddf0_u16.to_le_bytes());
    output[0x20..0x24].copy_from_slice(&[0, 0, 1, 2]);
    output[0x25] = 1;
    output[0x28] = u8::from(song.system == System::Gg);
    text_field(&mut output[0x40..0x60], &song.title);
    text_field(&mut output[0x60..0x80], "<?>");
    text_field(&mut output[0x80..0xa0], "<?>");
    output.extend(program);
    Ok(NativeRip {
        bytes: output,
        metadata: NativeRipMetadata {
            schema: "zeff-native-music-rip/1",
            format: NativeRipFormat::Sgc,
            source_sha256: zeff_firmware::sha256_hex(bytes),
            source_byte_len: bytes.len(),
            output_sha256: String::new(),
            output_byte_len: 0,
            detector: "sega-psg-driver",
            profile: song.profile,
            raw_selector: u16::from(song.raw_index),
            load_address: LOAD,
            init_address: INIT,
            play_address: PLAY,
            original_init_address: init,
            original_play_address: play,
            play_rate_numerator: 60,
            play_rate_denominator: 1,
            frame_divider: song.frame_divider,
            mapped_spans: song.mapped_spans.clone(),
            warnings: vec![
                "One selected original PSG song with returning INIT/PLAY routines and fixed flat Sega mapper banks.".into(),
                "SGC specifies 60 Hz NTSC calls, slightly faster than the original console frame rate; the original profile's frame divider is retained. Natural end and loop tags are not encoded.".into(),
            ],
        },
    })
}
