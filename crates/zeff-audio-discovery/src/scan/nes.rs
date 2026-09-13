use super::*;

pub(super) fn inspect(
    bytes: &[u8],
    report: &mut ScanReport,
    budget: &mut Budget<'_>,
) -> Result<(), ScanStop> {
    let start = budget.remaining;
    let capacity = report.limits.max_candidates as usize - report.song_count();
    let result =
        super::super::nes_native::scan(bytes, &mut report.nes_native_songs, budget, capacity);
    record(
        report,
        2,
        result,
        report.nes_native_songs.len(),
        start - budget.remaining,
    )?;
    let start = budget.remaining;
    let capacity = report.limits.max_candidates as usize - report.song_count();
    let result = super::super::nes_tose::scan(bytes, &mut report.nes_tose_songs, budget, capacity);
    record(
        report,
        3,
        result,
        report.nes_tose_songs.len(),
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
