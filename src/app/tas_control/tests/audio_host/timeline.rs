use super::*;

#[derive(serde::Serialize)]
pub(super) struct PumpRecord {
    begin_ns: u64,
    after_tick_ns: u64,
    end_ns: u64,
    before: [u64; 5],
    after_tick: [u64; 5],
    after_pump: [u64; 5],
    #[serde(skip)]
    epoch: Instant,
}

impl PumpRecord {
    pub(super) fn after_tick(mut self, output: &AudioOutput) -> Self {
        self.after_tick_ns = self.epoch.elapsed().as_nanos() as u64;
        self.after_tick = output.test_queue_timing();
        self
    }
}

pub(super) struct Timeline {
    epoch: Instant,
    pumps: Vec<PumpRecord>,
}

impl Timeline {
    pub(super) fn new(epoch: Instant, duration: Duration) -> Self {
        Self {
            epoch,
            pumps: Vec::with_capacity(duration.as_millis() as usize + 1024),
        }
    }

    pub(super) fn begin(&self, output: &AudioOutput) -> PumpRecord {
        PumpRecord {
            begin_ns: self.epoch.elapsed().as_nanos() as u64,
            after_tick_ns: 0,
            end_ns: 0,
            before: output.test_queue_timing(),
            after_tick: [0; 5],
            after_pump: [0; 5],
            epoch: self.epoch,
        }
    }

    pub(super) fn finish(&mut self, mut record: PumpRecord, output: &AudioOutput) {
        record.end_ns = self.epoch.elapsed().as_nanos() as u64;
        record.after_pump = output.test_queue_timing();
        self.pumps.push(record);
    }

    pub(super) fn report(
        self,
        start: Instant,
        arrivals: &[[u64; 4]],
        callback: &mut Value,
    ) -> Value {
        let records = callback
            .as_object_mut()
            .unwrap()
            .remove("timeline")
            .unwrap();
        let records = records.as_array().unwrap();
        let app_start_ns = start.duration_since(self.epoch).as_nanos() as u64;
        let capture_end_ns = self.pumps.last().map_or(0, |p| p.end_ns).max(
            records
                .last()
                .map_or(0, |r| r["queue"]["fill_end_ns"].as_u64().unwrap()),
        );
        let mut centers = vec![0, app_start_ns, capture_end_ns];
        for (index, record) in records.iter().enumerate() {
            let queue = &record["queue"];
            if record["incident"] == true
                || (queue["primed_before"] == false && queue["primed_after"] == true)
            {
                centers.push(at(record));
                centers.extend(records.get(index.wrapping_sub(1)).map(at));
                centers.extend(records.get(index + 1).map(at));
            }
        }
        if let Some(record) = records
            .iter()
            .filter(|r| r["queue"]["primed_before"] == true)
            .min_by_key(|r| r["queue"]["available_before"].as_u64().unwrap())
        {
            centers.push(at(record));
        }
        if let Some(pair) = records
            .windows(2)
            .max_by_key(|pair| at(&pair[1]) - at(&pair[0]))
        {
            centers.extend([at(&pair[0]), at(&pair[1])]);
        }
        if let Some(pair) = self
            .pumps
            .windows(2)
            .max_by_key(|p| p[1].begin_ns - p[0].begin_ns)
        {
            centers.extend([pair[0].begin_ns, pair[1].begin_ns]);
        }
        if let Some(pair) = arrivals
            .windows(2)
            .max_by_key(|pair| pair[1][0] - pair[0][0])
        {
            centers.extend([pair[0][0], pair[1][0]]);
        }
        for interval in [
            self.pumps
                .iter()
                .max_by_key(|p| p.after_tick_ns - p.begin_ns),
            self.pumps.iter().max_by_key(|p| p.end_ns - p.after_tick_ns),
        ]
        .into_iter()
        .flatten()
        {
            centers.extend([interval.begin_ns, interval.end_ns]);
        }
        let windows = windows(centers);
        let retained_pumps: Vec<_> = self
            .pumps
            .iter()
            .filter(|p| {
                windows
                    .iter()
                    .any(|[begin, end]| p.begin_ns <= *end && p.end_ns >= *begin)
            })
            .collect();
        let retained_arrivals: Vec<_> = arrivals
            .iter()
            .filter(|r| contains(&windows, r[0]))
            .collect();
        let retained_callbacks: Vec<_> = records
            .iter()
            .filter(|r| contains(&windows, at(r)))
            .collect();
        let end = capture_end_ns / 1_000_000_000 + 1;
        let seconds: Vec<_> = (0..end).map(|second| {
            let within = |ns: u64| ns / 1_000_000_000 == second;
            let pumps: Vec<_> = self.pumps.iter().filter(|p| within(p.begin_ns)).collect();
            let callbacks: Vec<_> = records.iter().filter(|r| within(at(r))).collect();
            json!({
                "second": second,
                "minimum_primed_available_scalars": callbacks.iter().filter(|r| r["queue"]["primed_before"] == true)
                    .map(|r| r["queue"]["available_before"].as_u64().unwrap()).min(),
                "minimum_app_ring_scalars": pumps.iter().flat_map(|p| [p.before[0], p.after_tick[0], p.after_pump[0]]).min(),
                "maximum_app_staged_scalars": pumps.iter().flat_map(|p| [p.before[1], p.after_tick[1], p.after_pump[1]]).max(),
                "source_frames": arrivals.iter().filter(|r| within(r[0])).map(|r| r[1]).sum::<u64>(),
                "source_scalars": arrivals.iter().filter(|r| within(r[0])).map(|r| r[2]).sum::<u64>(),
                "consumed_output_frames": callbacks.iter().map(|r| r["consumed"].as_u64().unwrap()).sum::<u64>(),
                "maximum_tick_ns": pumps.iter().map(|p| p.after_tick_ns-p.begin_ns).max(),
                "maximum_queue_pump_ns": pumps.iter().map(|p| p.end_ns-p.after_tick_ns).max(),
            })
        }).collect();
        json!({
            "clock": "shared std::time::Instant; nanoseconds since before stream setup",
            "app_start_ns": app_start_ns, "capture_end_ns": capture_end_ns,
            "queue_columns": ["ring_scalars", "staged_scalars", "preroll_scalars", "producer_generation", "published_generation"],
            "arrival_columns": ["at_ns", "advanced_frames", "source_scalars", "completed_step_requests"],
            "queue_snapshots_are_concurrent_observations": true,
            "windows_ns": windows,
            "total_records": {"pumps": self.pumps.len(), "arrivals": arrivals.len(), "callbacks": records.len()},
            "pumps": retained_pumps, "arrivals": retained_arrivals, "callbacks": retained_callbacks,
            "one_second_summary": seconds,
        })
    }
}

fn at(record: &Value) -> u64 {
    record["queue"]["at_ns"].as_u64().unwrap()
}

fn windows(centers: Vec<u64>) -> Vec<[u64; 2]> {
    let mut ranges: Vec<_> = centers
        .into_iter()
        .map(|at| {
            [
                at.saturating_sub(500_000_000),
                at.saturating_add(500_000_000),
            ]
        })
        .collect();
    ranges.sort_unstable();
    let mut merged: Vec<[u64; 2]> = Vec::new();
    for [start, end] in ranges {
        if let Some(last) = merged.last_mut().filter(|last| start <= last[1]) {
            last[1] = last[1].max(end);
        } else {
            merged.push([start, end]);
        }
    }
    merged
}

fn contains(windows: &[[u64; 2]], at: u64) -> bool {
    windows
        .iter()
        .any(|[start, end]| (*start..=*end).contains(&at))
}

#[test]
fn synchronized_windows_preserve_both_sides_of_a_stall_and_absolute_epoch() {
    let ranges = windows(vec![0, 4_000_000_000, 4_250_000_000, u64::MAX]);
    assert_eq!(
        ranges,
        vec![
            [0, 500_000_000],
            [3_500_000_000, 4_750_000_000],
            [u64::MAX - 500_000_000, u64::MAX]
        ]
    );
    assert!(contains(&ranges, 3_999_999_999));
    assert!(contains(&ranges, 4_250_000_001));
    assert!(!contains(&ranges, 2_000_000_000));
}

#[test]
fn synchronized_report_keeps_incidents_gap_endpoints_and_capture_end_after_teardown() {
    let epoch = Instant::now() - Duration::from_secs(60);
    let pumps = [1250, 1500, 5250, 5500, 8000, 12000, 18000, 20100]
        .map(|ms| {
            let begin_ns = ms * 1_000_000;
            let after_tick_ns = begin_ns + if ms == 5500 { 200_000_000 } else { 2_000_000 };
            PumpRecord {
                epoch,
                begin_ns,
                after_tick_ns,
                end_ns: after_tick_ns + 1_000_000,
                before: [100, 0, 100, 0, 0],
                after_tick: [200, 20, 100, 0, 0],
                after_pump: [220, 0, 100, 0, 0],
            }
        })
        .into_iter()
        .collect();
    let callbacks: Vec<_> = [2, 3, 6, 7, 9, 10, 11, 14, 20].into_iter().map(|second| {
        json!({
            "incident": second == 6, "consumed": if second == 6 { 0 } else { 4 },
            "queue": {
                "at_ns": second * 1_000_000_000_u64, "fill_end_ns": second * 1_000_000_000_u64 + 1000,
                "primed_before": second != 7, "primed_after": second != 6,
                "available_before": if second == 11 { 8 } else { 16 },
            }
        })
    }).collect();
    let mut callback = json!({"timeline": callbacks});
    let arrivals = [
        [4_100_000_000, 1, 100, 1],
        [4_200_000_000, 2, 200, 1],
        [17_100_000_000, 3, 300, 1],
        [17_200_000_000, 4, 400, 1],
    ];
    let report =
        Timeline { epoch, pumps }.report(epoch + Duration::from_secs(1), &arrivals, &mut callback);
    assert_eq!(report["app_start_ns"], 1_000_000_000_u64);
    assert_eq!(report["capture_end_ns"], 20_103_000_000_u64);
    assert_eq!(report["one_second_summary"].as_array().unwrap().len(), 21);
    let callbacks: Vec<_> = report["callbacks"]
        .as_array()
        .unwrap()
        .iter()
        .map(at)
        .collect();
    for second in [3, 6, 7, 11, 14, 20] {
        assert!(callbacks.contains(&(second * 1_000_000_000)));
    }
    assert!(!callbacks.contains(&10_000_000_000));
    assert!(callbacks.windows(2).all(|p| p[0] <= p[1]));
    assert_eq!(report["arrivals"], json!(arrivals));
    assert_eq!(report["one_second_summary"][4]["source_frames"], 3);
    assert_eq!(report["one_second_summary"][17]["source_scalars"], 700);
    assert_eq!(report["one_second_summary"][6]["consumed_output_frames"], 0);
    assert_eq!(report["one_second_summary"][7]["consumed_output_frames"], 4);
    assert_eq!(
        report["one_second_summary"][5]["maximum_tick_ns"],
        200_000_000
    );
    assert!(
        report["pumps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["begin_ns"] == 12_000_000_000_u64)
    );
    assert!(
        report["pumps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["begin_ns"] == 18_000_000_000_u64)
    );
    assert!(
        report["pumps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["end_ns"] == 20_103_000_000_u64)
    );
    assert!(callback.get("timeline").is_none());
}
