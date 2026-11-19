use super::*;
use crate::emu_thread::audio_timing::Record;
use std::collections::{BTreeMap, BTreeSet};

pub(in crate::app::tas_control::tests::audio_host) fn report(
    records: Vec<Record>,
    timeline: &Value,
) -> Value {
    let windows: Vec<[u64; 2]> = serde_json::from_value(timeline["windows_ns"].clone()).unwrap();
    let mut requests: BTreeMap<u64, Vec<&Record>> = BTreeMap::new();
    let mut counts = BTreeMap::new();
    for record in &records {
        *counts.entry(record.stage).or_insert(0_usize) += 1;
        if record.request != 0 {
            requests.entry(record.request).or_default().push(record);
        }
    }
    let overlaps = |first: u64, last: u64| {
        windows
            .iter()
            .any(|[begin, end]| first <= *end && last >= *begin)
    };
    let mut retained: BTreeSet<_> = requests
        .iter()
        .filter(|(_, events)| overlaps(events.first().unwrap().at_ns, events.last().unwrap().at_ns))
        .map(|(&id, _)| id)
        .collect();
    let mut driver_pending = BTreeMap::new();
    let mut driver_retained = BTreeSet::new();
    for (index, record) in records.iter().enumerate().filter(|(_, r)| r.request == 0) {
        if contains(&windows, record.at_ns) {
            driver_retained.insert(index);
        }
        if let Some(stage) = record.stage.strip_suffix("_begin") {
            driver_pending.insert(stage, index);
        } else if let Some(stage) = record.stage.strip_suffix("_end")
            && let Some(begin) = driver_pending.remove(stage)
            && overlaps(records[begin].at_ns, record.at_ns)
        {
            driver_retained.extend([begin, index]);
        }
    }
    let mut maxima = BTreeMap::new();
    for (name, begin, end) in [
        ("dispatch_to_worker", "dispatch_begin", "worker_start"),
        ("worker_to_result", "worker_start", "result_built"),
        ("first_field_to_send", "field_audio_ready", "send_begin"),
        ("result_to_send", "result_built", "send_begin"),
        ("send_to_app", "send_begin", "app_observe"),
        ("app_to_queue", "app_observe", "app_queue_end"),
    ] {
        let mut ranked: Vec<_> = requests
            .iter()
            .filter_map(|(&id, events)| {
                let first = events.iter().find(|r| r.stage == begin)?.at_ns;
                let last = events.iter().find(|r| r.stage == end)?.at_ns;
                last.checked_sub(first).map(|duration| (duration, id))
            })
            .collect();
        ranked.sort_unstable_by(|a, b| b.cmp(a));
        ranked.truncate(8);
        retained.extend(ranked.iter().map(|&(_, id)| id));
        maxima.insert(name, ranked);
    }
    let events: Vec<_> = records
        .iter()
        .enumerate()
        .filter(|(index, r)| {
            if r.request == 0 {
                driver_retained.contains(index)
            } else {
                retained.contains(&r.request)
            }
        })
        .map(|(_, r)| r)
        .collect();
    json!({
        "clock": timeline["clock"], "lost_records": 0,
        "payloads": {"dispatch_begin": "field=residual timing debt ns at tick now after frames_due; frames=requested; aux=inflight before dispatch", "field_step_end": "step returned before finalization, not proof of drained samples", "field_audio_ready": "drain completed; single-field drain follows UI snapshot, multi-field drains immediately", "app_queue_end": "aux=observed ring stereo scalars after queueing"},
        "total_records": records.len(), "total_requests": requests.len(),
        "stage_counts": counts, "largest_intervals_ns_request": maxima,
        "retained_request_ids": retained, "events": events,
        "limits": "test-only observer; bounded nonblocking channel; complete chains for window-intersecting requests and top8 intervals; driver interval endpoints overlapping callback windows; wall time is not CPU time; merged delivery totals repeat per constituent ID",
    })
}

#[test]
fn request_report_keeps_whole_chains_and_exposes_withheld_field_time() {
    let event = |at_ns, request, stage| Record {
        at_ns,
        request,
        stage,
        field: 0,
        frames: 1,
        scalars: 2,
        aux: 0,
    };
    let report = report(
        vec![
            event(1, 1, "dispatch_begin"),
            event(10, 1, "worker_start"),
            event(20, 1, "field_audio_ready"),
            event(50, 1, "send_begin"),
            event(70, 1, "app_observe"),
            event(80, 1, "app_queue_end"),
            event(55, 0, "driver_sleep_end"),
            event(99, 0, "driver_poll_end"),
        ],
        &json!({"clock": "shared", "windows_ns": [[50, 60]]}),
    );
    assert_eq!(report["total_requests"], 1);
    assert_eq!(report["events"].as_array().unwrap().len(), 7);
    assert_eq!(report["events"][0]["at_ns"], 1);
    assert_eq!(
        report["largest_intervals_ns_request"]["first_field_to_send"],
        json!([[30, 1]])
    );
}

#[test]
fn request_report_retains_spanning_requests_and_both_driver_endpoints() {
    let event = |at_ns, request, stage| Record {
        at_ns,
        request,
        stage,
        field: 0,
        frames: 1,
        scalars: 2,
        aux: 0,
    };
    let mut records = vec![
        event(1, 99, "dispatch_begin"),
        event(99, 99, "worker_start"),
        event(2, 0, "driver_sleep_begin"),
        event(98, 0, "driver_sleep_end"),
    ];
    for id in 1..=9 {
        records.extend([
            event(0, id, "dispatch_begin"),
            event(100, id, "worker_start"),
        ]);
    }
    records.sort_by_key(|r| r.at_ns);
    let result = report(
        records,
        &json!({"clock": "shared", "windows_ns": [[50, 60]]}),
    );
    let events = result["events"].as_array().unwrap();
    assert_eq!(events.iter().filter(|r| r["request"] == 99).count(), 2);
    assert_eq!(events.iter().filter(|r| r["request"] == 0).count(), 2);
}
