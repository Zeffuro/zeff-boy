use super::*;

fn roomy_limits() -> ScanLimits {
    ScanLimits {
        max_work: 20_000_000,
        max_candidates: 16,
    }
}

fn two_drivers(shared_stream: bool) -> Vec<u8> {
    let mut bytes = image(0x500, 0xc100, false);
    let other = image(0x1000, 0xc200, false);
    bytes[0x1000..0x1000 + fingerprint::REFERENCE.len()]
        .copy_from_slice(&other[0x1000..0x1000 + fingerprint::REFERENCE.len()]);
    bytes[0x3b..0x3e].copy_from_slice(&[0xcd, 0x1f, 0x12]);
    bytes[0x46..0x4c].copy_from_slice(&[0x21, 0, 0x21, 0xcd, 0x32, 0x10]);
    bytes[0x2100..0x2103].copy_from_slice(&[0xb2, 0x39, 0]);
    if shared_stream {
        word(&mut bytes, 0x47, 0x2000);
    }
    bytes
}

#[test]
fn every_driver_keeps_its_own_stream_and_frame_evidence() {
    for shared in [false, true] {
        let bytes = two_drivers(shared);
        let report = discover(&bytes, roomy_limits(), &AtomicBool::new(false)).unwrap();
        assert_eq!(report.bound.len(), 2);
        assert_eq!(report.candidate_count, 2);
        assert!(report.held.is_empty());
        for (index, song) in report.bound.iter().enumerate() {
            assert_eq!(
                song.offset,
                if shared || index == 0 { 0x2000 } else { 0x2100 }
            );
            assert_eq!(
                song.call_sites,
                [crate::tracker::FileSpan {
                    offset: (0x40 + index * 6) as u32,
                    byte_len: 6
                }]
            );
            assert_eq!(song.call_roots, [1]);
            assert_eq!(song.evidence.psg_play.offset, [0x532, 0x1032][index]);
            assert_eq!(song.evidence.ram_delta, [0x100, 0x200][index]);
            assert_eq!(
                song.evidence.frame_call_sites[0].offset,
                [0x38, 0x3b][index]
            );
            assert_eq!(song.evidence.frame_call_roots, [2]);
        }
        assert_eq!(
            report,
            discover(&bytes, roomy_limits(), &AtomicBool::new(false)).unwrap()
        );
    }
}

#[test]
fn an_unused_first_driver_does_not_hide_a_later_driver_or_lose_held_evidence() {
    for missing in [0x38, 0x40] {
        let mut bytes = two_drivers(false);
        bytes[missing..missing + 3].fill(0);
        let report = discover(&bytes, roomy_limits(), &AtomicBool::new(false)).unwrap();
        assert_eq!(report.bound.len(), 1);
        assert_eq!(report.bound[0].offset, 0x2100);
        assert_eq!(report.bound[0].evidence.psg_play.offset, 0x1032);
        assert_eq!(report.held.len(), 1);
        assert_eq!(report.held[0].kind, HeldKind::FingerprintOnly);
        assert_eq!(report.held[0].span.offset, 0x71f);
    }
}

#[test]
fn all_wrappers_bind_only_to_their_matching_driver_and_ram() {
    let mut bytes = image(0x500, 0xc100, true);
    let wrapper = bytes[0x100..0x116].to_vec();
    bytes[0x180..0x196].copy_from_slice(&wrapper);
    word(&mut bytes, 0x44, 0x180);
    let report = discover(&bytes, roomy_limits(), &AtomicBool::new(false)).unwrap();
    assert_eq!(report.bound.len(), 1);
    assert_eq!(
        report.bound[0].evidence.psg_play_loops.unwrap().offset,
        0x180
    );
    for at in [0x181, 0x185, 0x191] {
        let mut bad = bytes.clone();
        bad[at] ^= 1;
        let report = discover(&bad, roomy_limits(), &AtomicBool::new(false)).unwrap();
        assert!(report.bound.is_empty());
    }
}

#[test]
fn direct_and_wrapper_aliases_preserve_the_actual_entry_used() {
    let mut bytes = image(0x500, 0xc100, true);
    let wrapper = bytes[0x100..0x116].to_vec();
    bytes[0x180..0x196].copy_from_slice(&wrapper);
    bytes[0x46..0x4c].copy_from_slice(&[0x21, 0, 0x20, 0xcd, 0x80, 1]);
    bytes[0x4c..0x52].copy_from_slice(&[0x21, 0, 0x20, 0xcd, 0x32, 5]);
    bytes[0x52..0x58].copy_from_slice(&[0x21, 0, 0x20, 0xcd, 0x80, 1]);
    let report = discover(&bytes, roomy_limits(), &AtomicBool::new(false)).unwrap();
    assert_eq!(report.candidate_count, 4);
    assert_eq!(report.bound.len(), 3);
    let mut sites = std::collections::BTreeMap::new();
    for song in &report.bound {
        assert_eq!(song.offset, 0x2000);
        for call in &song.call_sites {
            sites.insert(
                call.offset,
                song.evidence.psg_play_loops.map(|span| span.offset),
            );
        }
    }
    assert_eq!(
        sites.into_iter().collect::<Vec<_>>(),
        [
            (0x40, Some(0x100)),
            (0x46, Some(0x180)),
            (0x4c, None),
            (0x52, Some(0x180))
        ]
    );
}

#[test]
fn a_damaged_copy_cannot_borrow_another_copys_frame_gate() {
    for mutation in [0x1000 + 0x50, 0x3b] {
        let mut bytes = two_drivers(false);
        bytes[mutation] = 0;
        let report = discover(&bytes, roomy_limits(), &AtomicBool::new(false)).unwrap();
        assert_eq!(report.bound.len(), 1);
        assert_eq!(report.bound[0].offset, 0x2000);
        assert_eq!(report.bound[0].evidence.psg_play.offset, 0x532);
    }
}

#[test]
fn budgets_cover_all_copies_and_never_return_a_partial_inventory() {
    let bytes = two_drivers(false);
    let cancel = AtomicBool::new(false);
    assert_eq!(
        discover(
            &bytes,
            ScanLimits {
                max_candidates: 1,
                ..roomy_limits()
            },
            &cancel
        ),
        Err(ScanStop::CandidateLimit)
    );
    assert_eq!(
        discover(
            &bytes,
            ScanLimits {
                max_work: 2_000_000,
                ..roomy_limits()
            },
            &cancel
        ),
        Err(ScanStop::WorkLimit)
    );
    assert_eq!(
        discover(&bytes, roomy_limits(), &AtomicBool::new(true)),
        Err(ScanStop::Cancelled)
    );
}
