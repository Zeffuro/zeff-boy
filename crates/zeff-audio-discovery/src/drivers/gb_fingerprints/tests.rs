use std::sync::atomic::AtomicBool;

use super::*;
use patterns::PatternId::*;

fn insert(bytes: &mut [u8], id: patterns::PatternId, at: usize) {
    let pattern = PATTERNS[id as usize].bytes;
    bytes[at..at + pattern.len()].copy_from_slice(pattern);
}

fn detect(bytes: &[u8]) -> Vec<DriverCandidate> {
    let mut findings = Vec::new();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    scan(bytes, &mut findings, &mut budget, 64).unwrap();
    findings
}

#[test]
fn pinned_patterns_have_stable_indices_and_bounded_rules() {
    for (index, pattern) in PATTERNS.iter().enumerate() {
        assert_eq!(pattern.id as usize, index);
        assert!((2..=128).contains(&pattern.bytes.len()));
        assert!(
            PATTERNS[..index]
                .iter()
                .all(|other| other.name != pattern.name)
        );
    }
    for group in FAMILIES {
        assert!(!group.rules.is_empty());
        for rule in group.rules {
            assert!(!rule.required.is_empty());
            for &id in rule.required {
                assert!((id as usize) < PATTERNS.len());
            }
        }
    }
}

#[test]
fn every_rule_agrees_with_an_unoptimized_matcher() {
    for group in FAMILIES {
        for rule in group.rules {
            let mut bytes = vec![0; 4096];
            for (index, &id) in rule.required.iter().enumerate() {
                insert(
                    &mut bytes,
                    id,
                    PATTERNS[id as usize].at.unwrap_or(511 + index * 193),
                );
            }
            let expected: Vec<_> = FAMILIES
                .iter()
                .filter_map(|group| {
                    group
                        .rules
                        .iter()
                        .find(|rule| {
                            rule.required.iter().all(|&id| {
                                let pattern = &PATTERNS[id as usize];
                                match pattern.at {
                                    Some(at) => {
                                        bytes.get(at..at + pattern.bytes.len())
                                            == Some(pattern.bytes)
                                    }
                                    None => bytes
                                        .windows(pattern.bytes.len())
                                        .any(|w| w == pattern.bytes),
                                }
                            })
                        })
                        .map(|rule| (rule.family, rule.variant))
                })
                .collect();
            let actual = detect(&bytes);
            assert!(!actual.is_empty(), "{} {}", rule.family, rule.variant);
            assert_eq!(
                actual
                    .iter()
                    .map(|c| (c.family, c.variant))
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }
}

#[test]
fn every_signature_is_found_at_its_boundary_and_evidence_is_exact() {
    for pattern in PATTERNS {
        let mut bytes = vec![0; 4096];
        let at = pattern.at.unwrap_or(bytes.len() - pattern.bytes.len());
        insert(&mut bytes, pattern.id, at);
        for candidate in detect(&bytes) {
            assert_eq!(
                candidate.qualification,
                FingerprintQualification::FingerprintOnly
            );
            assert_eq!(candidate.fingerprint_revision, REVISION);
            for witness in candidate.evidence {
                let expected = PATTERNS
                    .iter()
                    .find(|p| p.name == witness.signature)
                    .unwrap();
                let start = witness.span.offset as usize;
                let end = start + witness.span.byte_len as usize;
                assert_eq!(&bytes[start..end], expected.bytes);
                assert_eq!(witness.sha256, zeff_firmware::sha256_hex(expected.bytes));
            }
        }
    }
    let mut bytes = vec![0; 512];
    insert(&mut bytes, GhxAudio, 496);
    assert_eq!(detect(&bytes)[0].evidence[0].span.offset, 496);
    bytes.pop();
    assert!(detect(&bytes).is_empty());
}

#[test]
fn conjunctions_require_both_witnesses_and_priority_is_order_independent() {
    let mut bytes = vec![0; 4096];
    insert(&mut bytes, BlackBoxOne, 400);
    assert!(detect(&bytes).is_empty());
    insert(&mut bytes, BlackBoxTwo, 600);
    let findings = detect(&bytes);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].evidence.len(), 2);
    insert(&mut bytes, HugeGetNotePoly, 900);
    assert_eq!(detect(&bytes)[0].variant, "SuperDisk");
    insert(&mut bytes, HugeCoffeeBatShift, 800);
    assert_eq!(detect(&bytes)[0].variant, "Coffee Bat");
    insert(&mut bytes, HugeVolSlideV1, 1200);
    assert_eq!(detect(&bytes)[0].variant, "SuperDisk");
}

#[test]
fn fixed_headers_and_first_occurrence_are_preserved() {
    let mut bytes = vec![0; 2048];
    insert(&mut bytes, Deflemask, 800);
    insert(&mut bytes, LsdpackTitle, 900);
    assert!(detect(&bytes).is_empty());
    insert(&mut bytes, Deflemask, 1);
    insert(&mut bytes, LsdpackTitle, 0x134);
    insert(&mut bytes, GhxAudio, 1000);
    insert(&mut bytes, GhxAudio, 400);
    let findings = detect(&bytes);
    assert_eq!(findings.len(), 3);
    assert_eq!(findings[0].evidence[0].span.offset, 400);
    assert_eq!(findings[1].evidence[0].span.offset, 0x134);
    assert_eq!(findings[2].evidence[0].span.offset, 1);
}

#[test]
fn interrupted_scans_are_bounded_and_never_claim_completion() {
    let mut bytes = vec![0; 4096];
    insert(&mut bytes, GhxAudio, 400);
    insert(&mut bytes, MusyxSoundtool, 800);
    for (work, capacity, cancelled, expected) in [
        (0, 64, false, ScanStop::WorkLimit),
        (60, 64, false, ScanStop::WorkLimit),
        (100_000, 64, true, ScanStop::Cancelled),
        (100_000, 0, false, ScanStop::CandidateLimit),
        (100_000, 1, false, ScanStop::CandidateLimit),
    ] {
        let cancel = AtomicBool::new(cancelled);
        let mut budget = Budget {
            cancel: &cancel,
            remaining: work,
        };
        let mut findings = Vec::new();
        assert_eq!(
            scan(&bytes, &mut findings, &mut budget, capacity),
            Err(expected)
        );
        assert!(findings.len() <= capacity);
        assert!(budget.remaining <= work);
    }
}

#[test]
fn fingerprint_only_rom_has_no_song_or_playback_selection() {
    let mut bytes = vec![0; 32768];
    insert(&mut bytes, GhxAudio, 1000);
    let cancel = AtomicBool::new(false);
    let report = crate::scan(
        zeff_emu_common::system::System::Gb,
        &bytes,
        crate::ScanLimits::default(),
        &cancel,
    );
    assert_eq!(report.status, crate::ScanStatus::Complete);
    assert_eq!(report.driver_candidates.len(), 1);
    assert_eq!(report.song_count(), 0);
    assert!(report.song_ids().next().is_none());
    let outcome = report
        .detector_outcomes
        .iter()
        .find(|outcome| outcome.descriptor.id == "gb-driver-fingerprints")
        .unwrap();
    assert_eq!(outcome.descriptor.id, "gb-driver-fingerprints");
    assert_eq!(outcome.retained_matches, 1);
    assert_eq!(
        report
            .detector_outcomes
            .iter()
            .map(|o| o.work_used)
            .sum::<u64>(),
        report.work_used
    );
    let sidecar = crate::drivers::scan(
        zeff_emu_common::system::System::Gb,
        &bytes,
        crate::ScanLimits::default(),
        &cancel,
    );
    assert_eq!(sidecar.driver_candidates, report.driver_candidates);
    assert!(sidecar.findings.is_empty());
}
