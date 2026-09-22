use super::*;

pub(super) fn inspect(
    bytes: &[u8],
    report: &mut ScanReport,
    budget: &mut Budget<'_>,
) -> Result<(), ScanStop> {
    let start = budget.remaining;
    let capacity = report.limits.max_candidates as usize - report.song_count();
    let result =
        super::super::gb_native::scan(bytes, &mut report.gb_native_songs, budget, capacity);
    record(
        report,
        2,
        result,
        report.gb_native_songs.len(),
        start - budget.remaining,
    )?;
    let start = budget.remaining;
    let capacity = report.limits.max_candidates as usize - report.song_count();
    let result = super::super::gb_musyx::scan(bytes, &mut report.gb_musyx_songs, budget, capacity);
    record(
        report,
        3,
        result,
        report.gb_musyx_songs.len(),
        start - budget.remaining,
    )?;
    let start = budget.remaining;
    let capacity = report.limits.max_candidates as usize - report.song_count();
    let result = super::super::gb_tose::scan(bytes, &mut report.gb_tose_songs, budget, capacity);
    record(
        report,
        4,
        result,
        report.gb_tose_songs.len(),
        start - budget.remaining,
    )?;
    let start = budget.remaining;
    let capacity = report.limits.max_candidates as usize - report.song_count();
    let result = super::super::gb_quickthunder::scan(
        bytes,
        &mut report.gb_quickthunder_songs,
        budget,
        capacity,
    );
    record(
        report,
        5,
        result,
        report.gb_quickthunder_songs.len(),
        start - budget.remaining,
    )?;
    let start = budget.remaining;
    let capacity = report.limits.max_candidates as usize - report.song_count();
    let result = super::super::drivers::gb_fingerprints::scan(
        bytes,
        &mut report.driver_candidates,
        budget,
        capacity,
    );
    record(
        report,
        6,
        result,
        report.driver_candidates.len(),
        start - budget.remaining,
    )?;
    let start = budget.remaining;
    let capacity = report.limits.max_candidates as usize
        - report.song_count()
        - report.driver_candidates.len();
    let result = super::super::gb_ghx::scan(bytes, &mut report.gb_ghx_songs, budget, capacity);
    record(
        report,
        7,
        result,
        report.gb_ghx_songs.len(),
        start - budget.remaining,
    )?;
    let start = budget.remaining;
    let capacity = report.limits.max_candidates as usize
        - report.song_count()
        - report.driver_candidates.len();
    let result = super::super::gb_sound_system::scan(
        bytes,
        &mut report.gb_sound_system_songs,
        budget,
        capacity,
    );
    record(
        report,
        8,
        result,
        report.gb_sound_system_songs.len(),
        start - budget.remaining,
    )?;
    let start = budget.remaining;
    let capacity = report.limits.max_candidates as usize
        - report.song_count()
        - report.driver_candidates.len();
    let result =
        super::super::gb_carillon::scan(bytes, &mut report.gb_carillon_songs, budget, capacity);
    record(
        report,
        9,
        result,
        report.gb_carillon_songs.len(),
        start - budget.remaining,
    )?;
    let start = budget.remaining;
    let capacity = report.limits.max_candidates as usize
        - report.song_count()
        - report.driver_candidates.len();
    let source_sha256 = report
        .media
        .sha256
        .as_deref()
        .expect("preflight hashes accepted Game Boy sources");
    let result = super::super::huge::catalog::scan(
        bytes,
        source_sha256,
        &mut report.huge_songs,
        budget,
        capacity,
    );
    record(
        report,
        10,
        result,
        report.huge_songs.len(),
        start - budget.remaining,
    )
}

fn record(
    report: &mut ScanReport,
    index: usize,
    result: Result<(), ScanStop>,
    count: usize,
    work: u64,
) -> Result<(), ScanStop> {
    let state = match result {
        Ok(()) => DetectorState::Complete,
        Err(reason) => DetectorState::Incomplete(reason),
    };
    report.record_detector(report.applicable_detectors[index], state, count, work);
    result
}
