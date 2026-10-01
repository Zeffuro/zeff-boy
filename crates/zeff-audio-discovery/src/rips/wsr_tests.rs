use std::sync::atomic::AtomicBool;

use super::*;
use crate::{
    DetectorState, MAX_ROM_BYTES, ScanStatus,
    catalog::SongRef,
    formats::{AudioFormat, SongFormat},
    native_rips,
};

fn read(bytes: &[u8]) -> MusicRip {
    inspect(
        bytes,
        RipFormat::Wsr,
        ScanLimits::default(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .expect("valid WSR fixture")
}

fn source(byte_len: usize) -> Vec<u8> {
    assert!(byte_len >= 0x1_0000 && byte_len.is_multiple_of(0x1_0000));
    let mut bytes = vec![0x5a; byte_len];
    let trailer = bytes.len() - 32;
    bytes[trailer..trailer + 4].copy_from_slice(b"WSRF");
    bytes[trailer + 4] = 0xa5;
    bytes[trailer + 5] = 0xff;
    bytes[trailer + 6..trailer + 16].copy_from_slice(&[0xa0; 10]);
    bytes[trailer + 16..trailer + 22].copy_from_slice(&[0xea, 0, 0, 0, 0xf0, 0x90]);
    bytes[trailer + 22..].copy_from_slice(&[0; 10]);
    bytes
}

fn marked_source(byte_len: usize) -> Vec<u8> {
    assert!(byte_len >= 32);
    let mut bytes = vec![0; byte_len];
    let trailer = bytes.len() - 32;
    bytes[trailer..trailer + 4].copy_from_slice(b"WSRF");
    bytes
}

fn status(bytes: &[u8]) -> ScanStatus {
    scan(
        bytes,
        RipFormat::Wsr,
        ScanLimits::default(),
        &AtomicBool::new(false),
    )
    .status
}

#[test]
fn wsr_preserves_aligned_body_and_raw_trailer() {
    let bytes = crate::test_support::rips::wsr_fixture();
    let rip = read(&bytes);
    assert_eq!(rip.format, RipFormat::Wsr);
    assert_eq!(rip.version, None);
    assert_eq!(
        (
            rip.song_count,
            rip.first_song,
            rip.load_address,
            rip.init,
            rip.play,
        ),
        (None, None, None, None, None)
    );
    assert!(rip.title.is_empty() && rip.author.is_empty() && rip.copyright.is_empty());
    assert_eq!(rip.sha256, zeff_firmware::sha256_hex(&bytes));
    assert_eq!(
        rip.header,
        FileSpan {
            offset: 0xffe0,
            byte_len: 32
        }
    );
    assert_eq!(
        rip.program,
        FileSpan {
            offset: 0,
            byte_len: 0xffe0
        }
    );
    let RipDetails::Wsr {
        raw_byte_4,
        raw_start_song,
        opaque_trailer,
        reset_entry,
        cartridge_footer,
    } = rip.details
    else {
        panic!("expected WSR details");
    };
    assert_eq!((raw_byte_4, raw_start_song), (0xa5, 0xff));
    assert_eq!(opaque_trailer, [0; 10]);
    assert_eq!(
        reset_entry,
        FileSpan {
            offset: 0xfff0,
            byte_len: 6
        }
    );
    assert_eq!(
        cartridge_footer,
        FileSpan {
            offset: 0xfff6,
            byte_len: 10
        }
    );
    let json = serde_json::to_value(read(&bytes)).unwrap();
    for field in [
        "version",
        "song_count",
        "first_song",
        "load_address",
        "init",
        "play",
    ] {
        assert_eq!(json[field], serde_json::Value::Null);
    }
    assert_eq!(json["details"]["raw_byte_4"], 0xa5);
    assert_eq!(json["details"]["raw_start_song"], 255);
    let selected_rip = read(&bytes);
    let selected = SongRef::Rip(&selected_rip);
    assert!(selected.supports(SongFormat::Wsr));
    assert!(selected.supports(SongFormat::MappedAssets));
    assert!(!selected.supports(SongFormat::Nsf));
    assert!(!selected.supports(SongFormat::Audio(AudioFormat::Wav)));
    assert!(
        native_rips::encode_as(
            &bytes,
            selected,
            native_rips::NativeRipFormat::Nsf,
            &AtomicBool::new(false)
        )
        .is_err()
    );
}

#[test]
fn wsr_requires_aligned_source_and_marker_at_source_eof() {
    for bytes in [
        Vec::new(),
        vec![0; 0xffff],
        vec![0; 0x1_0001],
        vec![0; 0x1_8000],
        marked_source(0x8000),
        marked_source(0x1_8000),
    ] {
        assert_eq!(status(&bytes), ScanStatus::Unsupported);
    }
    let bytes = source(0x3_0000);
    let rip = read(&bytes);
    assert_eq!(rip.source.byte_len, 0x3_0000);
    assert_eq!(rip.header.offset, 0x2_ffe0);
    assert_eq!(rip.program.byte_len, 0x2_ffe0);
    let mut before_eof = bytes.clone();
    before_eof[0x1_ffe0..0x1_ffe4].copy_from_slice(b"WSRF");
    before_eof[0x2_ffe0..0x2_ffe4].fill(0);
    assert_eq!(status(&before_eof), ScanStatus::Unsupported);
}

#[test]
fn wsr_budget_cancellation_media_limit_and_verify_fail_closed() {
    let bytes = crate::test_support::rips::wsr_fixture();
    let full = scan(
        &bytes,
        RipFormat::Wsr,
        ScanLimits::default(),
        &AtomicBool::new(false),
    );
    assert_eq!(full.status, ScanStatus::Complete);
    for max_work in 0..full.work_used {
        let report = scan(
            &bytes,
            RipFormat::Wsr,
            ScanLimits {
                max_work,
                max_candidates: 1,
            },
            &AtomicBool::new(false),
        );
        assert!(report.music_rips.is_empty());
        assert!(matches!(report.status, ScanStatus::Incomplete(_)));
    }
    let cancelled = scan(
        &bytes,
        RipFormat::Wsr,
        ScanLimits::default(),
        &AtomicBool::new(true),
    );
    assert_eq!(
        cancelled.status,
        ScanStatus::Incomplete(ScanStop::Cancelled)
    );
    assert_eq!(
        cancelled.detector_outcomes[0].state,
        DetectorState::NotRun(ScanStop::Cancelled)
    );
    assert_eq!(
        inspect(
            &vec![0; MAX_ROM_BYTES + 1],
            RipFormat::Wsr,
            ScanLimits::default(),
            &AtomicBool::new(false)
        ),
        Err(ScanStop::MediaLimit)
    );
    let rip = read(&bytes);
    verify(&bytes, &rip, &AtomicBool::new(false)).unwrap();
    let mut changed = bytes;
    changed[0xffe5] ^= 1;
    assert!(verify(&changed, &rip, &AtomicBool::new(false)).is_err());
}
