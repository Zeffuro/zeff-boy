use std::{
    collections::BTreeMap,
    env,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use zeff_emu_common::{
    media::{MediaEvent, MediaSlotId},
    replay::{ReplayEvent, ReplayStartMetadata},
};

use super::*;
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasBranchDiff, TasBranchDiffLimits, TasControllerInput,
    TasDeviceIdentity, TasDigest, TasEditorSession, TasExternalIdentity, TasInitialBranch,
    TasInputFrame, TasInputSpan, TasProject, TasProjectIdentity, TasSeekStateCache,
};

const DEFAULT_SPAN_COUNT_PER_BRANCH: usize = 20_000;
const MIN_MEASURE_SPAN_COUNT_PER_BRANCH: usize = 10_000;
const FRAME_COUNT: u64 = 1_000_000;

fn input(buttons: u8) -> TasInputFrame {
    TasInputFrame {
        players: [TasControllerInput { buttons, dpad: 0 }; 5],
        ..TasInputFrame::default()
    }
}

fn span_count_per_branch() -> usize {
    if env::var_os("ZEFF_TAS_DIFF_MEASURE").is_none() {
        return DEFAULT_SPAN_COUNT_PER_BRANCH;
    }
    let Some(value) = env::var("ZEFF_TAS_DIFF_SPANS").ok() else {
        return DEFAULT_SPAN_COUNT_PER_BRANCH;
    };
    let count = value
        .parse::<usize>()
        .expect("ZEFF_TAS_DIFF_SPANS must be a whole number");
    assert!(
        (MIN_MEASURE_SPAN_COUNT_PER_BRANCH..=DEFAULT_SPAN_COUNT_PER_BRANCH).contains(&count),
        "ZEFF_TAS_DIFF_SPANS must be within {MIN_MEASURE_SPAN_COUNT_PER_BRANCH}..={DEFAULT_SPAN_COUNT_PER_BRANCH}"
    );
    count
}

fn sparse_spans(span_count: usize) -> Vec<TasInputSpan> {
    let stride = FRAME_COUNT / (span_count as u64);
    (0..span_count)
        .map(|index| TasInputSpan {
            start: (index as u64) * stride,
            length: 1,
            input: input(if index % 2 == 0 { 1 } else { 2 }),
        })
        .collect()
}

fn canonical_events(mut events: Vec<ReplayEvent>) -> Vec<ReplayEvent> {
    events.sort_by(ReplayEvent::canonical_cmp);
    events
}

fn source_events(frame_count: u64) -> Vec<ReplayEvent> {
    canonical_events(vec![
        ReplayEvent::FdsDiskSide { frame: 8, side: 0 },
        ReplayEvent::Media {
            frame: 8,
            sequence: 0,
            event: MediaEvent::Eject {
                slot: MediaSlotId::new("cart"),
            },
        },
        ReplayEvent::FdsDiskSide {
            frame: frame_count / 4,
            side: 0,
        },
    ])
}

fn target_events(frame_count: u64) -> Vec<ReplayEvent> {
    canonical_events(vec![
        ReplayEvent::FdsDiskSide { frame: 8, side: 1 },
        ReplayEvent::Media {
            frame: 8,
            sequence: 0,
            event: MediaEvent::Eject {
                slot: MediaSlotId::new("cart"),
            },
        },
        ReplayEvent::FdsDiskSide {
            frame: frame_count / 2,
            side: 0,
        },
    ])
}

fn project(span_count: usize) -> TasProject {
    let start_state = vec![0xD5; 16];
    TasProject::new(
        "branch-diff-responsiveness",
        TasProjectIdentity {
            system: "nes".to_owned(),
            core_family: "nes-test".to_owned(),
            determinism_abi: "nes-test-sync-v1".to_owned(),
            source_media_sha256: TasDigest([1; 32]),
            effective_media_sha256: TasDigest([1; 32]),
            patches: Vec::new(),
            firmware: Vec::new(),
            devices: vec![TasDeviceIdentity {
                port: "p1".to_owned(),
                device: "nes-standard-controller".to_owned(),
                configuration_sha256: TasDigest([2; 32]),
            }],
            sync_config_sha256: TasDigest([3; 32]),
            persistent_state: TasExternalIdentity::Absent,
            rtc_state: TasExternalIdentity::Absent,
            sensor_state: TasExternalIdentity::Absent,
            cheats: TasExternalIdentity::Absent,
            state_format_compatibility_id: "nes-test-state-v1".to_owned(),
            start_state_sha256: TasDigest::from_bytes(&start_state),
        },
        start_state,
        ReplayStartMetadata::default(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count: FRAME_COUNT,
            input_spans: sparse_spans(span_count),
            events: source_events(FRAME_COUNT),
        },
        BTreeMap::new(),
    )
    .unwrap()
}

fn state_with_sparse_branches(
    span_count: usize,
) -> (crate::test_support::TestDirectory, TasEditorWindowState) {
    let root = crate::test_support::test_directory("tas-branch-diff-responsiveness").unwrap();
    let manual_path = root.path().join("branch-diff.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
    let session =
        TasEditorSession::new(project(span_count), manual_path, autosaves, seek_cache).unwrap();
    let mut state = TasEditorWindowState::with_seek_cache_root(root.path().join("seek-cache"));
    state.install_verified_export_session(session);
    state
        .session
        .as_mut()
        .unwrap()
        .edit_transaction(|edit| {
            let stride = FRAME_COUNT / (span_count as u64);
            let first = ((span_count / 5) as u64) * stride;
            let second = ((span_count / 2) as u64) * stride;
            let third = ((span_count * 4 / 5) as u64) * stride;
            edit.fork_branch("main", 0, "target", "Target")?;
            edit.set_input_range("target", first, 1, input(4))?;
            edit.set_input_range("target", second, 1, input(8))?;
            edit.set_input_range("target", third, 1, input(16))?;
            edit.replace_branch_events("target", target_events(FRAME_COUNT))
        })
        .unwrap();
    let manual_path = state.session.as_ref().unwrap().manual_path().to_path_buf();
    state
        .session
        .as_ref()
        .unwrap()
        .project()
        .save_atomic(&manual_path)
        .unwrap();
    (root, state)
}

fn diff_digest(diff: &TasBranchDiff) -> TasDigest {
    TasDigest::from_bytes(format!("{diff:?}").as_bytes())
}

fn refresh_once(state: &mut TasEditorWindowState, ctx: &egui::Context) {
    let session = state.session.as_ref().unwrap();
    let _ = state.branch_diff_editor.refresh(session, ctx, true);
}

fn refresh_until_ready(state: &mut TasEditorWindowState, ctx: &egui::Context) -> TasBranchDiff {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        refresh_once(state, ctx);
        if let Some(diff) = state.branch_diff_editor.cached_diff() {
            return diff.clone();
        }
        assert!(
            Instant::now() < deadline,
            "background TAS branch comparison did not complete"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

fn write_fixture_if_requested(state: &TasEditorWindowState) {
    let Some(path) = env::var_os("ZEFF_TAS_DIFF_FIXTURE_OUT") else {
        return;
    };
    let path = PathBuf::from(path);
    assert!(
        path.extension()
            .is_some_and(|extension| extension == "ztas"),
        "ZEFF_TAS_DIFF_FIXTURE_OUT must name a .ztas file"
    );
    state
        .session
        .as_ref()
        .unwrap()
        .project()
        .save_atomic(&path)
        .unwrap();
    println!("tas_branch_diff_fixture path={}", path.display());
}

fn measure_if_requested(
    state: &mut TasEditorWindowState,
    ctx: &egui::Context,
    expected: &TasBranchDiff,
    expected_digest: TasDigest,
) {
    let Some(samples) = env::var("ZEFF_TAS_DIFF_MEASURE")
        .ok()
        .map(|value| value.parse::<usize>().unwrap_or(9).clamp(1, 99))
    else {
        return;
    };

    for sample in 0..samples {
        state.branch_diff_editor.clear();
        let completion_start = Instant::now();
        refresh_once(state, ctx);
        let cold_elapsed = completion_start.elapsed().as_micros();
        let cold = refresh_until_ready(state, ctx);
        let completion_elapsed = completion_start.elapsed().as_micros();
        assert_eq!(cold, *expected);
        println!(
            "tas_branch_diff_measure phase=cold_dispatch sample={sample} elapsed_us={cold_elapsed} digest={} input_hunks={} event_hunks={} omitted_input_hunks={} omitted_event_hunks={}",
            expected_digest.to_hex(),
            cold.input_hunks.len(),
            cold.event_hunks.len(),
            cold.omitted_input_hunks,
            cold.omitted_event_hunks,
        );
        println!(
            "tas_branch_diff_measure phase=total_completion sample={sample} elapsed_us={completion_elapsed} digest={} input_hunks={} event_hunks={} omitted_input_hunks={} omitted_event_hunks={}",
            expected_digest.to_hex(),
            cold.input_hunks.len(),
            cold.event_hunks.len(),
            cold.omitted_input_hunks,
            cold.omitted_event_hunks,
        );

        let start = Instant::now();
        refresh_once(state, ctx);
        let warm_elapsed = start.elapsed().as_micros();
        let warm = state.branch_diff_editor.cached_diff().unwrap().clone();
        assert_eq!(warm, *expected);
        println!(
            "tas_branch_diff_measure phase=warm_cache sample={sample} elapsed_us={warm_elapsed} digest={} input_hunks={} event_hunks={} omitted_input_hunks={} omitted_event_hunks={}",
            expected_digest.to_hex(),
            warm.input_hunks.len(),
            warm.event_hunks.len(),
            warm.omitted_input_hunks,
            warm.omitted_event_hunks,
        );
    }
}

#[test]
fn near_limit_sparse_branch_diff_matches_direct_project_result() {
    let span_count = span_count_per_branch();
    let (_root, mut state) = state_with_sparse_branches(span_count);
    let expected = state
        .session
        .as_ref()
        .unwrap()
        .project()
        .diff_branches("main", "target", TasBranchDiffLimits::default())
        .unwrap();
    let source = state
        .session
        .as_ref()
        .unwrap()
        .project()
        .branch("main")
        .unwrap();
    let target = state
        .session
        .as_ref()
        .unwrap()
        .project()
        .branch("target")
        .unwrap();
    assert_eq!(source.input_spans().len(), span_count);
    assert_eq!(target.input_spans().len(), span_count);
    assert_eq!(
        source.input_spans().len() + target.input_spans().len(),
        span_count * 2
    );
    assert!(source.input_spans().len() + target.input_spans().len() <= 200_000);
    assert!(!expected.input_hunks.is_empty());
    assert!(!expected.event_hunks.is_empty());

    let ctx = egui::Context::default();
    state.branch_diff_editor.clear();
    refresh_once(&mut state, &ctx);
    let actual = refresh_until_ready(&mut state, &ctx);
    assert_eq!(actual, expected);

    write_fixture_if_requested(&state);
    measure_if_requested(&mut state, &ctx, &expected, diff_digest(&expected));
}
