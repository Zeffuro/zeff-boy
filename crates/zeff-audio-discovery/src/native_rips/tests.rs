use super::*;
use crate::{
    ScanLimits,
    rips::{self, RipFormat},
};
use zeff_emu_common::system::System;

fn scan(bytes: &[u8], system: System) -> crate::ScanReport {
    crate::scan(
        system,
        bytes,
        ScanLimits::default(),
        &AtomicBool::new(false),
    )
}

#[test]
fn gbs_preserves_logical_banks_and_returning_entry_points() {
    let bytes = crate::gb_native::fixture_rom();
    let report = scan(&bytes, System::Gb);
    let song = &report.gb_native_songs[0];
    assert_eq!(
        supported_format(SongRef::GbNative(song)),
        Some(NativeRipFormat::Gbs)
    );
    let rip = encode(&bytes, SongRef::GbNative(song), &AtomicBool::new(false)).unwrap();
    let container = rips::inspect(
        &rip.bytes,
        RipFormat::Gbs,
        ScanLimits::default(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!((container.song_count, container.first_song), (1, 1));
    assert_eq!(
        (
            container.load_address,
            container.init.cpu_address,
            container.play.cpu_address
        ),
        (0x400, 0x800, 0x880)
    );
    assert!(container.warnings.is_empty());
    assert_eq!(rip.bytes[0x70 + 0x400 + 26], song.raw_index);
    assert_eq!(&rip.bytes[12..16], &[0xf0, 0xff, 0, 0]);
    for span in &song.mapped_spans {
        let source = span.effective_offset as usize;
        let count = span.byte_len as usize;
        assert_eq!(
            &rip.bytes[0x70 + source - 0x400..0x70 + source - 0x400 + count],
            &bytes[source..source + count]
        );
    }
    let mut changed = song.clone();
    changed.bank = 3;
    assert_eq!(supported_format(SongRef::GbNative(&changed)), None);
    assert!(encode(&bytes, SongRef::GbNative(&changed), &AtomicBool::new(false)).is_err());
    let mut changed = bytes.clone();
    changed[0x200e] ^= 1;
    assert!(encode(&changed, SongRef::GbNative(song), &AtomicBool::new(false)).is_err());
}

#[test]
fn nsf_contains_original_driver_with_returning_single_song_init() {
    let bytes = crate::nes_native::fixture_rom();
    let report = scan(&bytes, System::Nes);
    for song in &report.nes_native_songs {
        let selection = SongRef::NesNative(song);
        assert_eq!(supported_format(selection), Some(NativeRipFormat::Nsf));
        let rip = encode(&bytes, selection, &AtomicBool::new(false)).unwrap();
        let container = rips::inspect(
            &rip.bytes,
            RipFormat::Nsf,
            ScanLimits::default(),
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap();
        assert_eq!((container.song_count, container.first_song), (1, 1));
        assert_eq!(
            (container.init.cpu_address, container.play.cpu_address),
            (0x8000, 0xed30)
        );
        assert!(container.warnings.is_empty());
        assert_eq!(rip.bytes[0x80 + 13], song.raw_index);
        assert_eq!(rip.bytes[0x80 + 17], 0x60);
        assert_eq!(
            rip.metadata.source_sha256,
            zeff_firmware::sha256_hex(&bytes)
        );
        assert_eq!(
            rip.metadata.output_sha256,
            zeff_firmware::sha256_hex(&rip.bytes)
        );
        for span in &song.mapped_spans {
            let source = span.effective_offset as usize;
            let output = 0x80 + span.canonical_cpu_address as usize - 0x8000;
            let count = span.byte_len as usize;
            assert_eq!(
                &rip.bytes[output..output + count],
                &bytes[source..source + count]
            );
        }
    }
}

#[test]
fn sgc_sets_system_flat_banks_stack_and_selected_driver() {
    for (bytes, system) in [
        (crate::sega_psg::fixture_rom(), System::Sms),
        (crate::sega_psg::fixture_rom_six_byte(), System::Gg),
    ] {
        let report = scan(&bytes, system);
        let song = &report.sega_psg_songs[0];
        let rip = encode(&bytes, SongRef::SegaPsg(song), &AtomicBool::new(false)).unwrap();
        assert_eq!(&rip.bytes[..5], b"SGC\x1a\x01");
        assert_eq!(&rip.bytes[8..16], &[0, 4, 0, 8, 0x40, 8, 0xf0, 0xdd]);
        assert_eq!(&rip.bytes[0x20..0x24], &[0, 0, 1, 2]);
        assert_eq!((rip.bytes[0x24], rip.bytes[0x25]), (0, 1));
        assert_eq!(rip.bytes[0x28], u8::from(system == System::Gg));
        assert_eq!(rip.metadata.original_init_address, 0x4050);
        assert_eq!(rip.metadata.raw_selector, 0x81);
        assert_eq!(&rip.bytes[0xa0 + 0x440..0xa0 + 0x443], &[0xc3, 0, 0x40]);
        for span in &song.mapped_spans {
            let source = span.effective_offset as usize;
            let output = 0xa0 + span.canonical_cpu_address as usize - 0x400;
            let count = span.byte_len as usize;
            assert_eq!(
                &rip.bytes[output..output + count],
                &bytes[source..source + count]
            );
        }
    }
}

#[test]
fn reserved_sgc_low_rom_data_remains_unavailable() {
    let bytes = crate::sega_psg::fixture_rom_supplemental();
    let report = scan(&bytes, System::Gg);
    let song = SongRef::SegaPsg(&report.sega_psg_songs[0]);
    assert_eq!(supported_format(song), None);
    assert!(encode(&bytes, song, &AtomicBool::new(false)).is_err());
}

#[test]
fn sgc_late_host_dependencies_do_not_advertise_an_export() {
    let bytes = crate::sega_psg::fixture_rom();
    let report = scan(&bytes, System::Sms);
    for (profile, raw_index) in [
        ("sega-psg-bank-v1-06", 0x81),
        ("sega-psg-bank-v1-08", 0x86),
        ("sega-psg-bank-v1-15", 0x83),
        ("sega-psg-bank-v1-26", 0x88),
        ("sega-psg-bank-v1-26", 0x8a),
        ("sega-psg-bank-v1-37", 0x81),
        ("sega-psg-bank-v1-06", 0x90),
    ] {
        let mut song = report.sega_psg_songs[0].clone();
        song.profile = profile;
        song.raw_index = raw_index;
        let selection = SongRef::SegaPsg(&song);
        assert_eq!(supported_format(selection), None);
        assert!(encode(&bytes, selection, &AtomicBool::new(false)).is_err());
    }
}

#[test]
fn native_exports_revalidate_inventory_source_and_cancellation() {
    let mut bytes = crate::nes_native::fixture_rom();
    let report = scan(&bytes, System::Nes);
    let song = &report.nes_native_songs[0];
    assert!(encode(&bytes, SongRef::NesNative(song), &AtomicBool::new(true)).is_err());
    let mut changed = song.clone();
    changed.native.mapper = 1;
    assert_eq!(supported_format(SongRef::NesNative(&changed)), None);
    assert!(
        encode(
            &bytes,
            SongRef::NesNative(&changed),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    let mut changed = song.clone();
    changed.raw_index ^= 1;
    assert!(
        encode(
            &bytes,
            SongRef::NesNative(&changed),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    bytes[0x6c5c] ^= 1;
    assert!(encode(&bytes, SongRef::NesNative(song), &AtomicBool::new(false)).is_err());

    let mut bytes = crate::sega_psg::fixture_rom();
    let report = scan(&bytes, System::Sms);
    let song = &report.sega_psg_songs[0];
    let mut changed = song.clone();
    changed.frame_divider = 2;
    assert!(encode(&bytes, SongRef::SegaPsg(&changed), &AtomicBool::new(false)).is_err());
    bytes[0x8050] ^= 1;
    assert!(encode(&bytes, SongRef::SegaPsg(song), &AtomicBool::new(false)).is_err());
}

#[test]
fn new_cgb_and_mmc1_profiles_require_separate_native_rip_qualification() {
    let cancel = AtomicBool::new(false);
    let bytes = crate::gb_native::cgb_fixture_rom();
    let report = scan(&bytes, System::Gb);
    assert!(!report.gb_native_songs.is_empty());
    for song in &report.gb_native_songs {
        let selection = SongRef::GbNative(song);
        assert_eq!(supported_format(selection), None);
        assert!(encode(&bytes, selection, &cancel).is_err());
    }
    let bytes = crate::nes_native::fixture_rom_nintendo();
    let report = scan(&bytes, System::Nes);
    assert!(!report.nes_native_songs.is_empty());
    for song in &report.nes_native_songs {
        let selection = SongRef::NesNative(song);
        assert_eq!(supported_format(selection), None);
        assert!(encode(&bytes, selection, &cancel).is_err());
    }
}

#[test]
fn mapping_rejects_conflicts_bounds_and_wrapper_collisions() {
    let span = RomSpan {
        effective_offset: 0,
        byte_len: 2,
        canonical_cpu_address: 0x8000,
    };
    assert!(mapped_image(&[1], &[span], 0x8000, 0x10000).is_err());
    assert!(mapped_image(&[1, 2], &[span], 0x9000, 0x10000).is_err());
    assert!(mapped_image(&[1, 2], &[span], 0x8000, 0x8001).is_err());
    let conflict = RomSpan {
        effective_offset: 1,
        byte_len: 1,
        ..span
    };
    assert!(mapped_image(&[1, 2], &[span, conflict], 0x8000, 0x10000).is_err());
    let mut image = mapped_image(&[1, 2], &[span], 0x8000, 0x10000).unwrap();
    assert!(place_wrapper(&mut image, &[span], 0x8000, 0x8001, &[0x60]).is_err());
}
