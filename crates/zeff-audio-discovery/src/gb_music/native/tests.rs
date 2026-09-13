use super::*;
use crate::{ScanLimits, scan};
use zeff_emu_common::system::System;

#[test]
fn authored_source_discovers_native_playback_and_preserves_midi() {
    let bytes = fixture_rom();
    assert_eq!(zeff_firmware::sha256_hex(&bytes), fixture::PROFILE.sha256);
    let cancel = AtomicBool::new(false);
    let found = scan(System::Gb, &bytes, ScanLimits::default(), &cancel);
    assert_eq!(found.gb_songs.len(), 2);
    assert!(!supports_native(&found.gb_songs[0]));
    assert_eq!(found.gb_songs[1], fixture_song());
    let song = &found.gb_songs[1];
    assert!(supports_native(song));
    assert!(song.midi_exportable);
    assert!(super::super::midi(&bytes, song, 1, 2, &cancel).is_ok());
    let prepared = prepare_rom(&bytes, song, &cancel).unwrap();
    assert_eq!(prepared.timing, GbBankedTiming::Cgb);
    assert_eq!((prepared.wait_start, prepared.wait_end), (0xb7, 0xbd));
    assert_eq!(
        (prepared.ready_address, prepared.ready_value),
        (0xfffc, 0xa5)
    );
    assert_eq!((prepared.ack_address, prepared.ack_value), (0xfffb, 0x5a));
    for span in &song.mapped_spans {
        let start = span.offset as usize;
        let end = start + span.byte_len as usize;
        assert_eq!(bytes[start..end], prepared.bytes[start..end]);
    }
    assert_eq!(bytes[0x4000..], prepared.bytes[0x4000..]);
    for (offset, (&before, &after)) in bytes.iter().zip(&prepared.bytes).enumerate() {
        if before != after {
            assert!(
                (0xa0..0xda).contains(&offset)
                    || (0x40..0x43).contains(&offset)
                    || (0x22b..0x22e).contains(&offset)
            );
        }
    }
}

#[test]
fn profiles_gate_silence_and_indices_without_using_the_midi_projection() {
    let mut song = fixture_song();
    for (profile, count, hook, bank, selector) in [
        (super::super::PROFILE, 103, 0x22b, 0x9d, 0x3b97),
        (super::super::REV1_PROFILE, 103, 0x22b, 0x9d, 0x3b97),
        (super::super::REV2_PROFILE, 103, 0x22b, 0x9d, 0x3b24),
        (super::super::REV3_PROFILE, 93, 0x677, 0x9f, 0x3d98),
        (super::super::REV4_PROFILE, 93, 0x677, 0x9f, 0x3d98),
        (super::super::REV5_PROFILE, 93, 0x677, 0x9f, 0x3d98),
    ] {
        song.profile = profile;
        song.midi_exportable = false;
        for (index, accepted) in [(0, false), (1, true), (count - 1, true), (count, false)] {
            song.index = index;
            assert_eq!(supports_native(&song), accepted);
        }
        song.index = 1;
        let contract = contract(&song).unwrap();
        assert_eq!(contract.startup_hook, hook);
        assert_eq!(contract.bank_shadow, bank);
        assert_eq!(contract.selector, selector);
    }
    song.profile = "unknown";
    assert!(!supports_native(&song));
}

#[test]
fn preparation_rejects_source_and_inventory_mutations_and_cancellation() {
    let original = fixture_rom();
    let song = fixture_song();
    let cancel = AtomicBool::new(false);
    for offset in [0x143, 0x147, 0xa0, 0x22b, 0xe805c, 0xec104, 0x1fffff] {
        let mut bytes = original.clone();
        bytes[offset] ^= 1;
        assert!(prepare_rom(&bytes, &song, &cancel).is_err());
    }
    for modify in [
        |song: &mut GbSong| song.index = 0,
        |song: &mut GbSong| song.bank ^= 1,
        |song: &mut GbSong| song.title.push('!'),
        |song: &mut GbSong| song.midi_exportable = false,
        |song: &mut GbSong| song.mapped_spans.clear(),
    ] {
        let mut forged = song.clone();
        modify(&mut forged);
        assert!(prepare_rom(&original, &forged, &cancel).is_err());
    }
    assert!(prepare_rom(&original, &song, &AtomicBool::new(true)).is_err());
}

#[test]
fn bootstrap_retains_cgb_mapper_and_places_original_selector_in_de() {
    for contract in [REV0_CONTRACT, REV2_CONTRACT, REV3_CONTRACT] {
        let bytes = fixture_rom();
        let prepared = build(&bytes, 92, contract).unwrap();
        assert_eq!(&prepared.bytes[0xbd..0xc0], &[0x11, 92, 0]);
        assert_eq!(
            &prepared.bytes[0xc0..0xc3],
            &[
                0xcd,
                contract.selector as u8,
                (contract.selector >> 8) as u8
            ]
        );
        assert_eq!(
            &prepared.bytes[0xa6..0xad],
            &[0x3e, 0x3a, 0xe0, contract.bank_shadow, 0xea, 0, 0x20]
        );
        assert_eq!(&prepared.bytes[0x143..0x150], &bytes[0x143..0x150]);
    }
    for offset in [0xa0, 0xff, 0x143, 0x147, 0x148, 0x149] {
        let mut bytes = fixture_rom();
        bytes[offset] = 0xff;
        assert!(build(&bytes, 1, REV0_CONTRACT).is_err());
    }
    assert!(build(&fixture_rom()[..0x10000], 1, REV0_CONTRACT).is_err());
}
