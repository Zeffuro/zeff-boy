use serde_json::{Value, json};

use super::analyze;

#[test]
fn proves_a_complete_observed_record() {
    let source = source();
    let result = analyze(&source, &control(&source), &[]);
    let extent = &result.as_array().unwrap()[0];
    assert_eq!(result.as_array().unwrap().len(), 1);
    assert_eq!(extent["entry_argument_read_index"], 0);
    assert_eq!(extent["event_indices"], json!([10, 11, 12, 13]));
    assert_eq!(
        extent["source"],
        json!({
            "table": 0x8100, "address": 0x8102, "start": offset(0x8102),
            "length": 4, "values": "11223344",
        })
    );
    assert_eq!(extent["instruction_contract"]["bne"]["target"], 0x8002);
}

#[test]
fn rejects_malformed_partial_mismatched_and_wrapping_evidence() {
    let source = source();
    let mut partial = control(&source);
    partial["observations"].as_array_mut().unwrap().pop();
    assert_eq!(analyze(&source, &partial, &[]), json!([]));

    let mut mismatch = control(&source);
    mismatch["observations"][2]["writer_before"]["a"] = json!(0xff);
    assert_eq!(analyze(&source, &mismatch, &[]), json!([]));

    let mut repeated = control(&source);
    let extra = repeated["observations"][3].clone();
    repeated["observations"].as_array_mut().unwrap().push(extra);
    assert_eq!(analyze(&source, &repeated, &[]), json!([]));

    let mut malformed = source.clone();
    write(&mut malformed, 0x800c, &[0xd0, 0xf3]);
    assert_eq!(analyze(&malformed, &control(&malformed), &[]), json!([]));

    let mut wrapping = source.clone();
    write(&mut wrapping, 0x8002, &[0xb9, 0xff, 0xff]);
    assert_eq!(analyze(&wrapping, &control(&wrapping), &[]), json!([]));
}

#[test]
fn rejects_invalid_counts_and_extra_loop_iterations() {
    let source = source();
    for count in [0, 17] {
        let mut changed = source.clone();
        write(&mut changed, 0x800a, &[0xe0, count]);
        assert_eq!(analyze(&changed, &control(&changed), &[]), json!([]));
    }

    let mut smaller = source.clone();
    write(&mut smaller, 0x800a, &[0xe0, 3]);
    assert_eq!(analyze(&smaller, &control(&smaller), &[]), json!([]));
}

#[test]
fn rejects_wrong_register_entry_cycle_and_event_sequence() {
    let source = source();
    let mut nonzero_x = control(&source);
    nonzero_x["observations"][0]["writer_before"]["x"] = json!(1);
    assert_eq!(analyze(&source, &nonzero_x, &[]), json!([]));

    let mut changed_entry = control(&source);
    changed_entry["observations"][1]["call_path"][0]["entry"]["cycle"] = json!(101);
    assert_eq!(analyze(&source, &changed_entry, &[]), json!([]));

    let mut cycle_order = control(&source);
    cycle_order["observations"][1]["cycle"] = json!(109);
    assert_eq!(analyze(&source, &cycle_order, &[]), json!([]));

    let mut event_gap = control(&source);
    event_gap["observations"][1]["event_index"] = json!(12);
    assert_eq!(analyze(&source, &event_gap, &[]), json!([]));
    for (pointer, value) in [
        ("/entry_argument_reads/0/rom_read/cpu_cycle", Value::Null),
        ("/observations/3/writer_before/cycle", Value::Null),
        ("/observations/3/writer_before/cycle", json!(102)),
    ] {
        let mut changed = control(&source);
        *changed.pointer_mut(pointer).unwrap() = value;
        assert_eq!(analyze(&source, &changed, &[]), json!([]));
    }
}

#[test]
fn rejects_missing_or_reordered_initial_witnesses_and_bad_read_fields() {
    let source = source();
    let mut missing_witness = control(&source);
    missing_witness["entry_argument_reads"][0]["witnesses"] = json!([]);
    assert_eq!(analyze(&source, &missing_witness, &[]), json!([]));

    let mut reordered_witness = control(&source);
    reordered_witness["entry_argument_reads"][0]["witnesses"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    assert_eq!(analyze(&source, &reordered_witness, &[]), json!([]));

    let mut bad_source_offset = control(&source);
    bad_source_offset["entry_argument_reads"][0]["rom_read"]["source_offset"] =
        json!(offset(0x8102) + 1);
    assert_eq!(analyze(&source, &bad_source_offset, &[]), json!([]));

    let mut bad_index = control(&source);
    bad_index["entry_argument_reads"][0]["rom_read"]["index_value"] = json!(3);
    assert_eq!(analyze(&source, &bad_index, &[]), json!([]));

    let mut wrapping_index = control(&source);
    wrapping_index["entry_argument_reads"][0]["rom_read"]["index_value"] = json!(255);
    assert_eq!(analyze(&source, &wrapping_index, &[]), json!([]));
}

#[test]
fn rejects_data_and_loop_crossing_the_sixteen_kib_prg_mirror() {
    let mut data_mirror = source();
    write(&mut data_mirror, 0x8002, &[0xb9, 0xfd, 0xbf]);
    assert_eq!(
        analyze(&data_mirror, &control(&data_mirror), &[]),
        json!([])
    );

    let mut loop_mirror = source();
    write_mapped(
        &mut loop_mirror,
        0xbffe,
        &[
            0xa2, 0x00, 0xb9, 0x00, 0x81, 0x9d, 0x00, 0x40, 0xc8, 0xe8, 0xe0, 4, 0xd0, 0xf4,
        ],
    );
    let mut control = control(&loop_mirror);
    control["entry_argument_reads"][0]["rom_read"]["pc"] = json!(0xc000);
    assert_eq!(analyze(&loop_mirror, &control, &[]), json!([]));
}

#[test]
fn rejects_empty_malformed_and_capped_control_reports() {
    let source = source();
    assert_eq!(analyze(&source, &json!({}), &[]), json!([]));
    assert_eq!(
        analyze(
            &source,
            &json!({"entry_argument_reads": [{}], "observations": [{}]}),
            &[],
        ),
        json!([])
    );
    let capped = json!({
        "entry_argument_reads": vec![Value::Null; 4097],
        "observations": [],
    });
    assert_eq!(analyze(&source, &capped, &[]), json!([]));
}

#[test]
fn rejects_discontinuities_inside_the_observed_record_only() {
    let source = source();
    let control = control(&source);
    for boundary in [103, 104, 120] {
        assert_eq!(analyze(&source, &control, &[boundary]), json!([]));
    }
    for boundary in [102, 121] {
        assert_eq!(
            analyze(&source, &control, &[boundary])
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}

fn source() -> Vec<u8> {
    let mut source = vec![0; 16 + 0x4000];
    source[..16].copy_from_slice(&[b'N', b'E', b'S', 0x1a, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    write(
        &mut source,
        0x8000,
        &[
            0xa2, 0x00, 0xb9, 0x00, 0x81, 0x9d, 0x00, 0x40, 0xc8, 0xe8, 0xe0, 4, 0xd0, 0xf4,
        ],
    );
    source[offset(0x8102) as usize..offset(0x8106) as usize]
        .copy_from_slice(&[0x11, 0x22, 0x33, 0x44]);
    source
}

fn control(source: &[u8]) -> Value {
    let reads = vec![json!({
        "entry": {"pc": 0x8000, "cpu_cycle": 100},
        "event_index": 10,
        "writer_pc": 0x8005,
        "register": 0x4000,
        "rom_read": {
            "pc": 0x8002, "instruction": "b90081",
            "cpu_cycle": 103,
            "index_register": "y", "index_value": 2,
            "address": 0x8102, "source_offset": offset(0x8102),
            "value": source[offset(0x8102) as usize],
        },
        "witnesses": [
            {"pc": 0x8000, "cpu_cycle": 101, "bytes": "a200", "source_offset": offset(0x8000)},
            {"pc": 0x8002, "cpu_cycle": 103, "bytes": "b90081", "source_offset": offset(0x8002)},
        ],
    })];
    let observations = (0..4)
        .map(|index| {
            let y = index + 2;
            let value = source[offset(0x8100 + y) as usize];
            json!({
                "event_index": 10 + index,
                "cycle": 110 + index,
                "pc": 0x8005, "register": 0x4000 + index, "value": value,
                "writer_before": {
                    "pc": 0x8005, "cycle": 117 + index,
                    "x": index, "y": y, "a": value,
                },
                "call_path": [{"entry": {"pc": 0x8000, "cycle": 100}}],
            })
        })
        .collect::<Vec<_>>();
    json!({"entry_argument_reads": reads, "observations": observations})
}

fn write(source: &mut [u8], address: u16, bytes: &[u8]) {
    let start = offset(address) as usize;
    source[start..start + bytes.len()].copy_from_slice(bytes);
}

fn write_mapped(source: &mut [u8], address: u16, bytes: &[u8]) {
    for (offset, byte) in bytes.iter().copied().enumerate() {
        let address = address.checked_add(offset as u16).unwrap();
        source[nrom_offset(address)] = byte;
    }
}

fn offset(address: u16) -> u64 {
    16 + u64::from(address - 0x8000)
}

fn nrom_offset(address: u16) -> usize {
    16 + usize::from((address - 0x8000) & 0x3fff)
}
