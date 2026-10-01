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
        RipFormat::Hes,
        ScanLimits::default(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .expect("valid HES fixture")
}

fn source(load: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 0x20];
    bytes[..8].copy_from_slice(b"HESM\0\xff\0\x40");
    bytes[8..16].copy_from_slice(&[0xff, 0xf8, 0, 0, 0, 0, 0, 0]);
    bytes[16..20].copy_from_slice(b"DATA");
    bytes[20..24].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes[24..28].copy_from_slice(&load.to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn status(bytes: &[u8]) -> ScanStatus {
    scan(
        bytes,
        RipFormat::Hes,
        ScanLimits::default(),
        &AtomicBool::new(false),
    )
    .status
}

#[test]
fn hes_preserves_raw_declarations_and_exact_spans() {
    let bytes = crate::test_support::rips::hes_fixture();
    let rip = read(&bytes);
    assert_eq!(rip.format, RipFormat::Hes);
    assert_eq!(rip.version, Some(0));
    assert_eq!(
        (rip.song_count, rip.first_song, rip.load_address, rip.play),
        (None, None, None, None)
    );
    assert!(rip.title.is_empty() && rip.author.is_empty() && rip.copyright.is_empty());
    assert_eq!(rip.sha256, zeff_firmware::sha256_hex(&bytes));
    assert_eq!(
        rip.source,
        FileSpan {
            offset: 0,
            byte_len: 33
        }
    );
    assert_eq!(
        rip.header,
        FileSpan {
            offset: 0,
            byte_len: 32
        }
    );
    assert_eq!(
        rip.program,
        FileSpan {
            offset: 32,
            byte_len: 1
        }
    );
    assert_eq!(
        rip.init,
        Some(EntryPoint {
            cpu_address: 0x4000,
            initial_source_offset: None,
        })
    );
    let RipDetails::Hes {
        raw_start_song,
        initial_mprs,
        data_header,
        physical_load_address,
        reserved,
    } = rip.details
    else {
        panic!("expected HES details");
    };
    assert_eq!(raw_start_song, 0xff);
    assert_eq!(initial_mprs, [0xff, 0xf8, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        data_header,
        FileSpan {
            offset: 16,
            byte_len: 16
        }
    );
    assert_eq!(physical_load_address, 0);
    assert_eq!(reserved, [0; 4]);
    let json = serde_json::to_value(read(&bytes)).unwrap();
    for field in ["song_count", "first_song", "load_address", "play"] {
        assert_eq!(json[field], serde_json::Value::Null);
    }
    assert!(json["init"].is_object());
    assert_eq!(json["details"]["raw_start_song"], 255);
    let selected_rip = read(&bytes);
    let selected = SongRef::Rip(&selected_rip);
    assert!(selected.supports(SongFormat::Hes));
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
fn hes_accepts_the_last_rom_byte_and_refuses_non_rom_ranges() {
    let highest = read(&source(0xfffff, &[0xea]));
    assert!(matches!(
        highest.details,
        RipDetails::Hes {
            physical_load_address: 0xfffff,
            ..
        }
    ));
    for bytes in [
        source(0xfffff, &[0xea, 0xea]),
        source(0x1f0000, &[0xea]),
        source(u32::MAX, &[0xea]),
    ] {
        assert_eq!(status(&bytes), ScanStatus::Unsupported);
    }
}

#[test]
fn hes_distinguishes_truncation_from_unsupported_subset_variants() {
    let valid = crate::test_support::rips::hes_fixture();
    for len in 4..0x20 {
        assert_eq!(
            status(&valid[..len]),
            ScanStatus::Malformed(MalformedInput::TruncatedHeader)
        );
    }
    let mut payload_truncated = source(0, &[0xea]);
    payload_truncated[20..24].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        status(&payload_truncated),
        ScanStatus::Malformed(MalformedInput::ProgramLengthExceedsSource)
    );
    let mut unknown_version = valid.clone();
    unknown_version[4] = 1;
    unknown_version[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut unknown_tag = valid.clone();
    unknown_tag[16..20].copy_from_slice(b"JUNK");
    unknown_tag[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
    let empty = source(0, &[]);
    let mut reserved = valid.clone();
    reserved[28] = 1;
    let mut trailing = valid.clone();
    trailing.push(0);
    for bytes in [unknown_version, unknown_tag, empty, reserved, trailing] {
        assert_eq!(status(&bytes), ScanStatus::Unsupported);
    }
}

#[test]
fn hes_budget_cancellation_media_limit_and_verify_fail_closed() {
    let bytes = crate::test_support::rips::hes_fixture();
    let full = scan(
        &bytes,
        RipFormat::Hes,
        ScanLimits::default(),
        &AtomicBool::new(false),
    );
    assert_eq!(full.status, ScanStatus::Complete);
    for max_work in 0..full.work_used {
        let report = scan(
            &bytes,
            RipFormat::Hes,
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
        RipFormat::Hes,
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
            RipFormat::Hes,
            ScanLimits::default(),
            &AtomicBool::new(false)
        ),
        Err(ScanStop::MediaLimit)
    );
    let rip = read(&bytes);
    verify(&bytes, &rip, &AtomicBool::new(false)).unwrap();
    let mut changed = bytes.clone();
    *changed.last_mut().expect("fixture payload") ^= 1;
    assert!(verify(&changed, &rip, &AtomicBool::new(false)).is_err());
    let mut forged = rip;
    forged.program.byte_len = 0;
    assert!(verify(&bytes, &forged, &AtomicBool::new(false)).is_err());
}
