use super::*;
use crate::gax_native::{
    GaxNativeEntry, GaxNativeLayout, GaxNativeProfile, GaxRamCopy, RomSpan, driver, v3,
};

fn song(layout: GaxNativeLayout) -> GaxNativeSong {
    let entry = |offset| GaxNativeEntry {
        source: RomSpan::new(offset, 32),
        cpu_address: 0x0800_0001 + offset as u32,
    };
    GaxNativeSong {
        header: RomSpan::new(0x100, 32),
        index: 0,
        title: "synthetic".into(),
        channels: 4,
        native: GaxNativeProfile {
            version: "synthetic".into(),
            layout,
            new: Some(entry(0x500)),
            init: entry(0x600),
            mix: entry(0x700),
            play: entry(0x800),
            work_ram: 0x0300_0000,
            sample_rate: u16::MAX,
            ram_copies: Vec::new(),
        },
        mapped_spans: vec![RomSpan::new(0x100, 32)],
        warnings: Vec::new(),
    }
}

fn full_rom(value: u8) -> Vec<u8> {
    let mut bytes = vec![0xa5; MAX_ROM_BYTES];
    bytes[..4].copy_from_slice(&0xea00_002eu32.to_le_bytes());
    bytes[0x800..0x802].copy_from_slice(&0x4800u16.to_le_bytes());
    bytes[0x804..0x808].copy_from_slice(&0x0300_0000u32.to_le_bytes());
    bytes[MAX_ROM_BYTES - TAIL_BYTES..MAX_ROM_BYTES - FOOTER_BYTES].fill(value);
    bytes
}

#[test]
fn full_size_drivers_preserve_every_byte_outside_reset_and_owned_padding() {
    for (layout, value) in [
        (GaxNativeLayout::V2Current, 0),
        (GaxNativeLayout::V2_01, 255),
        (GaxNativeLayout::V3Legacy, 0),
        (GaxNativeLayout::V3Modern, 255),
    ] {
        let bytes = full_rom(value);
        let mut song = song(layout);
        song.mapped_spans
            .push(RomSpan::new(MAX_ROM_BYTES - 1024, 512));
        let build = if matches!(
            layout,
            GaxNativeLayout::V3Legacy | GaxNativeLayout::V3Modern
        ) {
            v3::build
        } else {
            driver::build
        };
        let prepared = build(&bytes, &song).unwrap();
        let offset = ((word(&prepared, 0).unwrap() & 0x00ff_ffff) as usize * 4) + 8;
        let small = build(&bytes[..0x1000], &song).unwrap();
        let length = small.len() - 0x1000;
        assert_eq!(choose(&bytes, &song, length), Some(offset));
        assert_eq!(prepared.len(), bytes.len());
        assert!(offset + length <= MAX_ROM_BYTES - 1024);
        assert_eq!(&prepared[4..offset], &bytes[4..offset]);
        assert_eq!(&prepared[offset + length..], &bytes[offset + length..]);
        let old_handler = small[0x1000..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|v| u32::from_le_bytes(*v))
            .find(|&value| (0x0800_1000..0x0800_1000 + length as u32).contains(&value))
            .unwrap();
        let new_handler = 0x0800_0000 + offset as u32 + (old_handler - 0x0800_1000);
        assert!(
            prepared[offset..offset + length]
                .as_chunks::<4>()
                .0
                .iter()
                .any(|v| *v == new_handler.to_le_bytes())
        );
    }
}

#[test]
fn padding_must_avoid_mapped_spans_copied_data_and_original_code() {
    let bytes = full_rom(0);
    let mut song = song(GaxNativeLayout::V2Current);
    let tail = MAX_ROM_BYTES - 4096;
    song.native.ram_copies.push(GaxRamCopy {
        source: RomSpan::new(tail, 3840),
        destination: 0x0200_0000,
    });
    let offset = choose(&bytes, &song, 512).unwrap();
    assert!(offset + 512 <= tail);
    song.native.ram_copies.clear();
    song.native.init.source = RomSpan::new(MAX_ROM_BYTES - 8192, 32);
    let offset = choose(&bytes, &song, 512).unwrap();
    assert!(offset + 512 <= MAX_ROM_BYTES - 8192 - 256);
    song.native.init.source = RomSpan::new(0x600, 32);
    let mut reset_at_tail = bytes.clone();
    let entry = MAX_ROM_BYTES - 4096;
    reset_at_tail[..4].copy_from_slice(&(0xea00_0000 | ((entry as u32 - 8) >> 2)).to_le_bytes());
    assert!(choose(&reset_at_tail, &song, 512).unwrap() + 512 <= entry);
    song.mapped_spans
        .push(RomSpan::new(MAX_ROM_BYTES - TAIL_BYTES, TAIL_BYTES));
    assert!(choose(&bytes, &song, 512).is_none());
    assert!(driver::build(&bytes, &song).is_err());
}

#[test]
fn full_size_rom_without_uniform_tail_padding_is_rejected() {
    let mut bytes = vec![0xa5; MAX_ROM_BYTES];
    bytes[..4].copy_from_slice(&0xea00_002eu32.to_le_bytes());
    let song = song(GaxNativeLayout::V3Modern);
    assert!(choose(&bytes, &song, 512).is_none());
    assert!(v3::build(&bytes, &song).is_err());
    bytes[MAX_ROM_BYTES - 4096..MAX_ROM_BYTES - FOOTER_BYTES].fill(0);
    bytes[..4].fill(0);
    assert!(choose(&bytes, &song, 512).is_none());
}

#[test]
fn appended_payload_preserves_source_and_alignment() {
    let bytes = vec![0xa5; 0x1001];
    let offset = choose(&bytes, &song(GaxNativeLayout::V2Current), 12).unwrap();
    assert_eq!(offset, 0x1004);
    let prepared = install(&bytes, offset, &[0x5a; 12]).unwrap();
    assert_eq!(&prepared[4..bytes.len()], &bytes[4..]);
    assert_eq!(&prepared[bytes.len()..offset], &[0; 3]);
    assert_eq!(&prepared[offset..], &[0x5a; 12]);
}
