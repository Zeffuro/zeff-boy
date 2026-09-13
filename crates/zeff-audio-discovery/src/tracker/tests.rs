use std::sync::atomic::AtomicBool;

use super::*;
use crate::test_support::tracker::{it_fixture, mod_fixture, s3m_fixture, xm_fixture};

fn detect(bytes: &[u8]) -> Vec<EmbeddedModule> {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 5_000_000,
    };
    let mut output = Vec::new();
    scan(bytes, &mut output, &mut budget, 16).unwrap();
    output
}

#[test]
fn identifies_complete_embedded_modules_at_unaligned_offsets() {
    let xm = xm_fixture();
    let mod_file = mod_fixture();
    let mut source = vec![0xff; 73];
    source.extend_from_slice(&xm);
    source.extend_from_slice(&[0x23; 19]);
    let second = source.len();
    source.extend_from_slice(&mod_file);
    source.extend_from_slice(&[0xfe; 37]);
    let found = detect(&source);
    assert_eq!(found.len(), 2);
    assert_eq!(
        found[0].span,
        FileSpan {
            offset: 73,
            byte_len: xm.len() as u32
        }
    );
    assert_eq!(found[0].sample_points, 6);
    assert_eq!(found[1].span.offset, second as u32);
    assert_eq!(found[1].span.byte_len, mod_file.len() as u32);
    assert_eq!(found[1].sample_points, 8);
}

#[test]
fn identifies_s3m_and_it_with_exact_sample_extents_and_multiple_modules() {
    let s3m = s3m_fixture();
    let it = it_fixture();
    let mut source = vec![0xa5; 73];
    source.extend_from_slice(&s3m);
    let it_offset = source.len();
    source.extend_from_slice(&it);
    let found = detect(&source);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].format, EmbeddedFormat::S3m);
    assert_eq!(
        found[0].span,
        FileSpan {
            offset: 73,
            byte_len: 400
        }
    );
    assert_eq!(found[0].samples, 1);
    assert_eq!(found[0].sample_points, 4);
    assert_eq!(found[1].format, EmbeddedFormat::It);
    assert_eq!(
        found[1].span,
        FileSpan {
            offset: it_offset as u32,
            byte_len: 1316
        }
    );
    assert_eq!(found[1].instruments, 1);
    assert_eq!(found[1].samples, 1);
    assert_eq!(found[1].sample_points, 4);
}

#[test]
fn rejects_forged_or_unsupported_s3m_and_it_topologies() {
    let mut s3m = s3m_fixture();
    s3m[97..99].copy_from_slice(&1u16.to_le_bytes());
    assert!(detect(&s3m).is_empty());
    let mut s3m = s3m_fixture();
    s3m[128 + 30] = 4;
    assert!(detect(&s3m).is_empty());

    let mut it = it_fixture();
    it[193..197].copy_from_slice(&1u32.to_le_bytes());
    assert!(detect(&it).is_empty());
    let mut it = it_fixture();
    it[1218] |= 8;
    assert!(detect(&it).is_empty());
    let it = it_fixture();
    assert!(detect(&it[..it.len() - 1]).is_empty());
}

#[test]
fn s3m_and_it_reject_every_truncated_prefix_without_panicking() {
    for original in [s3m_fixture(), it_fixture()] {
        for end in 0..original.len() {
            assert!(detect(&original[..end]).is_empty(), "accepted prefix {end}");
        }
    }
}

#[test]
fn s3m_and_it_honor_work_budgets_cancellation_and_standalone_tails() {
    let cancel = AtomicBool::new(false);
    for (format, mut bytes) in [
        (EmbeddedFormat::S3m, s3m_fixture()),
        (EmbeddedFormat::It, it_fixture()),
    ] {
        bytes.extend_from_slice(&[0xd0, 0x0d]);
        let mut budget = Budget {
            cancel: &cancel,
            remaining: 5_000_000,
        };
        let module = scan_standalone(&bytes, format, &mut budget)
            .unwrap()
            .unwrap();
        assert_eq!(
            module.source,
            ModuleSource::Standalone { trailing_bytes: 2 }
        );
        #[cfg(not(target_arch = "wasm32"))]
        assert!(verify_original(&bytes, &module, &cancel).is_ok());
    }
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 2,
    };
    let mut output = Vec::new();
    assert_eq!(
        scan(&s3m_fixture(), &mut output, &mut budget, 1),
        Err(ScanStop::WorkLimit)
    );
    let cancel = AtomicBool::new(true);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 5_000_000,
    };
    let mut output = Vec::new();
    assert_eq!(
        scan(&it_fixture(), &mut output, &mut budget, 1),
        Err(ScanStop::Cancelled)
    );
}

#[test]
fn rejects_truncation_false_tags_invalid_patterns_and_sample_ranges() {
    for original in [xm_fixture(), mod_fixture()] {
        for end in [0, 16, 80, 336, original.len() - 1] {
            assert!(detect(&original[..end.min(original.len())]).is_empty());
        }
    }
    let mut bytes = xm_fixture();
    bytes[349] = 0x9f;
    assert!(detect(&bytes).is_empty());
    let mut bytes = xm_fixture();
    bytes[72..74].copy_from_slice(&0u16.to_le_bytes());
    assert!(detect(&bytes).is_empty());
    let mut bytes = mod_fixture();
    bytes[46..48].copy_from_slice(&3u16.to_be_bytes());
    bytes[48..50].copy_from_slice(&4u16.to_be_bytes());
    assert!(detect(&bytes).is_empty());
    let mut bytes = vec![0; 4096];
    bytes[1080..1084].copy_from_slice(b"M.K.");
    assert!(detect(&bytes).is_empty());
}

#[test]
fn keeps_completed_modules_when_candidate_limit_is_reached() {
    let first = xm_fixture();
    let mut source = first.clone();
    source.extend_from_slice(&first);
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 5000,
    };
    let mut output = Vec::new();
    assert_eq!(
        scan(&source, &mut output, &mut budget, 1),
        Err(ScanStop::CandidateLimit)
    );
    assert_eq!(output.len(), 1);
    assert_eq!(output[0].span.byte_len, first.len() as u32);
    let cancel = AtomicBool::new(true);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 5000,
    };
    let mut output = Vec::new();
    assert_eq!(
        scan(&source, &mut output, &mut budget, 10),
        Err(ScanStop::Cancelled)
    );
    assert!(output.is_empty());
}

#[test]
fn all_registered_systems_find_each_embedded_module_without_cpu_addresses() {
    for (format, module) in [
        (EmbeddedFormat::Xm, xm_fixture()),
        (EmbeddedFormat::Mod, mod_fixture()),
        (EmbeddedFormat::S3m, s3m_fixture()),
        (EmbeddedFormat::It, it_fixture()),
    ] {
        let mut source = vec![0; 517];
        source.extend_from_slice(&module);
        source.extend_from_slice(&[0; 97]);
        for spec in zeff_emu_common::system::System::specs() {
            let report = super::super::scan(
                spec.system,
                &source,
                Default::default(),
                &AtomicBool::new(false),
            );
            assert_eq!(report.status, super::super::ScanStatus::Complete);
            assert_eq!(report.song_count(), 1, "{:?} {format:?}", spec.system);
            let song = report
                .song(super::super::catalog::SongId::Module(0))
                .unwrap();
            let span = song.span().unwrap();
            assert_eq!(span.canonical_cpu_address, None);
            assert_eq!(span.effective_offset, 517);
            let found = &report.tracker_modules[0];
            assert_eq!(found.format, format);
            assert_eq!(found.span.byte_len as usize, module.len());
            assert_eq!(found.source, ModuleSource::Embedded);
            #[cfg(not(target_arch = "wasm32"))]
            verify_original(&source, found, &AtomicBool::new(false)).unwrap();
        }
    }
}

#[test]
fn standalone_validation_requires_offset_zero_honors_candidate_limit_and_checks_tails() {
    let mut bytes = xm_fixture();
    bytes.extend_from_slice(&[0xD0, 0x0D]);
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 5_000_000,
    };
    let module = scan_standalone(&bytes, EmbeddedFormat::Xm, &mut budget)
        .unwrap()
        .unwrap();
    assert_eq!(
        module.source,
        ModuleSource::Standalone { trailing_bytes: 2 }
    );
    #[cfg(not(target_arch = "wasm32"))]
    {
        assert!(verify_original(&bytes, &module, &cancel).is_ok());
        let mut extra_tail = bytes.clone();
        extra_tail.push(0);
        assert!(verify_original(&extra_tail, &module, &cancel).is_err());
    }

    let limited = super::super::scan_standalone_tracker(
        &bytes,
        EmbeddedFormat::Xm,
        super::super::ScanLimits {
            max_work: 5_000_000,
            max_candidates: 0,
        },
        &cancel,
    );
    assert_eq!(
        limited.status,
        super::super::ScanStatus::Incomplete(ScanStop::CandidateLimit)
    );
    assert!(limited.tracker_modules.is_empty());

    let mut prefixed = vec![0; 1];
    prefixed.extend_from_slice(&bytes);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 5_000_000,
    };
    assert!(
        scan_standalone(&prefixed, EmbeddedFormat::Xm, &mut budget)
            .unwrap()
            .is_none()
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn projected_fixture(sixteen_bit: bool) -> xm::Module {
    let sample = xm::Sample {
        name: "Delta".to_owned(),
        pcm: if sixteen_bit {
            vec![0, 32767, -32768, -1, 1203, -1701]
        } else {
            vec![0, 32512, -32768, -256, 256, 0]
        },
        sixteen_bit,
        loop_range: Some((1, 5)),
        ping_pong: true,
        volume: 64,
        panning: 96,
        relative_note: -3,
        finetune: 37,
    };
    xm::Module {
        name: "Delta fixture".to_owned(),
        channels: 2,
        orders: vec![0],
        restart: 0,
        speed: 6,
        bpm: 125,
        linear_frequency: true,
        patterns: vec![xm::Pattern {
            rows: 2,
            cells: vec![
                xm::Cell {
                    note: 49,
                    instrument: 1,
                    volume: 0x50,
                    ..Default::default()
                },
                xm::Cell::default(),
                xm::Cell {
                    note: 97,
                    ..Default::default()
                },
                xm::Cell::default(),
            ],
        }],
        instruments: vec![xm::Instrument {
            name: "Instrument".to_owned(),
            samples: vec![sample],
            volume_envelope: xm::Envelope {
                points: vec![(0, 64), (5, 32), (10, 0)],
                sustain: Some(1),
                loop_range: Some((0, 1)),
            },
            panning_envelope: xm::Envelope {
                points: vec![(0, 32), (7, 64)],
                sustain: None,
                loop_range: None,
            },
            vibrato: [1, 2, 3, 4],
            fadeout: 512,
            ..Default::default()
        }],
    }
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn writes_standard_headers_envelopes_and_wrapping_delta_samples() {
    for sixteen_bit in [false, true] {
        let module = projected_fixture(sixteen_bit);
        let bytes = xm::encode(&module, &AtomicBool::new(false)).unwrap();
        let found = detect(&bytes);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].sample_points, 6);
        let pattern_len = u16::from_le_bytes(bytes[343..345].try_into().unwrap()) as usize;
        let instrument = 345 + pattern_len;
        assert_eq!(
            u32::from_le_bytes(bytes[instrument..instrument + 4].try_into().unwrap()),
            263
        );
        assert_eq!(
            &bytes[instrument + 225..instrument + 235],
            &[3, 2, 1, 0, 1, 0, 0, 0, 7, 1]
        );
        let sample_header = instrument + 263;
        let stride = if sixteen_bit { 2 } else { 1 };
        assert_eq!(
            u32::from_le_bytes(
                bytes[sample_header + 4..sample_header + 8]
                    .try_into()
                    .unwrap()
            ),
            stride
        );
        assert_eq!(
            u32::from_le_bytes(
                bytes[sample_header + 8..sample_header + 12]
                    .try_into()
                    .unwrap()
            ),
            4 * stride
        );
        assert_eq!(
            bytes[sample_header + 14],
            if sixteen_bit { 0x12 } else { 2 }
        );
        let encoded = &bytes[sample_header + 40..];
        let mut previous = 0i16;
        let decoded: Vec<i16> = if sixteen_bit {
            encoded
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| {
                    previous = previous.wrapping_add(i16::from_le_bytes(*pair));
                    previous
                })
                .collect()
        } else {
            encoded
                .iter()
                .map(|&delta| {
                    let value = ((previous >> 8) as i8).wrapping_add(delta as i8);
                    previous = i16::from(value) << 8;
                    previous
                })
                .collect()
        };
        assert_eq!(decoded, module.instruments[0].samples[0].pcm);
        assert_eq!(bytes, xm::encode(&module, &AtomicBool::new(false)).unwrap());
    }
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn rejects_lossy_bit_depth_and_invalid_envelopes_before_writing() {
    let mut module = projected_fixture(false);
    module.instruments[0].samples[0].pcm[0] = 1;
    assert!(xm::encode(&module, &AtomicBool::new(false)).is_err());
    let mut module = projected_fixture(true);
    module.instruments[0].volume_envelope.sustain = Some(3);
    assert!(xm::encode(&module, &AtomicBool::new(false)).is_err());
    assert!(xm::encode(&projected_fixture(true), &AtomicBool::new(true)).is_err());
}

#[test]
fn accepts_legacy_unused_fields_and_implicit_empty_order_patterns() {
    let mut bytes = xm_fixture();
    bytes[80] = 7;
    let instrument = 353;
    bytes[instrument + 26] = 0xff;
    let sample = instrument + 263;
    bytes[sample + 14] = 1;
    assert_eq!(detect(&bytes).len(), 1);
    let mut bytes = mod_fixture();
    bytes[951] = 0x78;
    assert_eq!(detect(&bytes).len(), 1);
}
