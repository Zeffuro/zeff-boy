use super::tests::{capture, fixture};
use super::*;
use anyhow::Result;

fn source(caller: &[u8]) -> Vec<u8> {
    let mut source = fixture();
    source[16 + 0x80..16 + 0x80 + caller.len()].copy_from_slice(caller);
    let child = [
        0xa2, 0, 0xb9, 0, 0x81, 0x9d, 0, 0x40, 0xc8, 0xe8, 0xe0, 4, 0xd0, 0xf4, 0x60,
    ];
    source[16 + 0x200..16 + 0x200 + child.len()].copy_from_slice(&child);
    source[16 + 0x100..16 + 0x108].copy_from_slice(&[0xbf, 0, 0x40, 8, 0xbf, 0, 0x80, 8]);
    source
}

fn inspect(source: &[u8], button: bool) -> Result<Value> {
    let (row, inventory, trace) = capture(source, button)?;
    let result = observe(source, &trace, &row, &inventory, &AtomicBool::new(false));
    assert_eq!(result["status"], "complete", "{result}");
    Ok(result)
}

#[test]
fn native_writer_mask_and_two_distinct_records_are_source_bound() -> Result<()> {
    let source = source(&[
        0x85, 0xad, 0xa5, 0xad, 0x0a, 0x0a, 0xa8, 0x20, 0, 0x82, 0x60,
    ]);
    let mut hashes = Vec::new();
    for button in [false, true] {
        let result = inspect(&source, button)?;
        let links = result["caller_argument_reads"].as_array().unwrap();
        assert_eq!(links.len(), 1);
        let writer = &links[0]["ram_writer"];
        assert_eq!(writer["pc"], 0x8080);
        assert_eq!(writer["source_offset"], 16 + 0x80);
        assert_eq!(writer["value"], u8::from(button));
        assert_eq!(writer["value_constraint"]["values"], json!([0, 1]));
        assert_eq!(writer["value_constraint"]["scope"], "writer_path_only");
        assert!(
            writer["end_cpu_cycle"].as_u64().unwrap()
                <= links[0]["ram_read"]["cpu_cycle"].as_u64().unwrap()
        );
        assert_eq!(
            result["record_extents"].as_array().unwrap().len(),
            1,
            "{result}"
        );
        let candidates = &result["writer_path_records"][0];
        assert_eq!(candidates["global_selector_domain_proven"], false);
        let records = candidates["records"].as_array().unwrap();
        assert_eq!(records.len(), 2);
        for (i, record) in records.iter().enumerate() {
            assert_eq!(record["selector_value"], i);
            assert_eq!(record["index"], i * 4);
            assert_eq!(record["length"], 4);
            assert_eq!(record["source_offset"], 16 + 0x100 + i * 4);
            assert_eq!(record["observed_in_this_call"], i == usize::from(button));
        }
        hashes.push(result["verification"]["native_f32_sha256"].clone());
    }
    assert_ne!(hashes[0], hashes[1]);
    Ok(())
}

#[test]
fn native_read_snapshot_survives_a_later_alias_store() -> Result<()> {
    let source = source(&[
        0x85, 0xad, 0xad, 0xad, 8, 0xa2, 9, 0x86, 0xad, 0x0a, 0x0a, 0xa8, 0x20, 0, 0x82, 0x60,
    ]);
    let result = inspect(&source, true)?;
    let link = &result["caller_argument_reads"][0];
    assert_eq!(link["ram_read"]["address"], 0x8ad);
    assert_eq!(link["ram_writer"]["pc"], 0x8080);
    assert_eq!(link["ram_writer"]["value"], 1);
    assert_eq!(
        link["ram_writer"]["value_constraint"]["values"],
        json!([0, 1])
    );
    Ok(())
}

#[test]
fn native_equal_clobber_and_indexed_store_do_not_keep_old_constraint() -> Result<()> {
    for (prefix, writer_pc, constraint) in [
        (
            vec![0x85, 0xad, 0xa9, 1, 0x8d, 0xad, 8],
            json!(0x8084),
            json!([1]),
        ),
        (
            vec![0x85, 0xad, 0xa2, 0, 0x9d, 0xad, 8],
            Value::Null,
            Value::Null,
        ),
    ] {
        let mut caller = prefix;
        caller.extend_from_slice(&[0xa5, 0xad, 0x0a, 0x0a, 0xa8, 0x20, 0, 0x82, 0x60]);
        let result = inspect(&source(&caller), true)?;
        let writer = &result["caller_argument_reads"][0]["ram_writer"];
        assert_eq!(writer["pc"], writer_pc);
        assert_eq!(writer["value_constraint"]["values"], constraint);
    }
    Ok(())
}

#[test]
fn native_wrapping_or_overlapping_domains_withhold_candidate_records() -> Result<()> {
    for (mask, shifts) in [(0xc0, true), (1, false)] {
        let mut caller = vec![0x29, mask, 0x85, 0xad, 0xa5, 0xad];
        if shifts {
            caller.extend_from_slice(&[0x0a, 0x0a]);
        }
        caller.extend_from_slice(&[0xa8, 0x20, 0, 0x82, 0x60]);
        let result = inspect(&source(&caller), false)?;
        assert_eq!(result["record_extents"].as_array().unwrap().len(), 1);
        assert_eq!(result["writer_path_records"], json!([]));
    }
    Ok(())
}

#[test]
fn native_new_evidence_is_withheld_if_source_or_pcm_verification_fails() -> Result<()> {
    let source = source(&[
        0x85, 0xad, 0xa5, 0xad, 0x0a, 0x0a, 0xa8, 0x20, 0, 0x82, 0x60,
    ]);
    let (mut row, inventory, trace) = capture(&source, false)?;
    row["validation"]["native_reference"]["evidence"]["f32_sha256"] = json!("f".repeat(64));
    let mut changed = source.clone();
    changed[16 + 0x100] ^= 1;
    for result in [
        observe(&source, &trace, &row, &inventory, &AtomicBool::new(false)),
        observe(&source, &trace, &row, &inventory, &AtomicBool::new(true)),
        observe(&changed, &trace, &row, &inventory, &AtomicBool::new(false)),
    ] {
        assert_eq!(result["status"], "unavailable");
        for key in [
            "caller_argument_reads",
            "record_extents",
            "writer_path_records",
        ] {
            assert_eq!(result[key], json!([]));
        }
    }
    Ok(())
}

#[test]
fn record_discontinuities_include_interrupt_brk_idle_and_fail_on_overflow() {
    use zeff_emu_common::debug::DebugEvent;
    let mut boundaries = Vec::new();
    let mut record = InstructionTraceRecord::default();
    record.set_instruction(&[0xea]);
    record_boundary(&mut boundaries, 7, Some(&record)).unwrap();
    assert!(boundaries.is_empty());
    record.set_instruction(&[0x00, 0]);
    record_boundary(&mut boundaries, 9, Some(&record)).unwrap();
    record.event = Some(DebugEvent::Interrupt);
    record.set_instruction(&[]);
    record_boundary(&mut boundaries, 16, Some(&record)).unwrap();
    record_boundary(&mut boundaries, 23, None).unwrap();
    assert_eq!(boundaries, [9, 16, 23]);
    boundaries.resize(4096, 24);
    assert_eq!(
        record_boundary(&mut boundaries, 25, None),
        Err("record_boundary_limit")
    );
}
