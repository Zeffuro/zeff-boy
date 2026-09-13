use std::sync::atomic::AtomicBool;

use super::*;
use zeff_emu_common::system::System;

fn put_word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_half(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn address(offset: usize) -> u32 {
    0x0800_0000 + offset as u32
}

fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; 0x1000];
    let banner = b"GAX Sound Engine v2.01a (Jun 6 2001) \xa9 Shin'en Multimedia\0";
    bytes[0x40..0x40 + banner.len()].copy_from_slice(banner);
    let functions = [
        (0x500, V2_LEGACY_NEW[0].bytes),
        (0x540, V2_LEGACY_INIT[0].bytes),
        (0x580, V2_01_IRQ.bytes),
        (0x5c0, V2_01_PLAY[0].bytes),
    ];
    for (offset, signature) in functions {
        bytes[offset..offset + signature.len()].copy_from_slice(signature);
    }
    put_word(&mut bytes, 0x6e4, 0x0300_0010);
    put_word(&mut bytes, 0x618, 0x0300_0010);

    put_word(&mut bytes, 0x100, 4);
    for (index, handler) in [0x200, 0x220, 0x260, 0x280].into_iter().enumerate() {
        put_word(&mut bytes, 0x104 + index * 4, address(handler));
    }
    handler(&mut bytes, 0x200, 1, Some(0x2a0), 0x2f0);
    handler(&mut bytes, 0x220, 0, None, 0x300);
    handler(&mut bytes, 0x240, 0, None, 0x2d0);
    handler(&mut bytes, 0x260, 0, None, 0x2c0);
    handler(&mut bytes, 0x280, 0, None, 0x2b0);
    put_word(&mut bytes, 0x2a0, address(0x240));
    bytes[0x2c0..0x2d0].copy_from_slice(b"\"Fixture\" \xa9 Test");

    put_half(&mut bytes, 0x300, 4);
    put_half(&mut bytes, 0x302, 64);
    put_half(&mut bytes, 0x304, 2);
    put_word(&mut bytes, 0x30c, address(0x350));
    put_word(&mut bytes, 0x310, address(0x380));
    put_word(&mut bytes, 0x314, address(0x3a0));
    put_word(&mut bytes, 0x380, address(0x400));
    put_word(&mut bytes, 0x3a0, 0);
    bytes
}

fn current_fixture() -> Vec<u8> {
    let mut bytes = fixture();
    let banner = b"GAX Sound Engine v2.10 (Jan 1 2002) \xa9 Shin'en Multimedia\0";
    bytes[0x40..0x40 + banner.len()].copy_from_slice(banner);
    bytes[0x500..0x500 + NEW_SIGNATURES[1].bytes.len()].copy_from_slice(NEW_SIGNATURES[1].bytes);
    bytes[0x540..0x540 + INIT_SIGNATURES[4].bytes.len()].copy_from_slice(INIT_SIGNATURES[4].bytes);
    bytes[0x580..0x580 + MIX_SIGNATURES[3].bytes.len()].copy_from_slice(MIX_SIGNATURES[3].bytes);
    bytes[0x5c0..0x5c0 + PLAY_SIGNATURES[3].bytes.len()].copy_from_slice(PLAY_SIGNATURES[3].bytes);
    put_word(&mut bytes, 0x6f4, 0x0300_0010);
    bytes
}

fn legacy_v2_fixture() -> Vec<u8> {
    let mut bytes = fixture();
    let banner = b"GAX Sound Engine v2.02B (Oct 18 2001) Test\0";
    bytes[0x40..0x40 + banner.len()].copy_from_slice(banner);
    bytes[0x500..0x500 + V2_LEGACY_NEW[1].bytes.len()].copy_from_slice(V2_LEGACY_NEW[1].bytes);
    bytes[0x540..0x540 + V2_LEGACY_INIT[1].bytes.len()].copy_from_slice(V2_LEGACY_INIT[1].bytes);
    bytes
}

fn handler(bytes: &mut [u8], offset: usize, linked: u32, linked_at: Option<usize>, data: usize) {
    for delta in [0, 4, 8] {
        put_word(bytes, offset + delta, address(0x700 + delta));
    }
    put_word(bytes, offset + 12, linked);
    if let Some(linked_at) = linked_at {
        put_word(bytes, offset + 16, address(linked_at));
    }
    put_word(bytes, offset + 24, address(data));
}

fn scan_fixture(bytes: &[u8]) -> Vec<GaxNativeSong> {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000_000,
    };
    let mut songs = Vec::new();
    scan(bytes, &mut songs, &mut budget, 64).unwrap();
    songs
}

#[test]
fn scan_requires_all_original_driver_witnesses() {
    let mut bytes = fixture();
    bytes[0x580] ^= 1;
    assert!(scan_fixture(&bytes).is_empty());
}

#[test]
fn scan_inventory_binds_v2_song_to_original_entrypoints() {
    let songs = scan_fixture(&fixture());
    assert_eq!(songs.len(), 1);
    let song = &songs[0];
    assert_eq!(song.header.effective_offset, 0x100);
    assert_eq!(song.title, "\"Fixture\" © Test");
    assert_eq!(song.channels, 4);
    assert_eq!(song.native.layout, GaxNativeLayout::V2_01);
    assert_eq!(song.native.new.unwrap().cpu_address, address(0x500) | 1);
    assert_eq!(song.native.init.cpu_address, address(0x540) | 1);
    assert_eq!(song.native.mix.cpu_address, address(0x580) | 1);
    assert_eq!(song.native.play.cpu_address, address(0x5c0) | 1);
    assert_eq!(song.native.work_ram, 0x0300_0014);
}

#[test]
fn public_scan_retains_native_songs_for_each_v2_layout() {
    for (bytes, layout) in [
        (fixture(), GaxNativeLayout::V2_01),
        (current_fixture(), GaxNativeLayout::V2Current),
    ] {
        let report = crate::scan(
            System::Gba,
            &bytes,
            crate::ScanLimits::default(),
            &AtomicBool::new(false),
        );
        assert_eq!(report.gax_native_songs.len(), 1);
        assert_eq!(report.gax_native_songs[0].native.layout, layout);
    }
}

#[test]
fn legacy_v2_requires_one_local_driver_and_shared_irq_play_state() {
    let songs = scan_fixture(&legacy_v2_fixture());
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].native.layout, GaxNativeLayout::V2_01);
    assert_eq!(songs[0].native.new.unwrap().cpu_address, address(0x500) | 1);

    let mut mismatched_state = legacy_v2_fixture();
    put_word(&mut mismatched_state, 0x618, 0x0300_0020);
    assert!(scan_fixture(&mismatched_state).is_empty());
}

#[test]
fn prepare_rom_installs_v2_bootstrap_and_rejects_forged_inventory() {
    let bytes = fixture();
    let song = scan_fixture(&bytes).pop().unwrap();
    let cancel = AtomicBool::new(false);
    let patched = prepare_rom(&bytes, &song, &cancel).unwrap();
    let base = (bytes.len() + 3) & !3;
    assert!(patched.len() > base);
    assert_eq!(word(&patched, 0), Some(0xea00_03fe));
    for address in [
        address(0x500),
        address(0x540),
        address(0x580),
        address(0x5c0),
    ] {
        assert!(
            patched[base..]
                .windows(4)
                .any(|window| window == (address | 1).to_le_bytes())
        );
    }
    assert!(
        patched[base..]
            .windows(4)
            .any(|window| window == address(0x100).to_le_bytes())
    );
    assert!(
        patched[base..]
            .windows(4)
            .any(|window| window == 0xe1c4_01b0u32.to_le_bytes())
    );
    assert!(
        !patched[base..]
            .windows(4)
            .any(|window| window == 0xe1c4_01b2u32.to_le_bytes())
    );

    let mut forged = song;
    forged.channels = 7;
    assert!(prepare_rom(&bytes, &forged, &cancel).is_err());
}

#[test]
fn scan_propagates_cancellation_while_rejecting_headers() {
    let bytes = fixture();
    let cancel = AtomicBool::new(true);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000_000,
    };
    let mut songs = Vec::new();
    assert_eq!(
        scan(&bytes, &mut songs, &mut budget, 64),
        Err(ScanStop::Cancelled)
    );
}

#[test]
fn header_parser_preserves_work_limit() {
    let bytes = fixture();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 0,
    };
    assert!(matches!(
        parse_song(&bytes, 0x100, &mut budget),
        Err(ParseError::Stop(ScanStop::WorkLimit))
    ));
}

#[test]
fn candidate_limit_preserves_complete_songs() {
    let bytes = fixture();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000_000,
    };
    let mut songs = Vec::new();
    assert_eq!(
        scan(&bytes, &mut songs, &mut budget, 0),
        Err(ScanStop::CandidateLimit)
    );
    assert!(songs.is_empty());
}

#[test]
fn workspace_excludes_copied_runtime_and_reserved_stacks() {
    let mut native = scan_fixture(&current_fixture()).remove(0).native;
    assert_eq!(driver::workspace(&native), Some(0x0300_00b0));
    native.ram_copies.push(GaxRamCopy {
        source: RomSpan::new(0, 0x1048),
        destination: 0x0300_0000,
    });
    assert_eq!(driver::workspace(&native), Some(0x0300_1100));
    native.ram_copies[0].source.byte_len = 0x6000;
    assert!(driver::workspace(&native).is_none());
    native.ram_copies[0].source.byte_len = 0x4000;
    assert_eq!(driver::workspace(&native), Some(0x0300_4000));
    native.layout = GaxNativeLayout::V2_01;
    assert!(driver::workspace(&native).is_none());
}

#[test]
fn current_workspace_uses_available_ram_below_stacks_and_driver_state() {
    let mut bytes = current_fixture();
    let mut native = scan_fixture(&bytes).remove(0).native;
    let (slot, _) = state_slot(&bytes, native.play.source.effective_offset as usize).unwrap();
    assert_eq!(
        driver::v2_workspace(&bytes, &native),
        Some((0x0300_00b0, 0x7d50))
    );

    put_word(&mut bytes, slot, 0x0300_5f20);
    native.work_ram = DEFAULT_WORK_RAM;
    assert_eq!(
        driver::v2_workspace(&bytes, &native),
        Some((0x0300_009c, 0x5e84))
    );
    native.ram_copies.push(GaxRamCopy {
        source: RomSpan::new(0, 0x1048),
        destination: 0x0300_0000,
    });
    assert_eq!(
        driver::v2_workspace(&bytes, &native),
        Some((0x0300_1100, 0x4e20))
    );

    put_word(&mut bytes, slot, 0x0300_3100);
    assert_eq!(
        driver::v2_workspace(&bytes, &native),
        Some((0x0300_1100, 0x2000))
    );
    put_word(&mut bytes, slot, 0x0300_30fc);
    assert!(driver::v2_workspace(&bytes, &native).is_none());
    put_word(&mut bytes, slot, 0x0300_1100);
    assert!(driver::v2_workspace(&bytes, &native).is_none());

    put_word(&mut bytes, slot, 0x0200_0000);
    assert_eq!(
        driver::v2_workspace(&bytes, &native),
        Some((0x0300_1100, 0x6d00))
    );
    put_word(&mut bytes, slot, 0);
    assert!(driver::v2_workspace(&bytes, &native).is_none());
    native.layout = GaxNativeLayout::V2_01;
    assert_eq!(
        driver::v2_workspace(&bytes, &native),
        Some((0x0300_1100, 0x4000))
    );
}
