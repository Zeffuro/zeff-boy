use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};

const MAX_DOCUMENT_BYTES: u64 = 64 * 1024;
const MAX_PLANS: usize = 16;
const MAX_STEPS: u64 = 3_600;
const MAX_TOTAL_STEPS: u64 = 14_400;
const MAX_PRESS_BYTES: usize = 8_192;
const MAX_EVENTS: usize = 128;

pub(super) struct Plan {
    pub(super) name: String,
    pub(super) steps: u64,
    pub(super) press: Option<String>,
    pub(super) events: Value,
}

pub(super) struct Document {
    pub(super) sha256: String,
    pub(super) plans: Vec<Plan>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDocument {
    schema: String,
    plans: Vec<RawPlan>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPlan {
    name: String,
    steps: u64,
    press: Option<String>,
}

pub(super) fn defaults() -> Vec<Plan> {
    vec![
        Plan {
            name: "baseline".to_owned(),
            steps: 720,
            press: None,
            events: json!([]),
        },
        Plan {
            name: "start".to_owned(),
            steps: 720,
            press: Some("start@180-181".to_owned()),
            events: json!([input_event(180, 181, 0x08)]),
        },
        Plan {
            name: "start_then_a".to_owned(),
            steps: 720,
            press: Some("start@180-181,a@300-301".to_owned()),
            events: json!([input_event(180, 181, 0x08), input_event(300, 301, 0x01)]),
        },
    ]
}

pub(super) fn load(path: &Path) -> Result<Document> {
    let bytes = read_bounded(path)?;
    let raw: RawDocument = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid audio capture plans document {}", path.display()))?;
    ensure!(
        raw.schema == "zeff-audio-capture-plans/1",
        "audio capture plans document has an unsupported schema"
    );
    ensure!(
        (1..=MAX_PLANS).contains(&raw.plans.len()),
        "audio capture plans document requires 1..={MAX_PLANS} plans"
    );
    let mut names = std::collections::BTreeSet::new();
    let mut total_steps = 0u64;
    let mut plans = Vec::with_capacity(raw.plans.len());
    for raw_plan in raw.plans {
        ensure!(valid_name(&raw_plan.name), "invalid capture plan name");
        ensure!(
            names.insert(raw_plan.name.clone()),
            "capture plan names must be unique"
        );
        ensure!(
            (1..=MAX_STEPS).contains(&raw_plan.steps),
            "capture plan steps must be 1..={MAX_STEPS}"
        );
        total_steps = total_steps
            .checked_add(raw_plan.steps)
            .context("capture plan step total overflowed")?;
        ensure!(
            total_steps <= MAX_TOTAL_STEPS,
            "capture plan step total must be at most {MAX_TOTAL_STEPS}"
        );
        let events = match &raw_plan.press {
            Some(press) => parse_events(press, raw_plan.steps)?,
            None => json!([]),
        };
        plans.push(Plan {
            name: raw_plan.name,
            steps: raw_plan.steps,
            press: raw_plan.press,
            events,
        });
    }
    Ok(Document {
        sha256: zeff_firmware::sha256_hex(&bytes),
        plans,
    })
}

pub(super) fn schedule_json(plans: &[Plan]) -> Value {
    Value::Array(
        plans
            .iter()
            .map(|plan| {
                json!({
                    "name": &plan.name,
                    "requested_steps": plan.steps,
                    "press": &plan.press,
                    "player_1": &plan.events,
                })
            })
            .collect(),
    )
}

fn parse_events(press: &str, steps: u64) -> Result<Value> {
    ensure!(
        press.len() <= MAX_PRESS_BYTES,
        "capture plan press string exceeds {MAX_PRESS_BYTES} bytes"
    );
    let events = crate::cli::parse::parse_input_event_arg(press, "capture plan press")?;
    ensure!(
        events.len() <= MAX_EVENTS,
        "capture plan press has more than {MAX_EVENTS} events"
    );
    for event in &events {
        ensure!(
            !event.reset,
            "capture plan press cannot include reset events"
        );
        ensure!(
            event.start_frame >= 1
                && event.start_frame <= event.end_frame
                && event.end_frame <= steps,
            "capture plan input event is outside 1..={steps}"
        );
    }
    serde_json::to_value(events).context("could not serialize capture plan input events")
}

fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let file = File::open(path).with_context(|| {
        format!(
            "failed to open audio capture plans document {}",
            path.display()
        )
    })?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file(),
        "audio capture plans document must be a regular file"
    );
    ensure!(
        metadata.len() <= MAX_DOCUMENT_BYTES,
        "audio capture plans document exceeds {MAX_DOCUMENT_BYTES} bytes"
    );
    let expected_len = metadata.len();
    let mut bytes = Vec::with_capacity(usize::try_from(expected_len)?);
    file.take(MAX_DOCUMENT_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 == expected_len,
        "audio capture plans document exceeds its declared bounds"
    );
    Ok(bytes)
}

fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=32).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
        && !windows_device_name(name)
}

fn windows_device_name(name: &str) -> bool {
    matches!(name, "con" | "prn" | "aux" | "nul")
        || name
            .strip_prefix("com")
            .or_else(|| name.strip_prefix("lpt"))
            .is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

fn input_event(start_frame: u64, end_frame: u64, buttons: u8) -> Value {
    json!({
        "start_frame": start_frame,
        "end_frame": end_frame,
        "buttons": buttons,
        "dpad": 0,
        "coleco_keypad": null,
        "reset": false,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn temporary_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "zeff-audio-plans-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn load_document(value: Value) -> Result<Document> {
        let path = temporary_path();
        std::fs::write(&path, serde_json::to_vec(&value)?)?;
        let result = load(&path);
        std::fs::remove_file(path)?;
        result
    }

    #[test]
    fn custom_document_uses_the_headless_input_parser_and_schedule() -> Result<()> {
        let document = load_document(json!({
            "schema": "zeff-audio-capture-plans/1",
            "plans": [{"name": "select_start", "steps": 720, "press": "select+start@180-181"}]
        }))?;
        assert_eq!(
            document.plans[0].events,
            json!([input_event(180, 181, 0x0c)])
        );
        assert_eq!(
            schedule_json(&document.plans),
            json!([{"name":"select_start","requested_steps":720,"press":"select+start@180-181","player_1":[input_event(180,181,0x0c)]}])
        );
        assert_eq!(document.sha256.len(), 64);
        Ok(())
    }

    #[test]
    fn custom_document_retains_parser_order_and_accepts_the_final_step_index() -> Result<()> {
        let document = load_document(json!({
            "schema": "zeff-audio-capture-plans/1",
            "plans": [{
                "name": "keypad_then_a", "steps": 3,
                "press": "up+right+1@1-1,a@2-2"
            }, {
                "name": "last", "steps": 720, "press": "start@720-720"
            }]
        }))?;
        assert_eq!(
            document.plans[0].events,
            json!([
                {"start_frame":1,"end_frame":1,"buttons":0,"dpad":5,"coleco_keypad":1,"reset":false},
                {"start_frame":2,"end_frame":2,"buttons":1,"dpad":0,"coleco_keypad":null,"reset":false}
            ])
        );
        assert_eq!(document.plans[1].events[0]["end_frame"], 720);
        Ok(())
    }

    #[test]
    fn custom_document_accepts_all_bounded_plan_and_event_slots() -> Result<()> {
        let plans = (0..MAX_PLANS)
            .map(|index| json!({"name":format!("plan{index}"),"steps":1}))
            .collect::<Vec<_>>();
        assert!(
            load_document(json!({
                "schema":"zeff-audio-capture-plans/1", "plans":plans
            }))
            .is_ok()
        );
        assert!(
            load_document(json!({
                "schema":"zeff-audio-capture-plans/1",
                "plans":[{"name":"maximum","steps":3600}]
            }))
            .is_ok()
        );
        let press = (0..MAX_EVENTS)
            .map(|_| "a@1-1")
            .collect::<Vec<_>>()
            .join(",");
        let document = load_document(json!({
            "schema":"zeff-audio-capture-plans/1",
            "plans":[{"name":"events","steps":2,"press":press}]
        }))?;
        assert_eq!(
            document.plans[0].events.as_array().unwrap().len(),
            MAX_EVENTS
        );
        Ok(())
    }

    #[test]
    fn custom_document_rejects_unsafe_names_ranges_resets_and_unknown_fields() -> Result<()> {
        for plan in [
            json!({"name":"CON","steps":1}),
            json!({"name":"con","steps":1}),
            json!({"name":"com1","steps":1}),
            json!({"name":"good","steps":720,"press":"a@721-721"}),
            json!({"name":"good","steps":720,"press":"a@0-1"}),
            json!({"name":"good","steps":720,"press":"reset@1-1"}),
            json!({"name":"good","steps":0}),
            json!({"name":"good","steps":1,"extra":true}),
        ] {
            assert!(
                load_document(json!({"schema":"zeff-audio-capture-plans/1","plans":[plan]}))
                    .is_err()
            );
        }
        assert!(load_document(json!({"schema":"other","plans":[]})).is_err());
        assert!(
            load_document(json!({
                "schema":"zeff-audio-capture-plans/1", "plans":[]
            }))
            .is_err()
        );
        assert!(
            load_document(json!({
                "schema":"zeff-audio-capture-plans/1",
                "plans":[{"name":"same","steps":1},{"name":"same","steps":1}]
            }))
            .is_err()
        );
        let many_events = (0..=MAX_EVENTS)
            .map(|_| "a@1-1")
            .collect::<Vec<_>>()
            .join(",");
        assert!(
            load_document(json!({
                "schema":"zeff-audio-capture-plans/1",
                "plans":[{"name":"good","steps":1,"press":many_events}]
            }))
            .is_err()
        );
        let too_long = "a".repeat(MAX_PRESS_BYTES + 1);
        assert!(
            load_document(json!({
                "schema":"zeff-audio-capture-plans/1",
                "plans":[{"name":"good","steps":1,"press":too_long}]
            }))
            .is_err()
        );
        assert!(
            load_document(json!({
                "schema":"zeff-audio-capture-plans/1",
                "plans":[
                    {"name":"one","steps":3600}, {"name":"two","steps":3600},
                    {"name":"three","steps":3600}, {"name":"four","steps":3600},
                    {"name":"five","steps":1}
                ]
            }))
            .is_err()
        );
        let too_many_plans = (0..=MAX_PLANS)
            .map(|index| json!({"name":format!("plan{index}"),"steps":1}))
            .collect::<Vec<_>>();
        assert!(
            load_document(json!({
                "schema":"zeff-audio-capture-plans/1", "plans":too_many_plans
            }))
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn custom_document_file_bound_is_enforced_before_parsing() -> Result<()> {
        let path = temporary_path();
        std::fs::write(&path, vec![b' '; MAX_DOCUMENT_BYTES as usize + 1])?;
        let result = load(&path);
        std::fs::remove_file(path)?;
        assert!(result.is_err());
        Ok(())
    }
}
