use std::collections::{HashSet, VecDeque};

use super::HeadlessOptions;

#[derive(Clone)]
pub(super) struct StuckReport {
    pub(super) frame: u64,
    pub(super) window_frames: usize,
    pub(super) unique_pcs: usize,
    pub(super) framebuffer_changed: bool,
    pub(super) first_pc: u64,
    pub(super) last_pc: u64,
    pub(super) classification: Option<String>,
    pub(super) expected_wait: bool,
}

pub(super) struct StuckTracker {
    window_frames: usize,
    pc_threshold: usize,
    pcs: VecDeque<u64>,
    framebuffer_hashes: VecDeque<u64>,
    progress_markers: VecDeque<Option<u64>>,
    pub(super) current_report: Option<StuckReport>,
}

pub(super) struct StuckObservation<'a> {
    pub(super) frame: u64,
    pub(super) pc: u64,
    pub(super) framebuffer: &'a [u8],
    pub(super) progress_marker: Option<u64>,
    pub(super) classification: Option<&'a str>,
    pub(super) expected_wait: bool,
}

impl StuckTracker {
    pub(super) fn from_options(opts: &HeadlessOptions) -> Option<Self> {
        if opts.stuck_window_frames == 0 {
            return None;
        }

        Some(Self {
            window_frames: opts.stuck_window_frames.min(usize::MAX as u64) as usize,
            pc_threshold: opts.stuck_pc_threshold,
            pcs: VecDeque::new(),
            framebuffer_hashes: VecDeque::new(),
            progress_markers: VecDeque::new(),
            current_report: None,
        })
    }

    pub(super) fn observe(&mut self, observation: StuckObservation<'_>) -> Option<&StuckReport> {
        let StuckObservation {
            frame,
            pc,
            framebuffer,
            progress_marker,
            classification,
            expected_wait,
        } = observation;
        self.pcs.push_back(pc);
        self.framebuffer_hashes
            .push_back(framebuffer_fingerprint(framebuffer));
        self.progress_markers.push_back(progress_marker);

        while self.pcs.len() > self.window_frames {
            self.pcs.pop_front();
        }
        while self.framebuffer_hashes.len() > self.window_frames {
            self.framebuffer_hashes.pop_front();
        }
        while self.progress_markers.len() > self.window_frames {
            self.progress_markers.pop_front();
        }

        if self.pcs.len() < self.window_frames {
            return self.current_report.as_ref();
        }

        let unique_pcs = self.pcs.iter().copied().collect::<HashSet<_>>().len();
        let framebuffer_changed = self
            .framebuffer_hashes
            .iter()
            .copied()
            .collect::<HashSet<_>>()
            .len()
            > 1;
        let progress_marker_changed = self
            .progress_markers
            .iter()
            .flatten()
            .copied()
            .collect::<HashSet<_>>()
            .len()
            > 1;

        if unique_pcs <= self.pc_threshold && !framebuffer_changed && !progress_marker_changed {
            self.current_report = Some(StuckReport {
                frame,
                window_frames: self.window_frames,
                unique_pcs,
                framebuffer_changed,
                first_pc: self.pcs.front().copied().unwrap_or(pc),
                last_pc: pc,
                classification: classification.map(str::to_owned),
                expected_wait,
            });
        } else {
            self.current_report = None;
        }

        self.current_report.as_ref()
    }

    pub(super) fn current_report(&self) -> Option<&StuckReport> {
        self.current_report.as_ref()
    }
}

pub(super) fn framebuffer_fingerprint(framebuffer: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

    let mut hash = FNV_OFFSET ^ framebuffer.len() as u64;
    let stride = (framebuffer.len() / 4096).max(1);
    for byte in framebuffer.iter().step_by(stride) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[allow(clippy::too_many_arguments)]
pub(super) fn observe_stuck(
    tracker: &mut Option<StuckTracker>,
    system: &str,
    pc_width: usize,
    frame: u64,
    pc: u64,
    framebuffer: &[u8],
    progress_marker: Option<u64>,
    classification: Option<&str>,
    expected_wait: bool,
    stuck_active: &mut bool,
) {
    let Some(tracker) = tracker.as_mut() else {
        return;
    };

    match tracker.observe(StuckObservation {
        frame,
        pc,
        framebuffer,
        progress_marker,
        classification,
        expected_wait,
    }) {
        Some(report) if !*stuck_active => {
            println!("{}", format_stuck_report(system, report, pc_width));
            *stuck_active = true;
        }
        None if *stuck_active => {
            println!("[headless] system={system} stuck-cleared frame={frame}");
            *stuck_active = false;
        }
        _ => {}
    }
}

pub(super) fn format_pc(pc: u64, width: usize) -> String {
    format!("{:0width$X}", pc, width = width)
}

pub(super) fn format_stuck_report(system: &str, report: &StuckReport, pc_width: usize) -> String {
    let event = if report.expected_wait {
        "idle-detected"
    } else {
        "stuck-detected"
    };
    let classification = report
        .classification
        .as_deref()
        .map(|value| format!(" classification={value}"))
        .unwrap_or_default();
    format!(
        "[headless] system={} {} frame={} window={} unique_pcs={} framebuffer_changed={} first_pc={} last_pc={}{}",
        system,
        event,
        report.frame,
        report.window_frames,
        report.unique_pcs,
        if report.framebuffer_changed { 1 } else { 0 },
        format_pc(report.first_pc, pc_width),
        format_pc(report.last_pc, pc_width),
        classification
    )
}

pub(super) fn fail_on_stuck_if_needed(
    system: &str,
    tracker: Option<&StuckTracker>,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    if opts.fail_on_stuck
        && let Some(report) = tracker.and_then(StuckTracker::current_report)
        && !report.expected_wait
    {
        anyhow::bail!(
            "{system} headless run detected a stuck window: {} frames, {} unique PCs",
            report.window_frames,
            report.unique_pcs
        );
    }
    Ok(())
}
