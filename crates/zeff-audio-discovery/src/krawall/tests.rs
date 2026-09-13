use std::sync::atomic::{AtomicBool, Ordering};

use super::*;

const HEADER: usize = 0xc000;
const ROW: usize = 0x4000;
const INSTRUMENT: usize = 0x10200;
const SAMPLE: usize = 0x11000;

fn set_word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn set_half(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put(bytes: &mut [u8], offset: usize, value: &[u8]) {
    bytes[offset..offset + value.len()].copy_from_slice(value);
}

fn thumb_literal(bytes: &mut [u8], instruction: usize, slot: usize, register: u16) {
    let displacement = slot - ((instruction + 4) & !3);
    assert!(displacement.is_multiple_of(4) && displacement <= 1020);
    set_half(
        bytes,
        instruction,
        0x4800 | register << 8 | (displacement / 4) as u16,
    );
}

fn thumb_bl(bytes: &mut [u8], instruction: usize, target: usize) {
    let displacement = target as i32 - instruction as i32 - 4;
    set_half(
        bytes,
        instruction,
        0xf000 | ((displacement >> 12) as u16 & 2047),
    );
    set_half(
        bytes,
        instruction + 2,
        0xf800 | ((displacement >> 1) as u16 & 2047),
    );
}

fn copy_setup(bytes: &mut [u8], pc: usize, slot: usize, source: usize, target: u32, len: u32) {
    thumb_literal(bytes, pc, slot, 1);
    thumb_literal(bytes, pc + 2, slot + 4, 2);
    thumb_literal(bytes, pc + 4, slot + 8, 4);
    thumb_bl(bytes, pc + 6, 0x300);
    set_word(bytes, slot, 0x0800_0000 + source as u32);
    set_word(bytes, slot + 4, target);
    set_word(bytes, slot + 8, target + len);
}

fn fixture(encoding: PatternEncoding) -> Vec<u8> {
    let mut bytes = vec![0; 0x20000];
    let extended = encoding == PatternEncoding::Extended2004;
    put(
        &mut bytes,
        ROW,
        if extended {
            signatures::ROW_EXTENDED
        } else {
            signatures::ROW_PACKED
        },
    );
    put(
        &mut bytes,
        0x3800,
        if extended {
            signatures::INIT_RESET
        } else {
            signatures::INIT_SHORT
        },
    );
    put(
        &mut bytes,
        0x5000,
        if extended {
            signatures::PLAY_EXTENDED
        } else {
            signatures::PLAY_PACKED
        },
    );
    put(&mut bytes, 0x6800, signatures::INSTRUMENT_UPDATE);
    let bank_instructions = if extended {
        [ROW + 0xd2, ROW + 0xe0]
    } else {
        [ROW + 0xbe, ROW + 0xcc]
    };
    for (instruction, value) in [
        (bank_instructions[0], 0x0801_0000),
        (bank_instructions[1], 0x0801_0100),
        (ROW + 12, 0x0200_1000),
        (0x5000 + if extended { 40 } else { 28 }, 0x0200_1000),
    ] {
        let slot = driver::literal_slot(&bytes, instruction).unwrap();
        set_word(&mut bytes, slot, value);
    }
    copy_setup(&mut bytes, 0x140, 0x240, 0x8000, 0x0300_0000, 0x1000);
    copy_setup(&mut bytes, 0x14a, 0x24c, 0x9000, 0x0200_0000, 0x2000);
    put(&mut bytes, 0x300, signatures::COPY_THUMB);
    put(&mut bytes, 0x8200, signatures::MIXER);
    put(&mut bytes, 0x8500, signatures::IRQ_RESTORABLE);
    bytes[HEADER] = 1;
    bytes[HEADER + 1] = 3;
    bytes[HEADER + 3..HEADER + 6].copy_from_slice(&[0, 254, 1]);
    bytes[HEADER + 292] = 2;
    bytes[HEADER + 355] = 128;
    bytes[HEADER + 356] = 6;
    bytes[HEADER + 357] = 125;
    bytes[HEADER + 358] = 1;
    for (index, pattern) in [0xd000, 0xd200].into_iter().enumerate() {
        set_word(
            &mut bytes,
            HEADER + 364 + index * 4,
            0x0800_0000 + pattern as u32,
        );
        bytes[pattern + 32] = 4;
        let data = pattern + if extended { 34 } else { 33 };
        put(
            &mut bytes,
            data,
            &[0x20, if extended { 48 } else { 96 }, 1, 0, 0, 0, 0],
        );
    }
    set_word(&mut bytes, 0x10000, 0x0800_0000 + INSTRUMENT as u32);
    set_word(&mut bytes, 0x10100, 0x0800_0000 + SAMPLE as u32);
    set_word(&mut bytes, SAMPLE + 4, 0x0800_0000 + SAMPLE as u32 + 82);
    set_word(&mut bytes, SAMPLE + 8, 8363);
    bytes[SAMPLE + 14] = 64;
    bytes[SAMPLE + 18..SAMPLE + 82].fill(32);
    bytes
}

fn scan_fixture(bytes: &[u8]) -> (Vec<KrawallSong>, Result<(), ScanStop>) {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    let mut songs = Vec::new();
    let status = scan(bytes, &mut songs, &mut budget, 128);
    (songs, status)
}

#[test]
fn both_pattern_encodings_use_native_bank_references_and_subsongs() {
    for encoding in [PatternEncoding::Packed2003, PatternEncoding::Extended2004] {
        let bytes = fixture(encoding);
        let (songs, status) = scan_fixture(&bytes);
        assert_eq!(status, Ok(()));
        assert_eq!(songs.len(), 2);
        assert_eq!(
            songs
                .iter()
                .map(|song| (song.subsong, song.start_order))
                .collect::<Vec<_>>(),
            [(0, 0), (1, 2)]
        );
        for song in &songs {
            assert_eq!(song.channels, 1);
            assert_eq!(song.pattern_count, 2);
            assert_eq!(song.note_count, 2);
            assert_eq!(song.instrument_count, 1);
            assert_eq!(song.sample_count, 1);
            assert_eq!(song.native.encoding, encoding);
            assert_eq!(song.native.mixer.cpu_address, 0x0300_0200);
            assert_eq!(song.native.timer1_irq.cpu_address, 0x0300_0500);
            assert!(
                song.mapped_spans
                    .iter()
                    .any(|span| span.effective_offset <= SAMPLE as u32
                        && span.effective_offset + span.byte_len >= SAMPLE as u32 + 82)
            );
            validate_song(&bytes, song, &AtomicBool::new(false)).unwrap();
        }
    }
}

#[test]
fn overlapping_startup_copies_require_identical_destination_bytes() {
    let mut bytes = fixture(PatternEncoding::Extended2004);
    copy_setup(&mut bytes, 0x154, 0x260, 0xe000, 0x0300_0200, 0x100);
    assert!(scan_fixture(&bytes).0.is_empty());

    let mixer_data = bytes[0x8200..0x8300].to_vec();
    put(&mut bytes, 0xe000, &mixer_data);
    let (songs, status) = scan_fixture(&bytes);
    assert_eq!(status, Ok(()));
    assert_eq!(songs.len(), 2);
    assert_eq!(songs[0].native.ram_copies.len(), 3);
    assert_eq!(songs[0].native.mixer.cpu_address, 0x0300_0200);
}

#[test]
fn both_pattern_encodings_preserve_high_instrument_bits() {
    for encoding in [PatternEncoding::Packed2003, PatternEncoding::Extended2004] {
        let mut bytes = fixture(encoding);
        let (start, event): (usize, &[u8]) = match encoding {
            PatternEncoding::Packed2003 => (0xd000 + 33, &[0x20, 97, 1, 0, 0, 0, 0]),
            PatternEncoding::Extended2004 => (0xd000 + 34, &[0x20, 176, 1, 1, 0, 0, 0, 0]),
        };
        put(&mut bytes, start, event);
        assert!(scan_fixture(&bytes).0.is_empty());
        set_word(&mut bytes, 0x10400, 0x0800_0000 + INSTRUMENT as u32);
        let (songs, status) = scan_fixture(&bytes);
        assert_eq!(status, Ok(()));
        assert_eq!(songs.len(), 2);
        assert_eq!(songs[0].instrument_count, 2);
        assert_eq!(songs[0].sample_count, 1);
    }
}

#[test]
fn an_unused_invalid_note_map_entry_is_preserved_without_repair() {
    let mut bytes = fixture(PatternEncoding::Extended2004);
    set_half(&mut bytes, INSTRUMENT + 94 * 2, 0xdead);
    let (songs, status) = scan_fixture(&bytes);
    assert_eq!(status, Ok(()));
    assert_eq!(songs.len(), 2);
    let prepared = prepare_rom(&bytes, &songs[0], &AtomicBool::new(false)).unwrap();
    assert_eq!(half(&prepared, INSTRUMENT + 94 * 2), Some(0xdead));
    let mut reached = bytes;
    reached[0xd000 + 35] = 95;
    let (songs, status) = scan_fixture(&reached);
    assert!(songs.is_empty());
    assert_eq!(status, Err(ScanStop::ValidationLimit));
}

#[test]
fn transposed_notes_without_instruments_validate_the_next_sample() {
    let mut bytes = fixture(PatternEncoding::Extended2004);
    bytes[SAMPLE + 13] = 12;
    put(
        &mut bytes,
        0xd000 + 34,
        &[0x20, 48, 1, 0, 0x20, 48, 0, 0, 0, 0],
    );
    set_half(&mut bytes, INSTRUMENT + 59 * 2, 1);
    let (songs, status) = scan_fixture(&bytes);
    assert!(songs.is_empty());
    assert_eq!(status, Err(ScanStop::ValidationLimit));
    set_word(&mut bytes, 0x10104, 0x0801_1200);
    let sample = bytes[SAMPLE..SAMPLE + 82].to_vec();
    put(&mut bytes, 0x11200, &sample);
    set_word(&mut bytes, 0x11204, 0x0801_1252);
    bytes[0x1120d] = 0;
    let (songs, status) = scan_fixture(&bytes);
    assert_eq!(status, Ok(()));
    assert_eq!(songs[0].sample_count, 2);
}

#[test]
fn setup_and_driver_mutations_fail_closed() {
    let pristine = fixture(PatternEncoding::Extended2004);
    for (offset, value) in [
        (ROW + 6, 0),
        (0x300, 0),
        (0x8500, 0),
        (0x245, 0xff),
        (0x5001, 0),
    ] {
        let mut bytes = pristine.clone();
        bytes[offset] = value;
        assert!(scan_fixture(&bytes).0.is_empty(), "mutation at {offset:x}");
    }
    let (songs, _) = scan_fixture(&pristine);
    let mut stale = pristine;
    stale[HEADER + 357] = 126;
    assert!(prepare_rom(&stale, &songs[0], &AtomicBool::new(false)).is_err());
}

#[test]
fn scan_preserves_results_before_limits_and_observes_cancellation() {
    let bytes = fixture(PatternEncoding::Extended2004);
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    let mut songs = Vec::new();
    assert_eq!(
        scan(&bytes, &mut songs, &mut budget, 1),
        Err(ScanStop::CandidateLimit)
    );
    assert_eq!(songs.len(), 1);
    cancel.store(true, Ordering::Relaxed);
    assert_eq!(
        scan(&bytes, &mut songs, &mut budget, 128),
        Err(ScanStop::Cancelled)
    );
    assert_eq!(songs.len(), 1);
    assert!(prepare_rom(&bytes, &songs[0], &cancel).is_err());
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 0,
    };
    assert_eq!(
        scan(&bytes, &mut Vec::new(), &mut budget, 128),
        Err(ScanStop::WorkLimit)
    );
}

#[test]
fn bootstrap_changes_reset_and_appends_only_owned_code() {
    let bytes = fixture(PatternEncoding::Extended2004);
    let (songs, _) = scan_fixture(&bytes);
    let prepared = prepare_rom(&bytes, &songs[1], &AtomicBool::new(false)).unwrap();
    assert_eq!(&prepared[4..bytes.len()], &bytes[4..]);
    assert!(prepared.len() > bytes.len() && prepared.len() <= bytes.len() + 512);
    let reset = super::super::word(&prepared, 0).unwrap();
    assert_eq!(reset & 0xff00_0000, 0xea00_0000);
    assert_eq!(((reset & 0x00ff_ffff) * 4 + 8) as usize, bytes.len());
    assert!(
        prepared[bytes.len()..]
            .windows(4)
            .any(|value| value == 0xe3a0_2001u32.to_le_bytes())
    );
}

#[test]
fn full_rom_bootstrap_uses_padding_outside_mapped_data_and_footer() {
    let mut bytes = fixture(PatternEncoding::Extended2004);
    let (mut songs, _) = scan_fixture(&bytes);
    bytes.resize(super::super::MAX_ROM_BYTES, 0);
    let end = bytes.len();
    bytes[end - 256..end - 252].copy_from_slice(&[0x80, 0xff, 0x80, 0xff]);
    songs[0].mapped_spans.push(RomSpan::new(end - 2048, 512));
    let offset = bootstrap::placement(&bytes, &songs[0]).unwrap();
    assert!(offset + 512 <= end - 256);
    assert!(!(end - 2048..end - 1536).contains(&offset));
    songs[0].mapped_spans.push(RomSpan::new(end - 65536, 65536));
    assert!(bootstrap::placement(&bytes, &songs[0]).is_none());
}

#[test]
fn malformed_patterns_and_truncated_inputs_do_not_produce_songs() {
    let original = fixture(PatternEncoding::Extended2004);
    for offset in [HEADER + 1, HEADER + 356, 0xd000 + 32] {
        let mut bytes = original.clone();
        bytes[offset] = 0;
        assert!(scan_fixture(&bytes).0.is_empty());
    }
    for end in [0, 1, 3, 215, 367, ROW + 100, SAMPLE + 20] {
        assert!(scan_fixture(&original[..end]).0.is_empty());
    }
}
