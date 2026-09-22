use std::sync::atomic::AtomicBool;

use super::*;

fn finite_stream() -> Vec<u8> {
    vec![0x81, 0x42, 0x90, 0x39, 0x01, 0xa3, 0x44, 0xb2, 0x38, 0]
}

#[test]
fn frame_stream_preserves_waits_loop_marker_and_stop_mutes() {
    let decoded = decode(&finite_stream(), 0, &AtomicBool::new(false)).unwrap();
    assert_eq!(decoded.frames, 4);
    assert_eq!(decoded.loop_offset, Some(5));
    assert_eq!(decoded.end_offset, 9);
    assert_eq!(
        decoded.spans,
        vec![FileSpan {
            offset: 0,
            byte_len: 10
        }]
    );
    assert_eq!(
        decoded
            .writes
            .iter()
            .map(|w| (w.frame, w.value))
            .collect::<Vec<_>>(),
        [
            (0, 0x81),
            (0, 0x42),
            (0, 0x90),
            (2, 0xa3),
            (2, 0x44),
            (2, 0xb2),
            (3, 0x9f),
            (3, 0xbf),
            (3, 0xdf),
            (3, 0xff)
        ]
    );
}

#[test]
fn substring_returns_after_last_byte_even_when_it_ends_a_frame() {
    let bytes = [
        0x08, 0x08, 0, 0x38, 0, 0xee, 0xee, 0xee, 0x81, 0x42, 0x90, 0x39,
    ];
    let decoded = decode(&bytes, 0, &AtomicBool::new(false)).unwrap();
    assert_eq!(decoded.frames, 4);
    assert_eq!(
        decoded.spans,
        [
            FileSpan {
                offset: 0,
                byte_len: 5
            },
            FileSpan {
                offset: 8,
                byte_len: 4
            }
        ]
    );
    assert_eq!(decoded.writes[0].source_offset, 8);
    assert_eq!(decoded.writes[3].frame, 3);
}

#[test]
fn truncated_invalid_and_unbounded_streams_are_rejected() {
    let cancel = AtomicBool::new(false);
    let bytes = finite_stream();
    for end in 0..bytes.len() {
        assert!(decode(&bytes[..end], 0, &cancel).is_err());
    }
    for bytes in [
        vec![0],
        vec![0x81, 0],
        vec![0x40, 0x38, 0],
        vec![0x90, 0x40, 0x38, 0],
        vec![0xe0, 0x40, 0x38, 0],
        vec![2],
        vec![7],
        vec![8, 0xff, 0xff],
        vec![8, 0, 0, 0],
        vec![8, 4, 0, 0, 0, 0, 0, 0],
        vec![8, 4, 0, 0, 1, 0, 0, 0],
        vec![0x81; MAX_STREAM_BYTES + 1],
    ] {
        assert!(decode(&bytes, 0, &cancel).is_err(), "{bytes:?}");
    }
    assert!(decode(&bytes, bytes.len() as u32, &cancel).is_err());
    assert!(decode(&bytes, u32::MAX, &cancel).is_err());
    assert!(decode(&bytes, 0, &AtomicBool::new(true)).is_err());
    let mut many_frames = vec![0x81];
    many_frames.extend(std::iter::repeat_n(0x3f, MAX_STREAM_BYTES));
    assert!(decode(&many_frames, 0, &cancel).is_err());
}

#[test]
fn export_is_a_playable_single_pass_projection() {
    let cancel = AtomicBool::new(false);
    let bytes = export_vgm(&finite_stream(), 0, FrameRate::Hz60, &cancel).unwrap();
    let scan = crate::vgm::scan(&bytes, crate::ScanLimits::default(), &cancel);
    assert_eq!(scan.status, crate::ScanStatus::Complete);
    let log = &scan.vgm_logs[0];
    assert_eq!(log.samples, 4 * 735);
    assert_eq!(log.loop_offset, None);
    assert!(log.warnings.is_empty());
    let prepared = crate::vgm::playback::prepare(&bytes, log, &cancel).unwrap();
    assert_eq!(prepared.writes.len(), 14);
    assert_eq!(prepared.writes[7].tick, 2 * 735);
    assert_eq!(prepared.writes[10].tick, 3 * 735);
    assert_eq!(prepared.duration_ticks, 4 * 735);
    assert_eq!(
        bytes,
        export_vgm(&finite_stream(), 0, FrameRate::Hz60, &cancel).unwrap()
    );
    let pal = export_vgm(&finite_stream(), 0, FrameRate::Hz50, &cancel).unwrap();
    let scan = crate::vgm::scan(&pal, crate::ScanLimits::default(), &cancel);
    assert_eq!(scan.vgm_logs[0].samples, 4 * 882);
}

#[test]
fn compressed_expansion_obeys_frame_and_write_limits() {
    let cancel = AtomicBool::new(false);
    for (payload, repeats, message) in [(0x3f, 530, "frame limit"), (0x81, 5200, "write limit")] {
        let mut bytes = vec![payload; 51];
        for _ in 0..repeats {
            bytes.extend([0x37, 0, 0]);
        }
        bytes.extend([0x38, 0]);
        assert!(bytes.len() < MAX_STREAM_BYTES);
        let error = decode(&bytes, 0, &cancel).unwrap_err();
        assert!(error.to_string().contains(message), "{error:#}");
    }
}
