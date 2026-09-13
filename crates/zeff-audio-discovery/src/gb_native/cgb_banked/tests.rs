use std::sync::atomic::AtomicBool;

use super::*;

fn songs(
    bytes: &[u8],
    limit: usize,
    work: u64,
) -> (Vec<GbNativeSong>, std::result::Result<(), ScanStop>) {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: work,
    };
    let mut found = Vec::new();
    let result = scan(bytes, &mut found, &mut budget, limit);
    (found, result)
}

#[test]
fn cgb_fixtures_bind_each_cartridge_and_packed_channel_header() {
    for (bytes, cartridge) in [
        (fixture::fixture_rom(), 0x19),
        (fixture::ram_fixture_rom(), 0x1b),
    ] {
        let (found, result) = songs(&bytes, 8, 20_000);
        result.unwrap();
        assert_eq!(found.len(), 3);
        assert_eq!(
            found.iter().map(|song| song.raw_index).collect::<Vec<_>>(),
            [0x31, 0x32, 0x33]
        );
        assert_eq!(
            found
                .iter()
                .map(|song| song.channels.len())
                .collect::<Vec<_>>(),
            [4, 3, 1]
        );
        for song in &found {
            assert_eq!(song.native.timing, GbNativeTiming::CgbDouble);
            assert_eq!(song.native.cartridge_type, cartridge);
            assert_eq!(song.table_entry.byte_len, 3);
            assert_eq!(song.header.byte_len, 1 + song.channels.len() as u32 * 2);
            for channel in &song.channels {
                assert_eq!(channel.entry.byte_len, 2);
                assert_eq!(channel.sequence.byte_len, 1);
            }
        }
    }
}

#[test]
fn original_cgb_speed_transition_and_irq_bytes_are_preserved() {
    let bytes = fixture::fixture_rom();
    let song = &songs(&bytes, 8, 20_000).0[0];
    let prepared = prepare_rom(&bytes, song, &AtomicBool::new(false)).unwrap();
    assert_eq!(&prepared.bytes[0x40..0x43], &bytes[0x40..0x43]);
    assert_eq!(&prepared.bytes[0x16d..0x16f], &[0x10, 0]);
    assert_eq!((prepared.wait_start, prepared.wait_end), (0x3f0a, 0x3f10));
    assert_eq!(
        (prepared.ready_address, prepared.ack_address),
        (0xfffc, 0xfffb)
    );
    for span in &song.mapped_spans {
        let start = span.effective_offset as usize;
        let end = start + span.byte_len as usize;
        assert_eq!(prepared.bytes[start..end], bytes[start..end]);
    }
    for (offset, (&before, &after)) in bytes.iter().zip(&prepared.bytes).enumerate() {
        if before != after {
            assert!((0x180..0x183).contains(&offset) || (0x3f00..0x3f40).contains(&offset));
        }
    }
}

#[test]
fn cgb_selections_require_exact_source_metadata_and_budget() {
    let bytes = fixture::fixture_rom();
    let (found, result) = songs(&bytes, 2, 20_000);
    assert_eq!(result, Err(ScanStop::CandidateLimit));
    assert_eq!(found.len(), 2);
    assert_eq!(songs(&bytes, 8, 8_192).1, Err(ScanStop::WorkLimit));
    let cancel = AtomicBool::new(false);
    let mut altered = found[0].clone();
    altered.playback_clocks += 1;
    assert!(prepare_rom(&bytes, &altered, &cancel).is_err());
    assert!(prepare_rom(&fixture::ram_fixture_rom(), &found[0], &cancel).is_err());
    assert!(prepare_rom(&bytes, &found[0], &AtomicBool::new(true)).is_err());
    let mut changed = bytes;
    changed[0x1fffff] ^= 1;
    assert!(songs(&changed, 8, 20_000).0.is_empty());
}
