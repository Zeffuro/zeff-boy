use super::*;

fn fixture() -> (Vec<u8>, BoundSong) {
    let bytes = crate::huge::discovery::tests::fixture(0x1800, 0xc1fd);
    let selected = discover(&bytes, Default::default(), &AtomicBool::new(false))
        .unwrap()
        .bound
        .remove(0);
    (bytes, selected)
}

#[test]
fn copies_only_validated_audio_and_rebuilds_a_discoverable_player() {
    let (bytes, selected) = fixture();
    for fill in [0, 0xff, 0xa5] {
        let isolated = build(&bytes, &selected, fill, &AtomicBool::new(false)).unwrap();
        assert_eq!(isolated.bytes.len(), 0x8000);
        assert_eq!((isolated.init_call, isolated.update_call), (0x16c, 0x180));
        assert_eq!((isolated.ram_start, isolated.ram_end), (0xc1fd, 0xc261));
        for (at, &original) in bytes.iter().enumerate().skip(0x200) {
            let copied = isolated
                .copied_spans
                .iter()
                .any(|s| (s.offset as usize..(s.offset + s.byte_len) as usize).contains(&at));
            assert_eq!(isolated.bytes[at], if copied { original } else { fill });
        }
        let report =
            discover(&isolated.bytes, Default::default(), &AtomicBool::new(false)).unwrap();
        assert_eq!(report.bound.len(), 1);
        assert_eq!(report.bound[0].song, selected.song);
        assert!(report.held.is_empty());
        let header_sum = isolated.bytes[0x134..0x14e]
            .iter()
            .fold(0u8, |sum, b| sum.wrapping_add(*b).wrapping_add(1));
        assert_eq!(header_sum, 1);
        let checksum = isolated
            .bytes
            .iter()
            .enumerate()
            .filter(|(i, _)| !matches!(i, 0x14e | 0x14f))
            .fold(0u16, |sum, (_, b)| sum.wrapping_add(u16::from(*b)));
        assert_eq!(
            u16::from_be_bytes([isolated.bytes[0x14e], isolated.bytes[0x14f]]),
            checksum
        );
    }
}

#[test]
fn changed_audio_forged_selections_cgb_and_cancellation_reject() {
    let (bytes, selected) = fixture();
    let cancel = AtomicBool::new(false);
    let mut changed = bytes.clone();
    changed[0x1000] += 1;
    assert!(build(&changed, &selected, 0, &cancel).is_err());
    let mut forged = selected.clone();
    forged.evidence.ram_address += 1;
    assert!(build(&bytes, &forged, 0, &cancel).is_err());
    let mut forged = selected.clone();
    forged.song.spans.pop();
    assert!(build(&bytes, &forged, 0, &cancel).is_err());
    changed = bytes.clone();
    changed[0x143] = 0x80;
    assert!(build(&changed, &selected, 0, &cancel).is_err());
    assert!(build(&bytes, &selected, 0, &AtomicBool::new(true)).is_err());
    changed = bytes.clone();
    changed[0x7000] ^= 0xff;
    assert_eq!(
        build(&changed, &selected, 0, &cancel).unwrap().bytes,
        build(&bytes, &selected, 0, &cancel).unwrap().bytes
    );
}

#[test]
fn source_data_in_bootstrap_window_is_not_overwritten() {
    let (mut bytes, _) = fixture();
    bytes[0x213..0x215].copy_from_slice(&0x120u16.to_le_bytes());
    let selected = discover(&bytes, Default::default(), &AtomicBool::new(false))
        .unwrap()
        .bound
        .remove(0);
    assert!(build(&bytes, &selected, 0, &AtomicBool::new(false)).is_err());
}
