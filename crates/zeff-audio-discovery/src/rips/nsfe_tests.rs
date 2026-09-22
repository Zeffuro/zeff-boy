use std::sync::atomic::AtomicBool;

use super::*;
use crate::{
    DetectorState, ScanStatus,
    catalog::SongRef,
    formats::{AudioFormat, SongFormat},
    native_rips,
};
use zeff_emu_common::system::System;

fn read(bytes: &[u8]) -> MusicRip {
    inspect(
        bytes,
        RipFormat::Nsfe,
        ScanLimits::default(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap()
}

fn chunk(bytes: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend(id);
    bytes.extend(payload);
}

fn container(parts: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let mut bytes = b"NSFE".to_vec();
    for (id, payload) in parts {
        chunk(&mut bytes, id, payload);
    }
    bytes
}

#[test]
fn nsfe_keeps_chunk_order_and_raw_declarations() {
    let bytes = crate::test_support::rips::fixture(RipFormat::Nsfe);
    let rip = read(&bytes);
    assert_eq!(rip.format, RipFormat::Nsfe);
    assert_eq!(rip.version, None);
    assert_eq!((rip.song_count, rip.first_song), (3, 2));
    assert_eq!(rip.sha256, zeff_firmware::sha256_hex(&bytes));
    assert_eq!(
        rip.header,
        FileSpan {
            offset: 0,
            byte_len: 4
        }
    );
    assert_eq!(
        rip.program,
        FileSpan {
            offset: 50,
            byte_len: 0x50
        }
    );
    assert_eq!(rip.init.initial_source_offset, None);
    assert_eq!(rip.play.initial_source_offset, None);
    assert_eq!(rip.title, "");
    assert_eq!(rip.author, "");
    assert_eq!(rip.copyright, "");
    let selected = SongRef::Rip(&rip);
    assert!(selected.supports(SongFormat::Nsfe));
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
    let RipDetails::Nsfe {
        chunks,
        info_header,
        data_header,
        raw_start_song,
        info_region_bits,
        info_expansion_bits,
        ntsc_period_us,
        pal_period_us,
        dendy_period_us,
        bank_payload,
        initial_banks,
        banking_enabled,
    } = rip.details
    else {
        panic!("expected NSFe details");
    };
    assert_eq!(
        chunks.iter().map(|chunk| chunk.id).collect::<Vec<_>>(),
        [*b"INFO", *b"RATE", *b"BANK", *b"DATA", *b"tlbl", *b"NEND"]
    );
    assert_eq!(
        info_header,
        FileSpan {
            offset: 4,
            byte_len: 8
        }
    );
    assert_eq!(
        data_header,
        FileSpan {
            offset: 42,
            byte_len: 8
        }
    );
    assert_eq!(chunks[3].payload, rip.program);
    assert_eq!(raw_start_song, 1);
    assert_eq!((info_region_bits, info_expansion_bits), (0, 0));
    assert_eq!(
        (ntsc_period_us, pal_period_us, dendy_period_us),
        (Some(16639), Some(19997), None)
    );
    assert_eq!(
        bank_payload,
        Some(FileSpan {
            offset: 42,
            byte_len: 0
        })
    );
    assert_eq!(initial_banks, [0; 8]);
    assert!(banking_enabled);
}

#[test]
fn nsfe_required_chunks_and_chunk_bounds_fail_closed() {
    let info = [0, 0x80, 0, 0x80, 1, 0x80, 0, 0, 3, 1];
    let data = [0x60];
    let cases = [
        (b"NSFE".to_vec(), MalformedInput::MissingRequiredChunk),
        (
            container(&[(b"INFO", &info), (b"DATA", &data)]),
            MalformedInput::MissingRequiredChunk,
        ),
        (
            container(&[(b"DATA", &data), (b"INFO", &info), (b"NEND", &[])]),
            MalformedInput::InvalidChunkOrder,
        ),
        (
            container(&[
                (b"INFO", &info),
                (b"INFO", &info),
                (b"DATA", &data),
                (b"NEND", &[]),
            ]),
            MalformedInput::DuplicateChunk,
        ),
        (
            container(&[
                (b"INFO", &info),
                (b"DATA", &data),
                (b"DATA", &data),
                (b"NEND", &[]),
            ]),
            MalformedInput::DuplicateChunk,
        ),
    ];
    for (bytes, malformed) in cases {
        let report = scan(
            &bytes,
            RipFormat::Nsfe,
            ScanLimits::default(),
            &AtomicBool::new(false),
        );
        assert_eq!(report.status, ScanStatus::Malformed(malformed));
        assert_eq!(
            report.detector_outcomes[0].state,
            DetectorState::Malformed(malformed)
        );
    }
    for bytes in [
        b"NSFE\x01".to_vec(),
        b"NSFE\x02\0\0\0DATA\x60".to_vec(),
        b"NSFE\xff\xff\xff\xffDATA".to_vec(),
    ] {
        assert_eq!(
            scan(
                &bytes,
                RipFormat::Nsfe,
                ScanLimits::default(),
                &AtomicBool::new(false),
            )
            .status,
            ScanStatus::Malformed(MalformedInput::TruncatedChunk)
        );
    }
}

#[test]
fn nsfe_info_rate_and_bank_validation_preserve_specified_defaults() {
    let info_nine = [0xf0, 0x8f, 0, 0x90, 1, 0x90, 3, 0x7f, 255];
    let data = [0x60];
    let bytes = container(&[
        (b"INFO", &info_nine),
        (b"BANK", &[]),
        (b"DATA", &data),
        (b"NEND", &[]),
    ]);
    let rip = read(&bytes);
    assert_eq!((rip.song_count, rip.first_song), (255, 1));
    assert_eq!(rip.load_address, 0x8ff0);
    assert_eq!(rip.init.cpu_address, 0x9000);
    assert_eq!(rip.play.cpu_address, 0x9001);
    assert!(matches!(
        rip.details,
        RipDetails::Nsfe {
            raw_start_song: 0,
            info_region_bits: 3,
            info_expansion_bits: 0x7f,
            ntsc_period_us: None,
            pal_period_us: None,
            dendy_period_us: None,
            initial_banks,
            banking_enabled: true,
            ..
        } if initial_banks == [0; 8]
    ));
    for (invalid, malformed) in [
        (
            [0xf0, 0x8f, 0, 0x90, 1, 0x90, 3, 0x7f, 0, 0],
            MalformedInput::InvalidSongCount,
        ),
        (
            [0xf0, 0x8f, 0, 0x90, 1, 0x90, 3, 0x7f, 2, 2],
            MalformedInput::InvalidFirstSong,
        ),
    ] {
        assert_eq!(
            scan(
                &container(&[(b"INFO", &invalid), (b"DATA", &data), (b"NEND", &[])]),
                RipFormat::Nsfe,
                ScanLimits::default(),
                &AtomicBool::new(false),
            )
            .status,
            ScanStatus::Malformed(malformed)
        );
    }
    let info_max_start = [0, 0x80, 0, 0x80, 1, 0x80, 0, 0, 255, 254];
    assert_eq!(
        read(&container(&[
            (b"INFO", &info_max_start),
            (b"DATA", &data),
            (b"NEND", &[])
        ]))
        .first_song,
        255
    );
    for (rate, expected) in [
        (&[1, 0][..], (Some(1), None, None)),
        (&[1, 0, 2, 0, 3, 0][..], (Some(1), Some(2), Some(3))),
    ] {
        let rip = read(&container(&[
            (b"INFO", &info_nine),
            (b"RATE", rate),
            (b"DATA", &data),
            (b"NEND", &[]),
        ]));
        assert!(matches!(
            rip.details,
            RipDetails::Nsfe {
                ntsc_period_us,
                pal_period_us,
                dendy_period_us,
                ..
            } if (ntsc_period_us, pal_period_us, dendy_period_us) == expected
        ));
    }
    for (payload, expected) in [
        (&[7, 8][..], [7, 8, 0, 0, 0, 0, 0, 0]),
        (&[1, 2, 3, 4, 5, 6, 7, 8, 9][..], [1, 2, 3, 4, 5, 6, 7, 8]),
    ] {
        let rip = read(&container(&[
            (b"INFO", &info_nine),
            (b"BANK", payload),
            (b"DATA", &data),
            (b"NEND", &[]),
        ]));
        assert!(matches!(
            rip.details,
            RipDetails::Nsfe {
                initial_banks,
                banking_enabled: true,
                ..
            } if initial_banks == expected
        ));
    }
    assert!(matches!(
        read(&container(&[
            (b"INFO", &info_nine),
            (b"DATA", &data),
            (b"NEND", &[])
        ]))
        .details,
        RipDetails::Nsfe {
            bank_payload: None,
            banking_enabled: false,
            ..
        }
    ));
    for rate in [&[1, 0, 1][..], &[0, 0][..]] {
        let bytes = container(&[
            (b"INFO", &info_nine),
            (b"RATE", rate),
            (b"DATA", &data),
            (b"NEND", &[]),
        ]);
        assert_eq!(
            scan(
                &bytes,
                RipFormat::Nsfe,
                ScanLimits::default(),
                &AtomicBool::new(false),
            )
            .status,
            ScanStatus::Unsupported
        );
    }
}

#[test]
fn nsfe_rejects_critical_and_nsf2_features_without_interpreting_optional_chunks() {
    let info = [0, 0x80, 0, 0x80, 1, 0x80, 0, 0, 1, 0];
    let data = [0x60];
    for id in [b"ABCD", b"NSF2", b"VRC7"] {
        let bytes = container(&[
            (b"INFO", &info),
            (id, &[]),
            (b"DATA", &data),
            (b"NEND", &[]),
        ]);
        assert_eq!(
            scan(
                &bytes,
                RipFormat::Nsfe,
                ScanLimits::default(),
                &AtomicBool::new(false),
            )
            .status,
            ScanStatus::Unsupported
        );
    }
    for bytes in [
        container(&[(b"INFO", &info), (b"DATA", &[]), (b"NEND", &[])]),
        container(&[
            (b"INFO", &info),
            (b"RATE", &[1, 0]),
            (b"RATE", &[1, 0]),
            (b"DATA", &data),
            (b"NEND", &[]),
        ]),
        container(&[
            (b"INFO", &info),
            (b"BANK", &[]),
            (b"BANK", &[]),
            (b"DATA", &data),
            (b"NEND", &[]),
        ]),
        container(&[(b"INFO", &info), (b"DATA", &data), (b"NEND", &[1])]),
        container(&[
            (b"INFO", &info),
            (b"DATA", &data),
            (b"NEND", &[]),
            (b"tlbl", b"trailing\0"),
        ]),
    ] {
        assert_eq!(
            scan(
                &bytes,
                RipFormat::Nsfe,
                ScanLimits::default(),
                &AtomicBool::new(false),
            )
            .status,
            ScanStatus::Unsupported
        );
    }
    let bytes = container(&[
        (b"INFO", &info),
        (b"regn", &[1, 2]),
        (b"DATA", &data),
        (b"NEND", &[]),
    ]);
    let rip = read(&bytes);
    let RipDetails::Nsfe { chunks, .. } = rip.details else {
        panic!("expected NSFe details");
    };
    assert_eq!(chunks[1].id, *b"regn");
    assert_eq!(
        chunks[1].payload,
        FileSpan {
            offset: 30,
            byte_len: 2
        }
    );
}

#[test]
fn nsfe_chunk_and_metadata_work_are_bounded_and_cancellable() {
    let info = [0, 0x80, 0, 0x80, 1, 0x80, 0, 0, 1, 0];
    let data = [0x60];
    let bytes = crate::test_support::rips::fixture(RipFormat::Nsfe);
    assert_eq!(
        scan(
            &bytes,
            RipFormat::Nsfe,
            ScanLimits {
                max_work: 9,
                max_candidates: 1,
            },
            &AtomicBool::new(false),
        )
        .status,
        ScanStatus::Incomplete(ScanStop::WorkLimit)
    );
    let mut chunk_limited = b"NSFE".to_vec();
    chunk(&mut chunk_limited, b"INFO", &info);
    for _ in 0..super::nsfe::MAX_CHUNKS {
        chunk(&mut chunk_limited, b"tlbl", &[]);
    }
    assert_eq!(
        scan(
            &chunk_limited,
            RipFormat::Nsfe,
            ScanLimits::default(),
            &AtomicBool::new(false),
        )
        .status,
        ScanStatus::Incomplete(ScanStop::InventoryLimit)
    );
    let mut chunk_exact = b"NSFE".to_vec();
    chunk(&mut chunk_exact, b"INFO", &info);
    for _ in 0..super::nsfe::MAX_CHUNKS - 3 {
        chunk(&mut chunk_exact, b"tlbl", &[]);
    }
    chunk(&mut chunk_exact, b"DATA", &data);
    chunk(&mut chunk_exact, b"NEND", &[]);
    assert_eq!(
        scan(
            &chunk_exact,
            RipFormat::Nsfe,
            ScanLimits::default(),
            &AtomicBool::new(false),
        )
        .status,
        ScanStatus::Complete
    );
    let mut long_info = info.to_vec();
    long_info.resize(257, 0);
    let long_metadata = container(&[(b"INFO", &long_info), (b"DATA", &data), (b"NEND", &[])]);
    assert_eq!(
        scan(
            &long_metadata,
            RipFormat::Nsfe,
            ScanLimits {
                max_work: 3,
                max_candidates: 1,
            },
            &AtomicBool::new(false),
        )
        .status,
        ScanStatus::Incomplete(ScanStop::WorkLimit)
    );
    assert_eq!(
        scan(
            &bytes,
            RipFormat::Nsfe,
            ScanLimits::default(),
            &AtomicBool::new(true),
        )
        .status,
        ScanStatus::Incomplete(ScanStop::Cancelled)
    );
}

#[test]
fn exported_nsfe_reimports_as_one_structural_rip() {
    let bytes = crate::nes_native::fixture_rom();
    let cancel = AtomicBool::new(false);
    let report = crate::scan(System::Nes, &bytes, ScanLimits::default(), &cancel);
    assert!(!report.nes_native_songs.is_empty());
    for song in &report.nes_native_songs {
        let exported = native_rips::encode_as(
            &bytes,
            SongRef::NesNative(song),
            native_rips::NativeRipFormat::Nsfe,
            &cancel,
        )
        .unwrap();
        let nsf = native_rips::encode(&bytes, SongRef::NesNative(song), &cancel).unwrap();
        let rip = read(&exported.bytes);
        assert_eq!(
            &exported.bytes[rip.program.offset as usize..][..rip.program.byte_len as usize],
            &nsf.bytes[0x80..]
        );
        assert_eq!(rip.first_song, 1);
        assert_eq!(rip.song_count, 1);
        assert_eq!(rip.format, RipFormat::Nsfe);
        assert!(SongRef::Rip(&rip).supports(SongFormat::Nsfe));
        let RipDetails::Nsfe {
            ntsc_period_us,
            pal_period_us,
            ..
        } = rip.details
        else {
            panic!("expected NSFe details");
        };
        assert_eq!((ntsc_period_us, pal_period_us), (Some(16639), Some(19997)));
    }
}
