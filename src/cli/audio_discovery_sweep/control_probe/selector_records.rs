use serde_json::{Value, json};

use super::super::nes_source::Nrom;

pub(super) fn analyze(source: &[u8], control: &Value) -> Value {
    let Some(nrom) = Nrom::parse(source) else {
        return json!([]);
    };
    let Some(callers) = control["caller_argument_reads"].as_array() else {
        return json!([]);
    };
    let Some(extents) = control["record_extents"].as_array() else {
        return json!([]);
    };
    let mut results = Vec::new();
    for (caller_index, caller) in callers.iter().enumerate() {
        for (extent_index, extent) in extents.iter().enumerate() {
            if caller["entry_argument_read_index"] != extent["entry_argument_read_index"] {
                continue;
            }
            if let Some(records) = candidates(source, nrom, caller, extent) {
                results.push(json!({
                    "caller_argument_read_index": caller_index, "record_extent_index": extent_index,
                    "qualification": "conditional_writer_path_candidates",
                    "global_selector_domain_proven": false, "playback_proven": false,
                    "records": records,
                }));
            }
        }
    }
    json!(results)
}

fn candidates(source: &[u8], nrom: Nrom, caller: &Value, extent: &Value) -> Option<Vec<Value>> {
    let constraint = &caller["ram_writer"]["value_constraint"];
    if constraint["scope"] != "writer_path_only" {
        return None;
    }
    let values = constraint["values"].as_array()?;
    if values.is_empty() || values.len() > 16 {
        return None;
    }
    let transforms = caller["transforms"].as_array()?;
    let length = u8::try_from(extent["source"]["length"].as_u64()?).ok()?;
    if !(1..=16).contains(&length) {
        return None;
    }
    let table = u16::try_from(extent["source"]["table"].as_u64()?).ok()?;
    let observed = u8::try_from(caller["ram_read"]["value"].as_u64()?).ok()?;
    let mut records = Vec::new();
    let mut last_end = None;
    let mut seen_observed = false;
    for value in values {
        let selector = u8::try_from(value.as_u64()?).ok()?;
        let mut index = selector;
        for transform in transforms {
            if transform["operation"] != "asl_a" {
                return None;
            }
            index = index.checked_mul(2)?;
        }
        index.checked_add(length - 1)?;
        let address = table.checked_add(u16::from(index))?;
        let start = nrom.offset_for(address)?;
        let end = start.checked_add(u64::from(length))?;
        if last_end.is_some_and(|last| last > start) {
            return None;
        }
        last_end = Some(end);
        for delta in 0..length {
            if nrom.offset_for(address.checked_add(u16::from(delta))?)? != start + u64::from(delta)
            {
                return None;
            }
        }
        let bytes = source.get(usize::try_from(start).ok()?..usize::try_from(end).ok()?)?;
        let was_observed = selector == observed;
        if was_observed {
            if caller["argument"]["value"].as_u64()? != u64::from(index)
                || extent["source"]["start"].as_u64()? != start
            {
                return None;
            }
            seen_observed = true;
        }
        records.push(
            json!({"selector_value": selector, "index": index, "address": address,
            "source_offset": start, "length": length, "bytes": const_hex::encode(bytes),
            "observed_in_this_call": was_observed}),
        );
    }
    seen_observed.then_some(records)
}
