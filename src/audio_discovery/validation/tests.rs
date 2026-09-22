use super::*;

struct Stream {
    samples: Vec<i16>,
    position: usize,
    bad_reset: bool,
    early_end: bool,
}

impl PcmSession for Stream {
    fn duration_frames(&self) -> usize {
        self.samples.len() / 2
    }
    fn position_frames(&self) -> usize {
        self.position / 2
    }
    fn sample_rate(&self) -> u32 {
        44_100
    }
    fn track_count(&self) -> usize {
        1
    }
    fn warnings(&self) -> &[String] {
        &[]
    }
    fn reset(&mut self) -> Result<()> {
        self.position = 0;
        if self.bad_reset {
            self.samples[0] ^= 1;
        }
        Ok(())
    }
    fn set_track_mask(&mut self, _: u16) -> Result<()> {
        Ok(())
    }
    fn read(&mut self, output: &mut [i16], _: &AtomicBool) -> Result<usize> {
        if self.early_end {
            return Ok(0);
        }
        let count = output.len().min(self.samples.len() - self.position);
        output[..count].copy_from_slice(&self.samples[self.position..self.position + count]);
        self.position += count;
        Ok(count)
    }
}

fn stream(samples: Vec<i16>) -> Box<dyn PcmSession> {
    Box::new(Stream {
        samples,
        position: 0,
        bad_reset: false,
        early_end: false,
    })
}

#[test]
fn metrics_are_exact_and_chunk_independent() {
    let pcm = vec![0, 0, 8, -8, 32767, -32768, 0, 9];
    let evidence = validate(|| Ok(stream(pcm.clone())), 4, &AtomicBool::new(false)).unwrap();
    assert!(evidence.deterministic());
    assert!(!evidence.silent);
    assert_eq!(evidence.pcm.frames, 4);
    assert_eq!(evidence.pcm.nonzero_frames, 3);
    assert_eq!(evidence.pcm.active_frames, 2);
    assert_eq!(evidence.pcm.clipped_samples, 2);
    assert_eq!(evidence.pcm.peak, 1.0);
    let bytes: Vec<_> = pcm.iter().flat_map(|sample| sample.to_le_bytes()).collect();
    assert_eq!(evidence.pcm.pcm_sha256, zeff_firmware::sha256_hex(&bytes));
    assert_eq!(
        evidence.pcm.activity_intervals,
        vec![Interval {
            start_frame: 2,
            end_frame: 4
        }]
    );
}

#[test]
fn activity_gaps_and_caps_are_explicit() {
    let mut metrics = Metrics::new(4);
    metrics.push(&[9, 0, 0, 0, 0, 0, 0, 0, 0, 9]);
    let evidence = metrics.finish();
    assert_eq!(
        evidence.activity_intervals,
        vec![
            Interval {
                start_frame: 0,
                end_frame: 1
            },
            Interval {
                start_frame: 4,
                end_frame: 5
            }
        ]
    );
    let mut metrics = Metrics::new(4);
    for _ in 0..300 {
        metrics.push(&[9, 0, 0, 0, 0, 0, 0, 0]);
    }
    let evidence = metrics.finish();
    assert_eq!(evidence.activity_intervals.len(), MAX_ACTIVITY_INTERVALS);
    assert!(evidence.activity_intervals_truncated);
    let evidence = validate(|| Ok(stream(vec![0; 4000])), 500, &AtomicBool::new(false)).unwrap();
    assert!(evidence.validation_duration_capped && evidence.silent && evidence.deterministic());
    assert_eq!(evidence.pcm.frames, 500);
}

#[test]
fn premature_output_cancellation_and_nondeterminism_are_not_successes() {
    assert!(
        validate(
            || Ok(Box::new(Stream {
                samples: vec![0; 4],
                position: 0,
                bad_reset: false,
                early_end: true
            })),
            2,
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(validate(|| Ok(stream(vec![0; 4])), 2, &AtomicBool::new(true)).is_err());
    let evidence = validate(
        || {
            Ok(Box::new(Stream {
                samples: vec![0; 4],
                position: 0,
                bad_reset: true,
                early_end: false,
            }))
        },
        2,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(!evidence.reset_render_matches && evidence.fresh_render_matches);
    let mut calls = 0;
    let evidence = validate(
        || {
            calls += 1;
            Ok(stream(vec![calls; 4]))
        },
        2,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(!evidence.fresh_render_matches && evidence.reset_render_matches);
}

#[test]
fn shared_validation_handles_soundfont_blocks_and_cdda_caps() -> Result<()> {
    use crate::audio_discovery::{
        catalog::SongId, media::ScanInput, preview::PreviewRequest, render::RenderOptions,
        test_support,
    };
    use std::sync::Arc;
    let mut bytes = test_support::fixture();
    bytes[0x100] = 1;
    bytes[0x208..0x20c].copy_from_slice(&[255, 255, 255, 128]);
    test_support::put_word(&mut bytes, 0x304, 8000 * 1024);
    test_support::put_word(&mut bytes, 0x308, 0);
    test_support::put_word(&mut bytes, 0x30c, 64);
    for index in 0..64 {
        bytes[0x310 + index] = if index < 32 { 100 } else { 156 };
    }
    let sequence = [0xbd, 0, 0xbb, 60, 0xbe, 127, 0xff, 60, 127, 0xa0, 0xb1];
    bytes[0x400..0x400 + sequence.len()].copy_from_slice(&sequence);
    let source = Arc::new(ScanInput {
        cdda: None,
        system: Some(zeff_emu_common::system::System::Gba),
        standalone_audio: None,
        bytes: bytes.into(),
        provenance: None,
        analysis_profile: "validation-test",
        display_name: None,
    });
    let cancel = AtomicBool::new(false);
    let manifest = source.analyze(Default::default(), &cancel);
    let options = RenderOptions {
        max_seconds: 1,
        sample_rate: 44_100,
        ..RenderOptions::default()
    };
    let evidence = validate(
        || {
            Ok(Box::new(
                PreviewRequest::prepare_song(&source, &manifest, SongId::Mp2k(0), options)?
                    .renderer(options.sample_rate, &cancel)?,
            ))
        },
        44_100,
        &cancel,
    )?;
    assert!(evidence.deterministic() && !evidence.silent);
    let (source, manifest, expected) = test_support::cdda_fixture()?;
    let evidence = validate(
        || {
            Ok(Box::new(
                PreviewRequest::prepare_song(&source, &manifest, SongId::Cdda(0), options)?
                    .renderer(options.sample_rate, &cancel)?,
            ))
        },
        1031,
        &cancel,
    )?;
    assert!(evidence.deterministic() && evidence.validation_duration_capped);
    let bytes: Vec<_> = expected[..2062]
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect();
    assert_eq!(evidence.pcm.pcm_sha256, zeff_firmware::sha256_hex(&bytes));
    Ok(())
}
