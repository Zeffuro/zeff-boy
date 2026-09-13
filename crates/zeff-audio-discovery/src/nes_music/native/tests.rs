use super::*;

fn scan(bytes: &[u8]) -> crate::ScanReport {
    crate::scan(
        zeff_emu_common::system::System::Nes,
        bytes,
        Default::default(),
        &AtomicBool::new(false),
    )
}

#[test]
fn queue_preparation_preserves_source_and_validates_every_selection_field() {
    let bytes = fixture_rom();
    let report = scan(&bytes);
    assert_eq!(report.nes_songs.len(), 16);
    let cancel = AtomicBool::new(false);
    for song in &report.nes_songs {
        if matches!(song.index, 7 | 15) {
            assert!(!supports_native(song));
            assert!(prepare_rom(&bytes, song, &cancel).is_err());
            continue;
        }
        let prepared = prepare_rom(&bytes, song, &cancel).unwrap();
        assert_eq!(prepared.bytes.len(), bytes.len());
        assert_eq!(&prepared.bytes[0x110..0x800a], &bytes[0x110..0x800a]);
        assert_eq!(&prepared.bytes[0x8010..], &bytes[0x8010..]);
        assert_eq!(prepared.ready_address, 0x7f0);
        assert_eq!(prepared.ack_address, 0x7f1);
        assert!(prepared.wait_start < prepared.wait_end);
        let mut changed = song.clone();
        changed.selector ^= 1;
        assert!(prepare_rom(&bytes, &changed, &cancel).is_err());
        let mut changed = song.clone();
        changed.queue = match changed.queue {
            NesQueue::Event => NesQueue::Area,
            NesQueue::Area => NesQueue::Event,
        };
        assert!(prepare_rom(&bytes, &changed, &cancel).is_err());
        let mut changed = song.clone();
        changed.channels[0].note_count += 1;
        assert!(prepare_rom(&bytes, &changed, &cancel).is_err());
        assert!(prepare_rom(&bytes, song, &AtomicBool::new(true)).is_err());
    }
    for offset in [6, 9, 0x72e0, 0x791d, 0x7faa] {
        let mut changed = bytes.clone();
        changed[offset] ^= 1;
        assert!(prepare_rom(&changed, &report.nes_songs[8], &cancel).is_err());
    }
}
