use super::*;

fn install(bytes: &mut [u8], start: usize, ram: u16) {
    for index in 0..reference::CODE.len() {
        bytes[start + index] = reference::relocated_byte(index, start as u16, ram);
    }
}

pub(in crate::huge) fn fixture(start: usize, ram: u16) -> Vec<u8> {
    let mut bytes = super::super::tests::fixture();
    for at in [0x40, 0x48, 0x50, 0x58, 0x60] {
        bytes[at] = 0xc9;
    }
    bytes[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 1]);
    bytes[0x150..0x157].copy_from_slice(&[0x21, 0, 2, 0xcd, start as u8, (start >> 8) as u8, 0xc9]);
    let update = start + reference::UPDATE_OFFSET;
    bytes[0x40..0x44].copy_from_slice(&[0xcd, update as u8, (update >> 8) as u8, 0xd9]);
    install(&mut bytes, start, ram);
    bytes
}

fn discover_default(bytes: &[u8]) -> DiscoveryReport {
    discover(bytes, ScanLimits::default(), &AtomicBool::new(false)).unwrap()
}

#[test]
fn relocated_driver_binds_literal_song_and_preserves_static_call_evidence() {
    for (start, ram) in [(0x1800, 0xc000), (0x1897, 0xc1fd), (0x2801, 0xcf9c)] {
        let report = discover_default(&fixture(start, ram));
        assert!(report.held.is_empty(), "{report:?}");
        assert_eq!(report.bound.len(), 1);
        assert_eq!(report.candidate_count, 2);
        let found = &report.bound[0];
        assert_eq!(found.song.descriptor, span(0x200, 21));
        assert_eq!(found.song.notes.len(), 16);
        assert_eq!(
            found.init_calls,
            [CallSite {
                instruction: span(0x153, 3),
                roots: 1
            }]
        );
        assert_eq!(found.evidence.driver, span(start, reference::CODE.len()));
        assert_eq!(found.evidence.ram_address, ram);
        assert_eq!(
            found.evidence.irq_update_calls,
            [CallSite {
                instruction: span(0x40, 3),
                roots: 2
            }]
        );
    }
}

#[test]
fn every_driver_byte_and_incoherent_relocation_is_required() {
    let mut bytes = fixture(0x1800, 0xc000);
    for index in 0..reference::CODE.len() {
        bytes[0x1800 + index] ^= 1;
        let cancel = AtomicBool::new(false);
        let mut inner = crate::Budget {
            cancel: &cancel,
            remaining: ScanLimits::default().max_work,
        };
        let mut budget =
            Budget::new(&mut inner, ScanLimits::default().max_candidates as usize).unwrap();
        assert!(
            find_drivers(&bytes, &mut budget).unwrap().is_empty(),
            "byte {index:x}"
        );
        bytes[0x1800 + index] ^= 1;
    }
    assert!(discover_default(&fixture(0x1800, 0xcf9d)).bound.is_empty());
    assert!(discover_default(&fixture(0x1800, 0xbfff)).bound.is_empty());
}

#[test]
fn dead_init_missing_irq_and_unsupported_song_remain_held() {
    for (offset, value, kind) in [
        (0x100, 0xc9, HeldKind::NoLiteralInitCall),
        (0x40, 0xd9, HeldKind::NoIrqUpdateCall),
        (0x1001, 0x11, HeldKind::UnsupportedSong),
        (0x150, 0x11, HeldKind::NoLiteralInitCall),
    ] {
        let mut bytes = fixture(0x1800, 0xc000);
        bytes[offset] = value;
        let report = discover_default(&bytes);
        assert!(report.bound.is_empty());
        assert!(report.held.iter().any(|e| e.kind == kind), "{report:?}");
    }
    let mut bytes = fixture(0x1800, 0xc000);
    bytes[0x147] = 1;
    assert_eq!(
        discover_default(&bytes).held[0].kind,
        HeldKind::UnsupportedMapping
    );
    let mut bytes = fixture(0x1800, 0xc000);
    bytes[0x213..0x215].copy_from_slice(&0x1800u16.to_le_bytes());
    let report = discover_default(&bytes);
    assert!(report.bound.is_empty());
    assert_eq!(report.held[0].kind, HeldKind::DriverDataOverlap);
}

#[test]
fn later_driver_copies_and_shared_descriptor_aliases_are_not_lost() {
    let mut bytes = fixture(0x2800, 0xc000);
    install(&mut bytes, 0x1800, 0xc200);
    bytes[0x156..0x15d].copy_from_slice(&[0x21, 0, 2, 0xcd, 0, 0x28, 0xc9]);
    let report = discover_default(&bytes);
    assert_eq!(report.bound.len(), 1);
    assert_eq!(report.bound[0].init_calls.len(), 2);
    assert_eq!(report.bound[0].evidence.init_address, 0x2800);
    assert_eq!(report.held[0].kind, HeldKind::NoIrqUpdateCall);
    assert_eq!(report.held[0].span.offset, 0x1800);
    bytes[0x15a] = 0;
    bytes[0x15b] = 0x18;
    let update = 0x1800 + reference::UPDATE_OFFSET;
    bytes[0x43..0x47].copy_from_slice(&[0xcd, update as u8, (update >> 8) as u8, 0xd9]);
    let report = discover_default(&bytes);
    assert!(report.held.is_empty());
    assert_eq!(report.bound.len(), 2);
    assert_eq!(report.bound[0].song, report.bound[1].song);
}

#[test]
fn invalid_descriptors_report_bounded_call_sites() {
    for descriptor in [0x7fecu16, 0x8000, 0xffff] {
        let mut bytes = fixture(0x1800, 0xc000);
        bytes[0x151..0x153].copy_from_slice(&descriptor.to_le_bytes());
        let report = discover_default(&bytes);
        assert!(report.bound.is_empty());
        assert_eq!(report.held.len(), 1);
        assert_eq!(report.held[0].kind, HeldKind::InvalidDescriptorPointer);
        assert_eq!(report.held[0].span, span(0x153, 3));
        for evidence in report.held {
            assert!(evidence.span.offset as usize + evidence.span.byte_len as usize <= bytes.len());
        }
    }
}

#[test]
fn budgets_include_flow_matching_and_all_descriptor_parsing() {
    let bytes = fixture(0x1800, 0xc000);
    let report = discover_default(&bytes);
    let cancel = AtomicBool::new(false);
    let limits = ScanLimits {
        max_work: report.work_used,
        max_candidates: report.candidate_count,
    };
    assert_eq!(discover(&bytes, limits, &cancel).unwrap(), report);
    assert_eq!(
        discover(
            &bytes,
            ScanLimits {
                max_work: limits.max_work - 1,
                ..limits
            },
            &cancel
        ),
        Err(ScanStop::WorkLimit)
    );
    assert_eq!(
        discover(
            &bytes,
            ScanLimits {
                max_candidates: 1,
                ..limits
            },
            &cancel
        ),
        Err(ScanStop::CandidateLimit)
    );
    assert_eq!(
        discover(&bytes, limits, &AtomicBool::new(true)),
        Err(ScanStop::Cancelled)
    );
    assert_eq!(
        discover(
            &bytes,
            ScanLimits {
                max_work: crate::MAX_SCAN_WORK + 1,
                ..limits
            },
            &cancel
        ),
        Err(ScanStop::InvalidLimits)
    );
}
