use super::{Budget, Profile, RomSpan, scan_profile, sequence};
use crate::{MAX_CANDIDATES, MAX_ROM_BYTES, MAX_SCAN_WORK, ScanLimits};
use std::sync::atomic::AtomicBool;

// Synthetic table: two u32 pointers at 0x100; 13 twelve-byte channel records at 0x200.
const FUZZ_PROFILE: Profile = Profile {
    name: "synthetic-natsume-driver",
    sha256: "",
    table: 0x100,
    count: 2,
    channel_config: 0x200,
    silence: Some(1),
    witnesses: &[],
};

pub(crate) fn fuzz_parse(bytes: &[u8], limits: ScanLimits, cancel: &AtomicBool) {
    if bytes.len() > MAX_ROM_BYTES {
        return;
    }
    let work = limits.max_work.min(MAX_SCAN_WORK);
    let candidates = limits.max_candidates.min(MAX_CANDIDATES) as usize;
    let mut budget = Budget {
        cancel,
        remaining: work,
    };
    let mut songs = Vec::new();
    let _ = scan_profile(bytes, &mut songs, &mut budget, candidates, &FUZZ_PROFILE);
    assert!(songs.len() <= candidates);
    for song in &songs {
        check_span(song.table_entry, bytes);
        check_span(song.header, bytes);
        for channel in &song.channels {
            check_span(channel.entry, bytes);
        }
        for span in &song.mapped_spans {
            check_span(*span, bytes);
        }
    }
    // Byte zero chooses hardware kind; byte one starts a direct command stream.
    if bytes.len() > 1
        && candidates > 0
        && let Ok((channel, spans)) = sequence::inspect(bytes, 0, bytes[0] % 6, 1, &mut budget)
    {
        check_span(channel.entry, bytes);
        for span in spans {
            check_span(span, bytes);
        }
    }
    assert!(budget.remaining <= work);
    #[cfg(feature = "fuzzing")]
    {
        let mut report = crate::ScanReport::new(
            "synthetic-natsume-driver",
            1,
            &[],
            &[],
            crate::MediaIdentity {
                system: "gba",
                byte_len: bytes.len() as u64,
                sha256: None,
            },
            limits,
        );
        report.natsume_songs = songs;
        crate::fuzzing::check_graphs(&report);
    }
}

fn check_span(span: RomSpan, bytes: &[u8]) {
    let start = span.effective_offset as usize;
    let len = span.byte_len as usize;
    assert!(bytes.get(start..start.checked_add(len).unwrap()).is_some());
    assert_eq!(
        span.canonical_cpu_address,
        0x0800_0000 + span.effective_offset
    );
}
