use super::*;
use device::Device;
use model::{Case, Outcome};
use value::Value;

fn count(node: &Value) -> usize {
    node.number().try_into().unwrap()
}

fn memory(node: &Value) -> BTreeMap<u32, u16> {
    let words = decode::words(node);
    let result: BTreeMap<_, _> = words.iter().copied().collect();
    assert_eq!(result.len(), words.len());
    result
}

pub(super) fn verify(device: &Device, case: &Case) -> bool {
    let image = case.raw.get("expected").get("device_image");
    let clock = image.get("clock_u64").number();
    let words = memory(image.get("word_RAM"));
    let events = decode::events(image.get("events"));
    let counts = [
        "instruction_transfers",
        "instruction_detectors",
        "entry_transfers",
        "entry_detectors",
        "idles",
    ]
    .map(|name| count(image.get("callback_counts").get(name)));
    let before_entry = image.get("before_entry").optional().map(|entry| {
        (
            entry.get("clock_u64").number(),
            memory(entry.get("word_RAM")),
        )
    });
    let script: Vec<(u32, Outcome, bool)> = image
        .get("script")
        .list()
        .iter()
        .map(|report| {
            (
                report.get("elapsed").u32(),
                decode::outcome(report.get("outcome")),
                report.get("word_deposited").flag(),
            )
        })
        .collect();
    let next = count(image.get("next"));
    let instruction_next = count(image.get("instruction_next"));
    let entry_event = image.get("entry_event").optional().map(count);
    let mutations: Vec<(usize, u32, u16)> = image
        .get("mutations")
        .list()
        .iter()
        .map(|change| {
            (
                count(change.get("instruction_callback")),
                change.get("address").u32(),
                change.get("value").u16(),
            )
        })
        .collect();
    let actual = format!("{device:?}");
    let expected = format!(
        "Device {{ clock: {clock}, words: {words:?}, events: {events:?}, counts: {counts:?}, before_entry: {before_entry:?}, script: {script:?}, next: {next}, instruction_next: {instruction_next}, entry_event: {entry_event:?}, mutations: {mutations:?} }}"
    );
    [
        "Device { clock: ",
        ", words: ",
        ", events: ",
        ", counts: ",
        ", before_entry: ",
        ", script: ",
        ", next: ",
        ", instruction_next: ",
        ", entry_event: ",
        ", mutations: ",
    ]
    .into_iter()
    .all(|field| actual.matches(field).count() == 1)
        && actual == expected
        && image.get("reports_consumed").flag()
        && next == script.len()
        && device.reports_consumed()
}
