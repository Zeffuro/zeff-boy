use serde_json::{Value, json};

const MAX_ROWS: usize = 16;

pub(super) fn compare(rows: &[Value]) -> Value {
    if rows.len() > MAX_ROWS {
        return unavailable("too_many_rows");
    }
    let Some((reference_index, reference_row)) = rows.iter().enumerate().find(|(_, row)| {
        row["status"] == "success"
            && row
                .pointer("/applied_input/player_1")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
    }) else {
        return unavailable("no_successful_no_input_reference");
    };
    let reference = match facts(reference_row) {
        Ok(facts) => facts,
        Err(reason) => {
            return json!({
                "status": "unavailable",
                "reason": "invalid_reference",
                "reference_index": reference_index,
                "reference_reason": reason,
                "comparisons": [],
            });
        }
    };
    let comparisons = rows
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != reference_index)
        .map(|(index, row)| compare_row(index, row, reference_index, &reference))
        .collect::<Vec<_>>();
    json!({
        "status": "complete",
        "reference": identity(reference_index, &reference),
        "comparisons": comparisons,
        "limitations": "Exact replay evidence only. It does not establish a causal input change, song, role, natural end, loop, or input-step to PCM-frame mapping.",
    })
}

struct Facts<'a> {
    plan: &'a str,
    archive_sha256: &'a str,
    trace_sha256: &'a str,
    context: &'a Value,
    requested_steps: u64,
    actual_steps: u64,
    sample_rate: u64,
    frames: u64,
    pcm_sha256: &'a str,
    activity_intervals: &'a Value,
    active_frames: u64,
    silent: bool,
}

fn compare_row(index: usize, row: &Value, reference_index: usize, reference: &Facts<'_>) -> Value {
    if row["status"] != "success" {
        return incomparable(index, row, "plan_not_successful");
    }
    let plan = match facts(row) {
        Ok(facts) => facts,
        Err(reason) => return incomparable(index, row, reason),
    };
    if plan.requested_steps != reference.requested_steps
        || plan.actual_steps != reference.actual_steps
    {
        return incomparable_with_id(index, &plan, "different_step_count");
    }
    if comparable_context(plan.context) != comparable_context(reference.context) {
        return incomparable_with_id(index, &plan, "different_context");
    }
    if plan.sample_rate != reference.sample_rate {
        return incomparable_with_id(index, &plan, "different_sample_rate");
    }
    if plan.frames != reference.frames {
        return incomparable_with_id(index, &plan, "different_pcm_frames");
    }
    if plan.pcm_sha256 == reference.pcm_sha256
        && (plan.activity_intervals != reference.activity_intervals
            || plan.active_frames != reference.active_frames
            || plan.silent != reference.silent)
    {
        return incomparable_with_id(index, &plan, "inconsistent_pcm_evidence");
    }
    json!({
        "status": "compared",
        "reference": identity(reference_index, reference),
        "plan": identity(index, &plan),
        "pcm": if plan.pcm_sha256 == reference.pcm_sha256 { "same_pcm" } else { "different_pcm" },
        "activity_spans_equal": plan.activity_intervals == reference.activity_intervals,
        "reference_activity": activity(reference),
        "plan_activity": activity(&plan),
        "active_frames_delta": signed_delta(reference.active_frames, plan.active_frames),
    })
}

fn facts(row: &Value) -> Result<Facts<'_>, &'static str> {
    let plan = nonempty_string(row.get("plan")).ok_or("invalid_plan_identity")?;
    let requested_steps = row["requested_steps"]
        .as_u64()
        .ok_or("invalid_step_count")?;
    if !(1..=3_600).contains(&requested_steps) {
        return Err("invalid_step_count");
    }
    let archive_sha256 =
        hash(row.pointer("/capture/archive_sha256")).ok_or("invalid_capture_identity")?;
    let trace_sha256 =
        hash(row.pointer("/capture/trace_sha256")).ok_or("invalid_capture_identity")?;
    let context = row.pointer("/capture/context").ok_or("invalid_context")?;
    valid_context(context, requested_steps)?;
    if row
        .pointer("/applied_input/player_1")
        .filter(|value| value.is_array())
        != context.pointer("/input/player_1")
    {
        return Err("invalid_input_context");
    }
    if row.pointer("/validation/capture_manifest/context") != Some(context)
        || hash(row.pointer("/validation/archive_sha256")) != Some(archive_sha256)
        || hash(row.pointer("/validation/trace_sha256")) != Some(trace_sha256)
    {
        return Err("invalid_validation_identity");
    }
    if row.pointer("/validation/status").and_then(Value::as_str) != Some("integrity_verified")
        || row
            .pointer("/validation/playback/status")
            .and_then(Value::as_str)
            != Some("rendered")
        || row
            .pointer("/validation/playback/evidence/fresh_render_matches")
            .and_then(Value::as_bool)
            != Some(true)
        || row
            .pointer("/validation/playback/evidence/reset_render_matches")
            .and_then(Value::as_bool)
            != Some(true)
        || row
            .pointer("/validation/playback/evidence/validation_duration_capped")
            .and_then(Value::as_bool)
            != Some(false)
        || row
            .pointer("/validation/native_reference/status")
            .and_then(Value::as_str)
            != Some("matched")
        || row
            .pointer("/validation/native_reference/evidence/projected_pcm_matches")
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Err("incomplete_validation");
    }
    let evidence = row
        .pointer("/validation/playback/evidence")
        .ok_or("invalid_pcm_evidence")?;
    let pcm = evidence.pointer("/pcm").ok_or("invalid_pcm_evidence")?;
    let sample_rate = pcm["sample_rate"].as_u64().ok_or("invalid_pcm_evidence")?;
    let frames = pcm["frames"].as_u64().ok_or("invalid_pcm_evidence")?;
    if !matches!(sample_rate, 44_100 | 48_000 | 63_072 | 96_000)
        || !(1..=sample_rate * 120).contains(&frames)
    {
        return Err("invalid_pcm_evidence");
    }
    let pcm_sha256 = hash(pcm.get("pcm_sha256")).ok_or("invalid_pcm_evidence")?;
    let activity_intervals = pcm
        .get("activity_intervals")
        .filter(|value| value.is_array())
        .ok_or("invalid_pcm_evidence")?;
    let silent = evidence["silent"].as_bool().ok_or("invalid_pcm_evidence")?;
    let interval_width =
        interval_width(activity_intervals, frames).ok_or("invalid_pcm_evidence")?;
    if pcm["activity_intervals_truncated"] != Value::Bool(false)
        || evidence["session_duration_frames"].as_u64() != Some(frames)
        || evidence["silence_threshold_i16"].as_u64() != Some(8)
        || evidence["activity_gap_frames"].as_u64() != Some(sample_rate * 3 / 4)
        || !valid_intervals(activity_intervals, frames)
    {
        return Err("invalid_pcm_evidence");
    }
    let active_frames = pcm["active_frames"]
        .as_u64()
        .ok_or("invalid_pcm_evidence")?;
    if silent != (active_frames == 0)
        || active_frames > interval_width
        || (silent && !activity_intervals.as_array().is_some_and(Vec::is_empty))
        || (!silent && activity_intervals.as_array().is_some_and(Vec::is_empty))
    {
        return Err("invalid_pcm_evidence");
    }
    let native_pcm = row
        .pointer("/validation/native_reference/evidence/pcm")
        .ok_or("invalid_pcm_evidence")?;
    if native_pcm != pcm {
        return Err("invalid_pcm_evidence");
    }
    Ok(Facts {
        plan,
        archive_sha256,
        trace_sha256,
        context,
        requested_steps,
        actual_steps: context["frames_run"].as_u64().ok_or("invalid_step_count")?,
        sample_rate,
        frames,
        pcm_sha256,
        activity_intervals,
        active_frames,
        silent,
    })
}

pub(super) fn valid_row(row: &Value) -> bool {
    facts(row).is_ok()
}

fn valid_context(context: &Value, requested_steps: u64) -> Result<(), &'static str> {
    let source = context
        .get("source")
        .filter(|value| value.is_object())
        .ok_or("invalid_context")?;
    let requested = source
        .get("requested_file")
        .filter(|value| value.is_object())
        .ok_or("invalid_context")?;
    let loaded = source
        .get("loaded_media")
        .filter(|value| value.is_object())
        .ok_or("invalid_context")?;
    if nonempty_string(context.get("system")).is_none()
        || !context.get("settings").is_some_and(Value::is_object)
        || hash(requested.get("sha256")).is_none()
        || !positive_u64(requested.get("byte_len"))
        || hash(loaded.get("sha256")).is_none()
        || !positive_u64(loaded.get("byte_len"))
        || !context.get("firmware").is_some_and(valid_firmware)
        || context["persistent_save_files"] != "not_loaded_or_written"
        || context["sample_generation"] != true
        || context["requested_frames"].as_u64() != Some(requested_steps)
        || context["frames_run"].as_u64() != Some(requested_steps)
        || !context
            .pointer("/input/player_1")
            .is_some_and(Value::is_array)
        || !context
            .pointer("/input/player_2")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
        || ["player_3", "player_4", "player_5"].iter().any(|player| {
            context["input"]
                .get(*player)
                .is_some_and(|value| value.as_array().is_none_or(|events| !events.is_empty()))
        })
    {
        return Err("invalid_context");
    }
    Ok(())
}

fn valid_firmware(value: &Value) -> bool {
    value.is_null()
        || (value.is_object()
            && nonempty_string(value.get("firmware_id")).is_some()
            && hash(value.get("sha256")).is_some()
            && positive_u64(value.get("byte_len")))
}

fn valid_intervals(value: &Value, frames: u64) -> bool {
    interval_width(value, frames).is_some()
}

fn interval_width(value: &Value, frames: u64) -> Option<u64> {
    let intervals = value.as_array()?;
    if intervals.len() > 256 {
        return None;
    }
    let mut previous_end = 0;
    let mut total = 0u64;
    for interval in intervals {
        let (Some(start), Some(end)) = (
            interval.get("start_frame").and_then(Value::as_u64),
            interval.get("end_frame").and_then(Value::as_u64),
        ) else {
            return None;
        };
        if start < previous_end || start >= end || end > frames {
            return None;
        }
        total = total.checked_add(end - start)?;
        previous_end = end;
    }
    Some(total)
}

fn positive_u64(value: Option<&Value>) -> bool {
    value.and_then(Value::as_u64).is_some_and(|value| value > 0)
}

fn comparable_context(context: &Value) -> Option<Value> {
    let mut context = context.clone();
    let fields = context.as_object_mut()?;
    fields.remove("input")?;
    fields.remove("requested_frames")?;
    fields.remove("frames_run")?;
    Some(context)
}

fn identity(index: usize, facts: &Facts<'_>) -> Value {
    json!({
        "index": index,
        "plan": facts.plan,
        "archive_sha256": facts.archive_sha256,
        "trace_sha256": facts.trace_sha256,
        "requested_steps": facts.requested_steps,
        "actual_steps": facts.actual_steps,
    })
}

fn activity(facts: &Facts<'_>) -> Value {
    json!({"silent": facts.silent, "active_frames": facts.active_frames})
}

fn signed_delta(reference: u64, plan: u64) -> Option<i64> {
    let reference = i64::try_from(reference).ok()?;
    let plan = i64::try_from(plan).ok()?;
    plan.checked_sub(reference)
}

fn hash(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str).filter(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn nonempty_string(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

fn unavailable(reason: &'static str) -> Value {
    json!({"status": "unavailable", "reason": reason, "comparisons": []})
}

fn incomparable(index: usize, row: &Value, reason: &'static str) -> Value {
    json!({
        "status": "incomparable",
        "reason": reason,
        "plan": {"index": index, "plan": row["plan"]},
    })
}

fn incomparable_with_id(index: usize, plan: &Facts<'_>, reason: &'static str) -> Value {
    json!({
        "status": "incomparable",
        "reason": reason,
        "plan": identity(index, plan),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(value: char) -> String {
        value.to_string().repeat(64)
    }

    fn row(name: &str, input: Value, pcm: &str) -> Value {
        let archive = hash('a');
        let trace = hash('b');
        let context = json!({
            "system":"sms", "settings":{"video_standard":"ntsc"}, "firmware":null,
            "persistent_save_files":"not_loaded_or_written", "sample_generation":true,
            "requested_frames":720, "frames_run":720,
            "input":{"player_1":input,"player_2":[]},
            "source":{"requested_file":{"sha256":hash('c'),"byte_len":1},
                "loaded_media":{"sha256":hash('d'),"byte_len":1}}
        });
        let pcm = json!({
            "sample_rate":48000,"frames":12,"pcm_sha256":pcm,
            "active_frames":5,"activity_intervals":[{"start_frame":0,"end_frame":5}],"activity_intervals_truncated":false
        });
        json!({
            "plan":name,"requested_steps":720,"status":"success",
            "applied_input":{"player_1":context["input"]["player_1"]},
            "capture":{"archive_sha256":archive,"trace_sha256":trace,"context":context},
            "validation":{"status":"integrity_verified","archive_sha256":hash('a'),"trace_sha256":hash('b'),
                "capture_manifest":{"context":context},
                "playback":{"status":"rendered","evidence":{"fresh_render_matches":true,"reset_render_matches":true,
                    "validation_duration_capped":false,"session_duration_frames":12,"silent":false,
                    "silence_threshold_i16":8,"activity_gap_frames":36000,"pcm":pcm}},
                "native_reference":{"status":"matched","evidence":{"projected_pcm_matches":true,"pcm":pcm}}}
        })
    }

    #[test]
    fn compares_identical_changed_and_silent_rows() {
        let reference = row("baseline", json!([]), &hash('e'));
        let identical = row("repeat", json!([{"buttons":8}]), &hash('e'));
        let mut changed = row("changed", json!([{"buttons":1}]), &hash('f'));
        changed["validation"]["playback"]["evidence"]["pcm"]["active_frames"] = json!(8);
        changed["validation"]["playback"]["evidence"]["pcm"]["activity_intervals"] =
            json!([{"start_frame":1,"end_frame":9}]);
        changed["validation"]["native_reference"]["evidence"]["pcm"]["active_frames"] = json!(8);
        changed["validation"]["native_reference"]["evidence"]["pcm"]["activity_intervals"] =
            json!([{"start_frame":1,"end_frame":9}]);
        let result = compare(&[reference, identical, changed]);
        assert_eq!(result["status"], "complete");
        assert_eq!(result["comparisons"][0]["pcm"], "same_pcm");
        assert_eq!(result["comparisons"][1]["pcm"], "different_pcm");
        assert_eq!(result["comparisons"][1]["activity_spans_equal"], false);
        assert_eq!(result["comparisons"][1]["active_frames_delta"], 3);
        let mut inconsistent = row("inconsistent", json!([{"buttons":2}]), &hash('e'));
        inconsistent["validation"]["playback"]["evidence"]["pcm"]["active_frames"] = json!(6);
        inconsistent["validation"]["playback"]["evidence"]["pcm"]["activity_intervals"] =
            json!([{"start_frame":0,"end_frame":6}]);
        inconsistent["validation"]["native_reference"]["evidence"]["pcm"]["active_frames"] =
            json!(6);
        inconsistent["validation"]["native_reference"]["evidence"]["pcm"]["activity_intervals"] =
            json!([{"start_frame":0,"end_frame":6}]);
        assert_eq!(
            compare(&[row("baseline", json!([]), &hash('e')), inconsistent])["comparisons"][0]["reason"],
            "inconsistent_pcm_evidence"
        );
        let mut silent = row("silent", json!([{"buttons":2}]), &hash('e'));
        silent["validation"]["playback"]["evidence"]["silent"] = json!(true);
        silent["validation"]["playback"]["evidence"]["pcm"]["active_frames"] = json!(0);
        silent["validation"]["playback"]["evidence"]["pcm"]["activity_intervals"] = json!([]);
        silent["validation"]["native_reference"]["evidence"]["pcm"]["active_frames"] = json!(0);
        silent["validation"]["native_reference"]["evidence"]["pcm"]["activity_intervals"] =
            json!([]);
        let mut silent_reference = row("baseline", json!([]), &hash('e'));
        silent_reference["validation"]["playback"]["evidence"]["silent"] = json!(true);
        silent_reference["validation"]["playback"]["evidence"]["pcm"]["active_frames"] = json!(0);
        silent_reference["validation"]["playback"]["evidence"]["pcm"]["activity_intervals"] =
            json!([]);
        silent_reference["validation"]["native_reference"]["evidence"]["pcm"]["active_frames"] =
            json!(0);
        silent_reference["validation"]["native_reference"]["evidence"]["pcm"]["activity_intervals"] =
            json!([]);
        assert_eq!(
            compare(&[silent_reference, silent])["comparisons"][0]["plan_activity"]["silent"],
            true
        );
    }

    #[test]
    fn rejects_missing_reference_failed_rows_and_incomplete_evidence() {
        assert_eq!(compare(&[])["reason"], "no_successful_no_input_reference");
        let mut failed = row("failed", json!([{"buttons":8}]), &hash('e'));
        failed["status"] = json!("capture_failed");
        let result = compare(&[row("baseline", json!([]), &hash('e')), failed]);
        assert_eq!(result["comparisons"][0]["reason"], "plan_not_successful");
        let mut malformed = row("baseline", json!([]), &hash('e'));
        malformed["capture"]["context"]["source"]["loaded_media"] = Value::Null;
        assert_eq!(compare(&[malformed])["reason"], "invalid_reference");
    }

    #[test]
    fn source_identity_and_malformed_activity_are_incomparable() {
        let reference = row("baseline", json!([]), &hash('e'));
        let mut source = row("source", json!([{"buttons":8}]), &hash('e'));
        source["capture"]["context"]["source"]["loaded_media"]["sha256"] = json!(hash('f'));
        source["validation"]["capture_manifest"]["context"] = source["capture"]["context"].clone();
        let mut activity = row("activity", json!([{"buttons":1}]), &hash('e'));
        activity["validation"]["playback"]["evidence"]["pcm"]["activity_intervals"] =
            json!([{"start_frame":4,"end_frame":3}]);
        activity["validation"]["native_reference"]["evidence"]["pcm"]["activity_intervals"] =
            json!([{"start_frame":4,"end_frame":3}]);
        let result = compare(&[reference, source, activity]);
        assert_eq!(result["comparisons"][0]["reason"], "different_context");
        assert_eq!(result["comparisons"][1]["reason"], "invalid_pcm_evidence");
    }

    #[test]
    fn input_binding_and_activity_policy_are_required() {
        let reference = row("baseline", json!([]), &hash('e'));
        let mut input = row("input", json!([{"buttons":8}]), &hash('e'));
        input["applied_input"]["player_1"] = json!([]);
        let mut policy = row("policy", json!([{"buttons":1}]), &hash('f'));
        policy["validation"]["playback"]["evidence"]["silence_threshold_i16"] = json!(7);
        let result = compare(&[reference, input, policy]);
        assert_eq!(result["comparisons"][0]["reason"], "invalid_input_context");
        assert_eq!(result["comparisons"][1]["reason"], "invalid_pcm_evidence");
    }

    #[test]
    fn separates_context_pcm_shape_and_validation_contracts() {
        let reference = row("baseline", json!([]), &hash('e'));
        let mut context = row("context", json!([{"buttons":8}]), &hash('e'));
        context["capture"]["context"]["firmware"] =
            json!({"firmware_id":"bios","sha256":hash('f'),"byte_len":1});
        context["validation"]["capture_manifest"]["context"]["firmware"] =
            context["capture"]["context"]["firmware"].clone();
        let mut rate = row("rate", json!([{"buttons":1}]), &hash('e'));
        rate["validation"]["playback"]["evidence"]["pcm"]["sample_rate"] = json!(44100);
        rate["validation"]["playback"]["evidence"]["activity_gap_frames"] = json!(33075);
        rate["validation"]["native_reference"]["evidence"]["pcm"]["sample_rate"] = json!(44100);
        let mut steps = row("steps", json!([{"buttons":2}]), &hash('e'));
        steps["requested_steps"] = json!(721);
        steps["capture"]["context"]["requested_frames"] = json!(721);
        steps["capture"]["context"]["frames_run"] = json!(721);
        steps["validation"]["capture_manifest"]["context"] = steps["capture"]["context"].clone();
        let mut duration = row("duration", json!([{"buttons":4}]), &hash('e'));
        duration["validation"]["playback"]["evidence"]["pcm"]["frames"] = json!(13);
        duration["validation"]["playback"]["evidence"]["session_duration_frames"] = json!(13);
        duration["validation"]["native_reference"]["evidence"]["pcm"]["frames"] = json!(13);
        let mut truncated = row("truncated", json!([{"buttons":3}]), &hash('e'));
        truncated["validation"]["playback"]["evidence"]["pcm"]["activity_intervals_truncated"] =
            json!(true);
        truncated["validation"]["native_reference"]["evidence"]["pcm"]["activity_intervals_truncated"] =
            json!(true);
        let result = compare(&[reference, context, rate, steps, duration, truncated]);
        assert_eq!(result["comparisons"][0]["reason"], "different_context");
        assert_eq!(result["comparisons"][1]["reason"], "different_sample_rate");
        assert_eq!(result["comparisons"][2]["reason"], "different_step_count");
        assert_eq!(result["comparisons"][3]["reason"], "different_pcm_frames");
        assert_eq!(result["comparisons"][4]["reason"], "invalid_pcm_evidence");
    }

    #[test]
    fn enforces_the_row_bound() {
        let rows = (0..=MAX_ROWS)
            .map(|index| row(&format!("plan{index}"), json!([]), &hash('e')))
            .collect::<Vec<_>>();
        assert_eq!(compare(&rows)["reason"], "too_many_rows");
    }
}
