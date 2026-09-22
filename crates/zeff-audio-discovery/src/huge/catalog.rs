use serde::Serialize;

use super::{discovery::BoundSong, isolation};
use crate::{Budget, ScanStop};

pub const MAX_VALIDATION_FRAMES: u32 = 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HugeSong {
    pub source_sha256: String,
    pub bound: BoundSong,
    pub validation_frames: u32,
}

pub fn control_period_frames(loop_frames: u32) -> Option<u32> {
    if loop_frames == 0 {
        return None;
    }
    let divisor = gcd(loop_frames, 256);
    loop_frames.checked_div(divisor)?.checked_mul(256)
}

pub fn required_validation_frames(loop_frames: u32) -> Option<u32> {
    loop_frames
        .checked_add(control_period_frames(loop_frames)?.checked_mul(2)?)
        .and_then(|frames| frames.checked_add(1))
}

pub(crate) fn scan(
    bytes: &[u8],
    source_sha256: &str,
    songs: &mut Vec<HugeSong>,
    budget: &mut Budget<'_>,
    capacity: usize,
) -> Result<(), ScanStop> {
    let report = super::discovery::discover_with_budget(bytes, budget, capacity)?;
    for bound in report.bound {
        let Some(validation_frames) = required_validation_frames(bound.song.loop_ticks) else {
            continue;
        };
        let bootstrap_matches = isolation::bootstrap_matches(bytes, &bound, budget)?;
        if validation_frames > MAX_VALIDATION_FRAMES || !bootstrap_matches {
            continue;
        }
        songs.push(HugeSong {
            source_sha256: source_sha256.to_owned(),
            bound,
            validation_frames,
        });
    }
    Ok(())
}

fn gcd(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use zeff_emu_common::system::System;

    #[test]
    fn recurrence_budget_is_exact_and_bounded() {
        assert_eq!(control_period_frames(128), Some(256));
        assert_eq!(required_validation_frames(128), Some(641));
        assert_eq!(required_validation_frames(256), Some(769));
        assert_eq!(required_validation_frames(257), Some(131_842));
        assert_eq!(control_period_frames(0), None);
        assert_eq!(required_validation_frames(0), None);
    }

    #[test]
    fn catalog_requires_the_generated_bootstrap_and_source_identity() {
        let bytes = crate::huge::discovery::tests::fixture(0x1800, 0xc000);
        let bound =
            super::super::discovery::discover(&bytes, Default::default(), &AtomicBool::new(false))
                .unwrap()
                .bound
                .remove(0);
        let source_cancel = AtomicBool::new(false);
        let mut source_budget = Budget {
            cancel: &source_cancel,
            remaining: crate::MAX_SCAN_WORK,
        };
        let mut source_songs = Vec::new();
        let source_hash = const_hex::encode(zeff_firmware::sha256_bytes(&bytes));
        scan(
            &bytes,
            &source_hash,
            &mut source_songs,
            &mut source_budget,
            16,
        )
        .unwrap();
        assert!(source_songs.is_empty());
        let isolated =
            isolation::build_from_bound(&bytes, &bound, 0, &AtomicBool::new(false)).unwrap();
        let cancel = AtomicBool::new(false);
        let mut budget = Budget {
            cancel: &cancel,
            remaining: crate::MAX_SCAN_WORK,
        };
        let mut songs = Vec::new();
        let isolated_hash = const_hex::encode(zeff_firmware::sha256_bytes(&isolated.bytes));
        scan(&isolated.bytes, &isolated_hash, &mut songs, &mut budget, 16).unwrap();
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].validation_frames, 641);
        assert_eq!(
            songs[0].source_sha256,
            const_hex::encode(zeff_firmware::sha256_bytes(&isolated.bytes))
        );
        let mut stale = isolated.bytes.clone();
        stale[0x7000] ^= 1;
        assert_ne!(
            songs[0].source_sha256,
            const_hex::encode(zeff_firmware::sha256_bytes(&stale))
        );
    }

    #[test]
    fn scanner_catalogues_pending_audio_and_gbs_selection_and_preserves_limits() {
        let source = crate::huge::discovery::tests::fixture(0x1800, 0xc000);
        let bound =
            super::super::discovery::discover(&source, Default::default(), &AtomicBool::new(false))
                .unwrap()
                .bound
                .remove(0);
        let bytes = isolation::build_from_bound(&source, &bound, 0, &AtomicBool::new(false))
            .unwrap()
            .bytes;
        let report = crate::scan(
            System::Gb,
            &bytes,
            Default::default(),
            &AtomicBool::new(false),
        );
        assert_eq!(report.huge_songs.len(), 1);
        let song = report.song(crate::catalog::SongId::Huge(0)).unwrap();
        assert!(song.requires_runtime_validation());
        assert!(
            crate::formats::AudioFormat::ALL
                .into_iter()
                .all(|format| { song.supports(crate::formats::SongFormat::Audio(format)) })
        );
        assert!(!song.supports(crate::formats::SongFormat::MappedAssets));
        assert!(!song.supports(crate::formats::SongFormat::Midi));
        assert!(song.supports(crate::formats::SongFormat::Gbs));
        let finding = report
            .catalog()
            .find(|item| item.id == crate::catalog::SongId::Huge(0))
            .unwrap();
        assert!(finding.runtime_validation_required);
        let coverage = crate::coverage::observe(&report);
        assert_eq!(coverage.stage, crate::coverage::CoverageStage::Catalogued);
        assert_eq!(coverage.pending_runtime_entries, 1);
        assert_eq!(coverage.render_supported_entries, 0);
        let blocker = crate::coverage::CoverageBlocker::CatalogRuntimeValidationRequired;
        assert!(coverage.blockers.contains(&blocker));
        assert_eq!(blocker.as_str(), "catalog_runtime_validation_required");
        assert!(
            !coverage
                .blockers
                .contains(&crate::coverage::CoverageBlocker::CatalogRenderUnsupported)
        );

        let cancel = AtomicBool::new(false);
        let mut budget = Budget {
            cancel: &cancel,
            remaining: crate::MAX_SCAN_WORK,
        };
        assert_eq!(
            scan(
                &bytes,
                &const_hex::encode(zeff_firmware::sha256_bytes(&bytes)),
                &mut Vec::new(),
                &mut budget,
                1,
            ),
            Err(ScanStop::CandidateLimit)
        );
        let cancelled = AtomicBool::new(true);
        let mut budget = Budget {
            cancel: &cancelled,
            remaining: crate::MAX_SCAN_WORK,
        };
        assert_eq!(
            scan(
                &bytes,
                &const_hex::encode(zeff_firmware::sha256_bytes(&bytes)),
                &mut Vec::new(),
                &mut budget,
                16,
            ),
            Err(ScanStop::Cancelled)
        );
    }
}
